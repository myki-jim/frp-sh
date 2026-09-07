//! 信令服务器：axum REST（房间注册/查询）+ UDP 公网探测 + TCP 中继配对。

use super::{
    CreateRoomRequest, CreateRoomResponse, JoinRoomRequest, JoinRoomResponse, RefreshRoomRequest,
    RoomInfo,
};
use crate::utils;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::{Mutex, Notify};

/// 房间注册表：room_id -> Room
pub type SharedState = Arc<Mutex<HashMap<String, Room>>>;

/// 请求认证头（客户端携带服务器密码）。
pub const AUTH_HEADER: &str = "X-Frp-Sh-Token";

/// 服务器应用状态：房间注册表 + 可选密码（按实例传递，避免全局污染）。
#[derive(Clone)]
pub struct AppState {
    pub rooms: SharedState,
    pub password: Option<String>,
    /// 内置 TURN 的公网地址（`--turn` + 启用密码认证时下发 RoomInfo.server_turn；
    /// 凭据复用服务器密码，客户端无需任何配置即可使用）
    pub turn_public: Option<SocketAddr>,
}

#[derive(Debug)]
pub struct Room {
    pub owner_token: String,
    pub access_token: Option<String>,
    pub services: Vec<crate::services::ServiceInfo>,
    pub cancelled: tokio_util::sync::CancellationToken,
    pub room_id: String,
    pub host_addr: SocketAddr,
    pub guest_addr: Option<SocketAddr>,
    pub created_at: u64,
    pub expires_at: u64,
    /// 房主虚拟网卡 IP（--tun 时通告）
    pub tun_ip: Option<String>,
    /// 房主局域网打洞地址（同局域网直连）
    pub host_lan: Vec<SocketAddr>,
    /// 房主局域网子网 CIDR（--tun 时访客据此加路由）
    pub host_subnets: Vec<String>,
    /// 房主分配的 TURN relay 地址
    pub host_turn_relay: Option<SocketAddr>,
    /// 访客局域网打洞地址（房主反向打洞）
    pub guest_lan: Vec<SocketAddr>,
    /// 访客暴露的局域网子网 CIDR（`--expose-lan`）
    pub guest_subnets: Vec<String>,
    /// 访客分配的 TURN relay 地址
    pub guest_turn_relay: Option<SocketAddr>,
    /// 房主预留的访客虚拟 IP 池（`--guest-ips`）
    pub guest_ips: Vec<String>,
    /// 房主程序版本（`major.minor.patch`）
    pub host_version: String,
    /// 房主设备显示名（默认主机名）
    pub host_name: Option<String>,
    /// 已分配：visitor UUID -> 虚拟 IP（重连复用）
    pub ip_assignments: HashMap<String, String>,
    /// 全部访客（网格模式多访客）：uuid -> 访客信息
    pub guests: HashMap<String, super::GuestInfo>,
    /// 中继等待槽位（配对完成前持有连接）
    pub relay_host: Option<PendingRelay>,
    pub relay_guest: Option<PendingRelay>,
    /// 网格模式中继：uuid -> 等待配对的对端连接（HOST/GUEST 各一张表）
    pub relay_hosts: HashMap<String, PendingRelay>,
    pub relay_guests: HashMap<String, PendingRelay>,
    /// 配对完成通知
    pub pair_notify: Arc<Notify>,
}

impl Room {
    fn expired(&self) -> bool {
        utils::now_unix() >= self.expires_at
    }
}

pub fn new_state() -> SharedState {
    Arc::new(Mutex::new(HashMap::new()))
}

// ---------- HTTP REST ----------

pub async fn run_http(
    listener: TcpListener,
    state: SharedState,
    password: Option<String>,
    turn_public: Option<SocketAddr>,
) -> anyhow::Result<()> {
    let app_state = AppState {
        rooms: state,
        password: password.clone(),
        turn_public,
    };
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/version", get(version_info))
        .route("/room/create", post(create_room))
        .route("/room/{id}/join", post(join_room))
        .route("/room/{id}/refresh", post(refresh_room))
        .route("/room/{id}/secure", post(secure_room))
        .route("/room/{id}/services", post(set_services))
        .route("/room/{id}", get(get_room).delete(delete_room))
        .with_state(app_state.clone())
        .layer(axum::middleware::from_fn_with_state(
            app_state,
            auth_middleware,
        ));
    axum::serve(listener, app).await?;
    Ok(())
}

/// 请求认证：服务器设置了密码时校验 `X-Frp-Sh-Token`（/version 与 /health 免认证）。
///
async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let path = req.uri().path().to_string();
    if path == "/version" || path == "/health" {
        return Ok(next.run(req).await);
    }
    let token = req
        .headers()
        .get(AUTH_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let admin = state
        .password
        .as_deref()
        .is_some_and(|pw| constant_time_eq(token, pw));
    let room_id = path
        .strip_prefix("/room/")
        .and_then(|p| p.split('/').next());
    let rooms = state.rooms.lock().await;
    let room = room_id.and_then(|id| rooms.get(id));
    let member = room.is_some_and(|r| {
        !r.expired()
            && r.access_token
                .as_deref()
                .is_some_and(|t| constant_time_eq(token, t))
    });
    let protected = room.is_some_and(|r| r.access_token.is_some());
    if !(admin || member || (state.password.is_none() && !protected && !token.starts_with("r3_"))) {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".into()));
    }
    drop(rooms);
    Ok(next.run(req).await)
}

/// 常数时间字符串比较（防时序侧信道）。
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// 返回服务器版本、线协议版本与是否启用密码认证。
async fn version_info(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "version": crate::version::VERSION,
        "protocol": crate::version::PROTOCOL_VERSION,
        "auth": state.password.is_some(),
    }))
}

async fn create_room(
    State(state): State<AppState>,
    Json(req): Json<CreateRoomRequest>,
) -> Result<Json<CreateRoomResponse>, (StatusCode, String)> {
    let now = utils::now_unix();
    let mut map = state.rooms.lock().await;
    // 惰性清理过期房间
    map.retain(|_, r| !r.expired());
    if map.len() >= 1024 || req.ttl == 0 || req.ttl > 7 * 24 * 3600 {
        return Err((
            StatusCode::BAD_REQUEST,
            "invalid TTL or room capacity".into(),
        ));
    }
    // 生成不冲突的房间号（4 位数字码空间小，撞号时重试；活房间数远小于 10000）
    let mut room_id = utils::new_room_id(&req.prefix);
    for _ in 0..50 {
        if !map.contains_key(&room_id) {
            break;
        }
        room_id = utils::new_room_id(&req.prefix);
    }
    if map.contains_key(&room_id) {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "room capacity exhausted".into(),
        ));
    }
    if req.host_lan.len() > 16
        || req.host_subnets.len() > 16
        || req.guest_ips.len() > 32
        || req.prefix.len() > 24
        || !req
            .prefix
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err((StatusCode::BAD_REQUEST, "invalid room metadata".into()));
    }
    let owner_token = utils::random_hex(32);
    map.insert(
        room_id.clone(),
        Room {
            owner_token: owner_token.clone(),
            access_token: None,
            services: Vec::new(),
            cancelled: tokio_util::sync::CancellationToken::new(),
            room_id: room_id.clone(),
            host_addr: req.addr,
            guest_addr: None,
            created_at: now,
            expires_at: now.saturating_add(req.ttl),
            tun_ip: req.tun_ip,
            host_lan: req.host_lan,
            host_subnets: req.host_subnets,
            guest_lan: Vec::new(),
            guest_subnets: Vec::new(),
            host_turn_relay: req.turn_relay,
            guest_turn_relay: None,
            guest_ips: req.guest_ips,
            host_version: req.version,
            host_name: req.name,
            ip_assignments: HashMap::new(),
            guests: HashMap::new(),
            relay_host: None,
            relay_guest: None,
            relay_hosts: HashMap::new(),
            relay_guests: HashMap::new(),
            pair_notify: Arc::new(Notify::new()),
        },
    );
    Ok(Json(CreateRoomResponse {
        owner_token,
        room_id,
        host_addr: req.addr,
    }))
}

async fn join_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(req): Json<JoinRoomRequest>,
) -> Result<Json<JoinRoomResponse>, (StatusCode, String)> {
    let mut map = state.rooms.lock().await;
    let room = map.get_mut(&room_id).ok_or_else(|| not_found(&room_id))?;
    if room.expired() {
        map.remove(&room_id);
        return Err(not_found(&room_id));
    }
    if req.addr_lan.len() > 16
        || req.guest_subnets.len() > 16
        || req
            .visitor_id
            .as_ref()
            .is_some_and(|s| s.len() > 128 || s.chars().any(char::is_whitespace))
        || req
            .name
            .as_ref()
            .is_some_and(|s| s.len() > 80 || s.chars().any(char::is_control))
    {
        return Err((StatusCode::BAD_REQUEST, "invalid member metadata".into()));
    }
    if room.guests.len() >= 32
        && !req
            .visitor_id
            .as_ref()
            .is_some_and(|id| room.guests.contains_key(id))
    {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            "room member limit reached".into(),
        ));
    }
    if let Some(ip) = &req.requested_ip {
        if !ip
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_private())
            || room.tun_ip.as_ref() == Some(ip)
            || room.guests.iter().any(|(id, g)| {
                Some(id) != req.visitor_id.as_ref() && g.vnet_ip.as_ref() == Some(ip)
            })
        {
            return Err((
                StatusCode::CONFLICT,
                "virtual address is invalid or already assigned".into(),
            ));
        }
    }
    // 虚拟 IP 分配：访客显式指定 > UUID 复用 > 按序取池中未分配的
    let assigned_ip = if room.guest_ips.is_empty() {
        req.requested_ip.or_else(|| {
            req.visitor_id
                .as_ref()
                .and_then(|id| room.ip_assignments.get(id).cloned())
        })
    } else {
        let taken: std::collections::HashSet<&str> =
            room.ip_assignments.values().map(String::as_str).collect();
        // 已分配给该设备的 IP 优先复用（重连稳定）
        if let Some(uid) = &req.visitor_id {
            if let Some(ip) = room.ip_assignments.get(uid) {
                Some(ip.clone())
            } else {
                // 显式指定的 IP 若在池内则用（占位检查）
                match &req.requested_ip {
                    Some(ip) if room.guest_ips.contains(ip) && !taken.contains(ip.as_str()) => {
                        room.ip_assignments.insert(uid.clone(), ip.clone());
                        Some(ip.clone())
                    }
                    _ => room
                        .guest_ips
                        .iter()
                        .find(|ip| !taken.contains(ip.as_str()))
                        .map(|ip| {
                            room.ip_assignments.insert(uid.clone(), ip.clone());
                            ip.clone()
                        }),
                }
            }
        } else {
            None
        }
    };
    // 网格模式：按 UUID 登记访客（重连复用同一条目，地址更新）
    let guest_uuid = req
        .visitor_id
        .clone()
        .unwrap_or_else(|| format!("anon-{}", req.addr));
    // 设备显示名：房内去重（重名自动加 -2/-3 后缀）
    let base_name = req
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| req.addr.ip().to_string());
    let taken_names: std::collections::HashSet<String> = room
        .guests
        .values()
        .filter(|g| g.uuid != guest_uuid)
        .filter_map(|g| g.name.clone())
        .collect();
    let mut device_name = base_name.clone();
    let mut n = 2;
    while taken_names.contains(&device_name) {
        device_name = format!("{base_name}-{n}");
        n += 1;
    }
    log::info!(
        "room {room_id}: guest {} ({device_name}) joined from {}, vnet {}",
        guest_uuid,
        req.addr,
        assigned_ip.as_deref().unwrap_or("-")
    );
    room.guests.insert(
        guest_uuid.clone(),
        super::GuestInfo {
            uuid: guest_uuid,
            name: Some(device_name.clone()),
            addr: req.addr,
            lan: req.addr_lan.clone(),
            vnet_ip: assigned_ip.clone(),
            subnets: req.guest_subnets.clone(),
            turn_relay: req.turn_relay,
        },
    );
    // 兼容旧客户端：guest_addr 保持"最近加入者"视图
    room.guest_addr = Some(req.addr);
    room.guest_lan = req.addr_lan;
    room.guest_subnets = req.guest_subnets;
    room.guest_turn_relay = req.turn_relay;
    Ok(Json(JoinRoomResponse {
        room_id,
        host_addr: room.host_addr,
        assigned_ip,
        name: Some(device_name.clone()),
    }))
}

async fn get_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
) -> Result<Json<RoomInfo>, (StatusCode, String)> {
    let mut map = state.rooms.lock().await;
    let room = map.get_mut(&room_id).ok_or_else(|| not_found(&room_id))?;
    if room.expired() {
        map.remove(&room_id);
        return Err(not_found(&room_id));
    }
    Ok(Json(RoomInfo {
        access_token: room.access_token.clone(),
        services: room.services.clone(),
        room_id: room.room_id.clone(),
        host_addr: room.host_addr,
        guest_addr: room.guest_addr,
        created_at: room.created_at,
        expires_at: room.expires_at,
        tun_ip: room.tun_ip.clone(),
        host_lan: room.host_lan.clone(),
        host_subnets: room.host_subnets.clone(),
        host_turn_relay: room.host_turn_relay,
        guest_lan: room.guest_lan.clone(),
        guest_subnets: room.guest_subnets.clone(),
        guest_turn_relay: room.guest_turn_relay,
        guests: room.guests.values().cloned().collect(),
        host_version: room.host_version.clone(),
        host_name: room.host_name.clone(),
        server_turn: if room.access_token.is_some() {
            None
        } else {
            state.turn_public
        },
    }))
}

/// 房主重连时刷新自己的公网地址与局域网信息（NAT 映射可能已过期）。
async fn refresh_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<RefreshRoomRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut map = state.rooms.lock().await;
    let room = map.get_mut(&room_id).ok_or_else(|| not_found(&room_id))?;
    if !owns(&headers, room) {
        return Err((StatusCode::FORBIDDEN, "room owner required".into()));
    }
    if room.expired() {
        map.remove(&room_id);
        return Err(not_found(&room_id));
    }
    room.host_addr = req.addr;
    room.host_lan = req.host_lan;
    room.host_subnets = req.host_subnets;
    room.host_turn_relay = req.turn_relay;
    log::info!("room {room_id}: host refreshed from {}", req.addr);
    Ok(StatusCode::NO_CONTENT)
}

fn owns(headers: &HeaderMap, room: &Room) -> bool {
    headers
        .get("X-Frp-Sh-Room-Token")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| constant_time_eq(t, &room.owner_token))
}
async fn delete_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    headers: HeaderMap,
) -> StatusCode {
    let mut map = state.rooms.lock().await;
    match map.get(&room_id) {
        None => return StatusCode::NOT_FOUND,
        Some(r) if !owns(&headers, r) => return StatusCode::FORBIDDEN,
        _ => {}
    }
    if let Some(room) = map.remove(&room_id) {
        room.cancelled.cancel();
    }
    StatusCode::NO_CONTENT
}

async fn secure_room(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut rooms = state.rooms.lock().await;
    let room = rooms.get_mut(&id).ok_or_else(|| not_found(&id))?;
    if !owns(&headers, room) {
        return Err((StatusCode::FORBIDDEN, "room owner required".into()));
    }
    room.cancelled.cancel();
    room.cancelled = tokio_util::sync::CancellationToken::new();
    room.guests.clear();
    room.relay_hosts.clear();
    room.relay_guests.clear();
    room.relay_host = None;
    room.relay_guest = None;
    let token = format!("r3_{}", utils::random_hex(32));
    room.access_token = Some(token.clone());
    Ok(Json(serde_json::json!({"token":token})))
}
async fn set_services(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(services): Json<Vec<crate::services::ServiceInfo>>,
) -> Result<StatusCode, (StatusCode, String)> {
    crate::services::validate_catalog(&services)
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid service catalog".into()))?;
    let mut rooms = state.rooms.lock().await;
    let room = rooms.get_mut(&id).ok_or_else(|| not_found(&id))?;
    if !owns(&headers, room) || room.access_token.is_none() || room.tun_ip.is_some() {
        return Err((StatusCode::FORBIDDEN, "service room owner required".into()));
    }
    room.services = services;
    Ok(StatusCode::NO_CONTENT)
}

fn not_found(room_id: &str) -> (StatusCode, String) {
    (
        StatusCode::NOT_FOUND,
        format!("room not found or expired: {room_id}"),
    )
}

// ---------- UDP 公网探测 ----------

pub async fn run_udp_echo(socket: UdpSocket) -> anyhow::Result<()> {
    let mut buf = [0u8; 512];
    loop {
        let (len, src) = socket.recv_from(&mut buf).await?;
        let text = String::from_utf8_lossy(&buf[..len]);
        if let Some(rest) = text.strip_prefix("ECHO ") {
            let token = rest.trim();
            if !token.is_empty() && token.len() <= 32 {
                let reply = format!("ADDR {} {}:{}", token, fmt_ip(src), src.port());
                let _ = socket.send_to(reply.as_bytes(), src).await;
            }
        }
    }
}

fn fmt_ip(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V4(v) => v.ip().to_string(),
        SocketAddr::V6(v) => format!("[{}]", v.ip()),
    }
}

// ---------- TCP 中继 ----------

/// 等待配对的最长时间
const RELAY_PAIR_TIMEOUT: Duration = Duration::from_secs(15);

struct PendingSignal(tokio_util::sync::CancellationToken);
impl Drop for PendingSignal {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
pub struct PendingRelay {
    done: PendingSignal,
    id: String,
    stream: crate::p2p::relay::RelayStream,
}
impl std::fmt::Debug for PendingRelay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PendingRelay")
    }
}
pub async fn run_relay(
    listener: TcpListener,
    state: SharedState,
    password: Option<String>,
) -> anyhow::Result<()> {
    let limits = Arc::new(tokio::sync::Semaphore::new(256));
    loop {
        let (stream, _) = listener.accept().await?;
        let Ok(permit) = limits.clone().try_acquire_owned() else {
            continue;
        };
        let state = state.clone();
        let password = password.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(e) = handle_relay_conn(stream, state, password).await {
                log::debug!("relay ended: {e}");
            }
        });
    }
}
async fn handle_relay_conn(
    tcp: TcpStream,
    state: SharedState,
    password: Option<String>,
) -> anyhow::Result<()> {
    tcp.set_nodelay(true)?;
    let mut tcp = crate::p2p::relay::enable_keepalive(tcp)?;
    let mut prefix = [0; 3];
    let n = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let n = tcp.peek(&mut prefix).await?;
            if n == 0 || n >= 3 {
                return Ok::<_, std::io::Error>(n);
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    let mut selected_room = None;
    let password = if n == 3 && &prefix == b"R3 " {
        let mut line = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let b = tcp.read_u8().await?;
                if b == b'\n' {
                    break;
                }
                anyhow::ensure!(line.len() < 140, "room preface too long");
                line.push(b);
            }
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        let id = String::from_utf8(line)?[3..].trim().to_string();
        let rooms = state.lock().await;
        let room = rooms
            .get(&id)
            .filter(|r| !r.expired())
            .ok_or_else(|| anyhow::anyhow!("room unavailable"))?;
        let token = room
            .access_token
            .clone()
            .ok_or_else(|| anyhow::anyhow!("room authorization unavailable"))?;
        selected_room = Some(id);
        Some(token)
    } else {
        password
    };
    let mut stream: crate::p2p::relay::RelayStream = match password {
        Some(p) => Box::new(crate::p2p::enc::EncStream::new(
            tcp,
            &crate::p2p::enc::key_from_password(&p),
        )),
        None => Box::new(tcp),
    };
    let (room_id, role, uuid, owner) = tokio::time::timeout(Duration::from_secs(15), async {
        let mut line = Vec::new();
        loop {
            let b = stream.read_u8().await?;
            if b == b'\n' {
                break;
            }
            if line.len() >= 512 {
                anyhow::bail!("relay hello too long");
            }
            line.push(b);
        }
        let text = String::from_utf8(line)?;
        let v: Vec<_> = text.split_whitespace().collect();
        if v.len() != 5 || v[0] != "HELLO2" || !matches!(v[2], "HOST" | "GUEST") {
            anyhow::bail!("bad relay handshake");
        }
        Ok::<_, anyhow::Error>((
            v[1].to_string(),
            v[2].to_string(),
            v[3].to_string(),
            v[4].to_string(),
        ))
    })
    .await??;
    let mut map = state.lock().await;
    let room = map
        .get_mut(&room_id)
        .ok_or_else(|| anyhow::anyhow!("room not found"))?;
    if room.access_token.is_some() && selected_room.as_deref() != Some(room_id.as_str()) {
        anyhow::bail!("room authorization required");
    }
    if room.expired() || (role == "HOST" && !constant_time_eq(&owner, &room.owner_token)) {
        drop(map);
        stream.write_all(b"ERROR AUTH\r\n").await?;
        stream.flush().await?;
        return Ok(());
    }
    let cancelled = room.cancelled.clone();
    let expires = room.expires_at;
    let other = if uuid == "-" {
        if role == "HOST" {
            room.relay_guest.take()
        } else {
            room.relay_host.take()
        }
    } else if role == "HOST" {
        room.relay_guests.remove(&uuid)
    } else {
        room.relay_hosts.remove(&uuid)
    };
    if let Some(other) = other {
        other.done.0.cancel();
        drop(map);
        stream.write_all(b"OK\r\n").await?;
        stream.flush().await?;
        let mut other = other.stream;
        tokio::select! {
            _ = tokio::io::copy_bidirectional(&mut stream, &mut other) => {},
            _ = cancelled.cancelled() => {},
            _ = tokio::time::sleep(Duration::from_secs(expires.saturating_sub(utils::now_unix()))) => {},
        }
        return Ok(());
    }
    if room.relay_hosts.len() + room.relay_guests.len() >= 128 {
        anyhow::bail!("room relay capacity");
    }
    drop(map);
    stream.write_all(b"WAIT\r\n").await?;
    stream.flush().await?;
    let id = utils::random_hex(16);
    let paired = tokio_util::sync::CancellationToken::new();
    let pending = PendingRelay {
        done: PendingSignal(paired.clone()),
        id: id.clone(),
        stream,
    };
    let mut map = state.lock().await;
    let room = map
        .get_mut(&room_id)
        .ok_or_else(|| anyhow::anyhow!("room removed"))?;
    anyhow::ensure!(
        !cancelled.is_cancelled() && !room.expired(),
        "room authorization expired"
    );
    // Recheck the peer after I/O outside the room lock.
    let other = if uuid == "-" {
        if role == "HOST" {
            room.relay_guest.take()
        } else {
            room.relay_host.take()
        }
    } else if role == "HOST" {
        room.relay_guests.remove(&uuid)
    } else {
        room.relay_hosts.remove(&uuid)
    };
    if let Some(other) = other {
        other.done.0.cancel();
        drop(map);
        let mut a = pending.stream;
        let mut b = other.stream;
        tokio::select! {
            _ = tokio::io::copy_bidirectional(&mut a, &mut b) => {},
            _ = cancelled.cancelled() => {},
            _ = tokio::time::sleep(Duration::from_secs(expires.saturating_sub(utils::now_unix()))) => {},
        }
        return Ok(());
    }
    if uuid == "-" {
        if role == "HOST" {
            room.relay_host = Some(pending)
        } else {
            room.relay_guest = Some(pending)
        }
    } else if role == "HOST" {
        room.relay_hosts.insert(uuid.clone(), pending);
    } else {
        room.relay_guests.insert(uuid.clone(), pending);
    }
    drop(map);
    tokio::select! {_=tokio::time::sleep(RELAY_PAIR_TIMEOUT)=>{},_=paired.cancelled()=>return Ok(()),_=cancelled.cancelled()=>return Ok(())}
    let mut map = state.lock().await;
    if let Some(room) = map.get_mut(&room_id) {
        if uuid == "-" {
            let slot = if role == "HOST" {
                &mut room.relay_host
            } else {
                &mut room.relay_guest
            };
            if slot.as_ref().is_some_and(|p| p.id == id) {
                slot.take();
            }
        } else {
            let slots = if role == "HOST" {
                &mut room.relay_hosts
            } else {
                &mut room.relay_guests
            };
            if slots.get(&uuid).is_some_and(|p| p.id == id) {
                slots.remove(&uuid);
            }
        }
    }
    Ok(())
}
