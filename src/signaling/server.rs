//! 信令服务器：axum REST（房间注册/查询）+ UDP 公网探测 + TCP 中继配对。

use super::{
    CreateRoomRequest, CreateRoomResponse, JoinRoomRequest, JoinRoomResponse, RefreshRoomRequest,
    RoomInfo,
};
use crate::utils;
use axum::extract::{Path, Request, State};
use axum::http::StatusCode;
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

/// 活跃 TCP 中继转发数（panel 用；配对成功 +1，转发结束 -1）。
pub static ACTIVE_RELAYS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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
    /// 设备上下行（面板房间详情）：uuid -> 字节计数；":host" 为房主总上下行。
    /// 客户端经 `POST /api/traffic` 周期上报（计数单调，取 max 合并）。
    pub traffic: HashMap<String, TrafficRow>,
    /// 中继等待槽位（配对完成前持有连接）
    pub relay_host: Option<TcpStream>,
    pub relay_guest: Option<TcpStream>,
    /// 网格模式中继：uuid -> 等待配对的对端连接（HOST/GUEST 各一张表）
    pub relay_hosts: HashMap<String, TcpStream>,
    pub relay_guests: HashMap<String, TcpStream>,
    /// 配对完成通知
    pub pair_notify: Arc<Notify>,
}

/// 单台设备的上下行累计字节（设备自身视角：up = 发出，down = 收到）。
#[derive(Debug, Clone, Copy)]
pub struct TrafficRow {
    pub up: u64,
    pub down: u64,
    pub ts: u64,
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
        .route("/room/{id}", get(get_room).delete(delete_room))
        .route("/api/traffic", post(report_traffic))
        .merge(crate::panel::server_routes())
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
/// 面板策略：`/panel` 页面本身免认证（渲染登录框）；`/api/panel/*` 数据接口
/// 与其他 REST 一样需要 token（服务器设置了密码时）。
async fn auth_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, (StatusCode, String)> {
    let path = req.uri().path().to_string();
    if path == "/version" || path == "/health" || path == "/panel" || path == "/" {
        return Ok(next.run(req).await);
    }
    if let Some(pw) = &state.password {
        // 面板数据接口：token 可走 header 或 query（浏览器 WebSocket 友好）
        let token = req
            .headers()
            .get(AUTH_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
            .or_else(|| {
                req.uri().query().and_then(|q| {
                    q.split('&').find_map(|kv| {
                        let (k, v) = kv.split_once('=')?;
                        if k == "token" {
                            Some(v.to_string())
                        } else {
                            None
                        }
                    })
                })
            });
        match token {
            Some(t) if constant_time_eq(&t, pw) => {}
            _ => return Err((StatusCode::UNAUTHORIZED, "unauthorized".into())),
        }
    }
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
    // 生成不冲突的房间号（4 位数字码空间小，撞号时重试；活房间数远小于 10000）
    let mut room_id = utils::new_room_id(&req.prefix);
    for _ in 0..50 {
        if !map.contains_key(&room_id) {
            break;
        }
        room_id = utils::new_room_id(&req.prefix);
    }
    map.insert(
        room_id.clone(),
        Room {
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
            traffic: HashMap::new(),
            relay_host: None,
            relay_guest: None,
            relay_hosts: HashMap::new(),
            relay_guests: HashMap::new(),
            pair_notify: Arc::new(Notify::new()),
        },
    );
    Ok(Json(CreateRoomResponse {
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
        server_turn: state.turn_public,
    }))
}

/// 房主重连时刷新自己的公网地址与局域网信息（NAT 映射可能已过期）。
async fn refresh_room(
    State(state): State<AppState>,
    Path(room_id): Path<String>,
    Json(req): Json<RefreshRoomRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut map = state.rooms.lock().await;
    let room = map.get_mut(&room_id).ok_or_else(|| not_found(&room_id))?;
    if room.expired() {
        map.remove(&room_id);
        return Err(not_found(&room_id));
    }
    room.host_addr = req.addr;
    room.host_lan = req.host_lan;
    room.host_subnets = req.host_subnets;
    room.host_turn_relay = req.turn_relay;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_room(State(state): State<AppState>, Path(room_id): Path<String>) -> StatusCode {
    let mut map = state.rooms.lock().await;
    if map.remove(&room_id).is_some() {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

// ---------- 设备上下行上报（面板房间详情） ----------

/// 客户端周期上报的链路计数。
#[derive(serde::Deserialize)]
pub struct TrafficLink {
    /// 对端设备名；未知时为 "*"（服务器在单访客房间自动归属）
    #[serde(default)]
    pub peer: String,
    pub sent: u64,
    pub recv: u64,
}

/// 一次流量上报：`role` 为 host/guest，`name` 为上报设备的服务器侧名称。
#[derive(serde::Deserialize)]
pub struct TrafficReport {
    pub room: String,
    pub role: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub links: Vec<TrafficLink>,
}

/// 记录设备上下行（计数单调：同键取较大值合并）。
fn record_traffic(room: &mut Room, key: &str, up: u64, down: u64) {
    let now = utils::now_unix();
    let e = room.traffic.entry(key.to_string()).or_insert(TrafficRow {
        up: 0,
        down: 0,
        ts: now,
    });
    // 设备进程重启后计数器归零：上次更新超过 60s 视为旧代次/离线，
    // 直接覆盖而非 max 合并，避免面板数值被历史高值永久卡死。
    if now.saturating_sub(e.ts) > 60 {
        e.up = up;
        e.down = down;
    } else {
        e.up = e.up.max(up);
        e.down = e.down.max(down);
    }
    e.ts = now;
}

/// 客户端链路流量上报：房主报每访客链路计数（host 视角），访客报自身上下行。
/// 统一换算为**设备视角**存储（up = 设备发出，down = 设备收到）：
/// 房主视角的 recv 即对端设备的发出量，sent 即对端设备的接收量。
async fn report_traffic(
    State(state): State<AppState>,
    Json(rep): Json<TrafficReport>,
) -> StatusCode {
    let mut map = state.rooms.lock().await;
    let room = match map.get_mut(&rep.room) {
        Some(r) if !r.expired() => r,
        _ => return StatusCode::NOT_FOUND,
    };
    match rep.role.as_str() {
        "host" => {
            let mut host_up = 0u64;
            let mut host_down = 0u64;
            let guest_uuids: Vec<(String, Option<String>)> = room
                .guests
                .values()
                .map(|g| (g.uuid.clone(), g.name.clone()))
                .collect();
            for l in &rep.links {
                host_up += l.sent;
                host_down += l.recv;
                // 房主视角 → 设备视角翻转
                let (dev_up, dev_down) = (l.recv, l.sent);
                let uuid = guest_uuids
                    .iter()
                    .find(|(_, n)| {
                        !l.peer.is_empty() && l.peer != "*" && n.as_deref() == Some(l.peer.as_str())
                    })
                    .map(|(u, _)| u.clone())
                    .or_else(|| {
                        // 名字未匹配：单访客房间直接归属
                        if guest_uuids.len() == 1 {
                            guest_uuids.first().map(|(u, _)| u.clone())
                        } else {
                            None
                        }
                    });
                if let Some(u) = uuid {
                    record_traffic(room, &u, dev_up, dev_down);
                }
            }
            if host_up > 0 || host_down > 0 {
                record_traffic(room, ":host", host_up, host_down);
            }
        }
        "guest" => {
            // 访客按服务器侧名称归属到自身 uuid
            let uuid = room
                .guests
                .values()
                .find(|g| g.name.as_deref() == Some(rep.name.as_str()))
                .map(|g| g.uuid.clone())
                .or_else(|| {
                    if room.guests.len() == 1 {
                        room.guests.values().next().map(|g| g.uuid.clone())
                    } else {
                        None
                    }
                });
            if let Some(u) = uuid {
                let mut up = 0u64;
                let mut down = 0u64;
                for l in &rep.links {
                    up += l.sent;
                    down += l.recv;
                }
                record_traffic(room, &u, up, down);
            }
        }
        _ => return StatusCode::BAD_REQUEST,
    }
    StatusCode::NO_CONTENT
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
const RELAY_PAIR_TIMEOUT: Duration = Duration::from_secs(600);

pub async fn run_relay(
    listener: TcpListener,
    state: SharedState,
    password: Option<String>,
) -> anyhow::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        // 短周期 TCP 保活：对端断网/切换网络后约 15s 内感知，关闭连接并让对端重连
        let stream = match crate::p2p::relay::enable_keepalive(stream) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("relay keepalive setup failed, dropping connection: {e}");
                continue;
            }
        };
        let state = state.clone();
        let password = password.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_relay_conn(stream, state, password).await {
                log::debug!("relay connection ended: {e}");
            }
        });
    }
}

/// 将中继连接按需包装为加密流（服务器设置了密码时）。
fn maybe_encrypt<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    password: Option<&str>,
) -> Box<dyn crate::p2p::relay::AsyncReadWrite> {
    match password {
        Some(pw) => {
            let key = crate::p2p::enc::key_from_password(pw);
            Box::new(crate::p2p::enc::EncStream::new(stream, &key))
        }
        None => Box::new(stream),
    }
}

async fn handle_relay_conn(
    stream: TcpStream,
    state: SharedState,
    password: Option<String>,
) -> anyhow::Result<()> {
    let (room_id, role, mut stream, token, peer_uuid) = read_hello(stream).await?;

    // 服务器设置了密码 → 校验客户端携带的 token，并启用中继流加密
    if let Some(pw) = &password {
        let ok = token.as_deref() == Some(pw.as_str());
        if !ok {
            let _ = stream.write_all(b"ERROR AUTH_FAILED\r\n").await;
            return Ok(());
        }
    }

    let mut map = state.lock().await;
    let room = map
        .get_mut(&room_id)
        .ok_or_else(|| anyhow::anyhow!("room {room_id} not found"))?;
    if room.expired() {
        let _ = stream.write_all(b"ERROR ROOM_EXPIRED\r\n").await;
        return Ok(());
    }

    // 对方已在等待 → 立即配对并转发（服务器有密码时两侧均加密）。
    // 网格模式（携带 uuid）按 uuid 配对；旧协议用单槽位。
    let other = if let Some(u) = &peer_uuid {
        if role == "HOST" {
            room.relay_guests.remove(u)
        } else {
            room.relay_hosts.remove(u)
        }
    } else if role == "HOST" {
        room.relay_guest.take()
    } else {
        room.relay_host.take()
    };
    if let Some(other) = other {
        let notify = room.pair_notify.clone();
        let _ = stream.write_all(b"OK\r\n").await;
        drop(map);
        notify.notify_waiters();
        let pw = password.clone();
        ACTIVE_RELAYS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        tokio::spawn(async move {
            let pw_opt = pw.as_deref();
            let mut a = maybe_encrypt(stream, pw_opt);
            let mut b = maybe_encrypt(other, pw_opt);
            let _ = tokio::io::copy_bidirectional(&mut a, &mut b).await;
            ACTIVE_RELAYS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        });
        return Ok(());
    }

    // 尚未配对：占住槽位（uuid 键控或单槽），回复 WAIT，等待对端
    let already = if let Some(u) = &peer_uuid {
        let slot = if role == "HOST" {
            &room.relay_hosts
        } else {
            &room.relay_guests
        };
        slot.contains_key(u)
    } else if role == "HOST" {
        room.relay_host.is_some()
    } else {
        room.relay_guest.is_some()
    };
    if already {
        // 同 uuid 的新连接只可能来自客户端重连（旧连接必然已死）：
        // 不再拒绝，改为顶掉旧槽位，避免僵尸 WAIT 连接把设备永久挡在门外。
        if let Some(u) = &peer_uuid {
            let slot = if role == "HOST" {
                &mut room.relay_hosts
            } else {
                &mut room.relay_guests
            };
            slot.remove(u);
        } else if role == "HOST" {
            room.relay_host.take();
        } else {
            room.relay_guest.take();
        }
    }
    let _ = stream.write_all(b"WAIT\r\n").await;
    {
        if let Some(u) = &peer_uuid {
            let slot = if role == "HOST" {
                &mut room.relay_hosts
            } else {
                &mut room.relay_guests
            };
            slot.insert(u.clone(), stream);
        } else {
            let slot = if role == "HOST" {
                &mut room.relay_host
            } else {
                &mut room.relay_guest
            };
            *slot = Some(stream);
        }
    }
    let notify = room.pair_notify.clone();
    drop(map);

    // uuid 槽位的等待者不监听 pair_notify：任何无关配对的 notify 都会把它唤醒并
    // 提前返回，导致超时清理被跳过、僵尸连接永久占槽（对端重连被 ALREADY 挡死）。
    // uuid 配对由配对分支直接消费槽内流；等待者只需保留超时兜底。
    // 单槽位（旧协议）无 uuid 可查，仍靠 notify 判断"槽已被消费"以提前收尾。
    let uuid_waiter = peer_uuid.is_some();
    tokio::select! {
        _ = tokio::time::sleep(RELAY_PAIR_TIMEOUT) => {
            let mut map = state.lock().await;
            if let Some(room) = map.get_mut(&room_id) {
                let slot_err = if let Some(u) = &peer_uuid {
                    let slot = if role == "HOST" {
                        &mut room.relay_hosts
                    } else {
                        &mut room.relay_guests
                    };
                    slot.remove(u)
                } else {
                    let slot = if role == "HOST" {
                        &mut room.relay_host
                    } else {
                        &mut room.relay_guest
                    };
                    slot.take()
                };
                if let Some(mut s) = slot_err {
                    let _ = s.write_all(b"ERROR NO_PEER\r\n").await;
                }
            }
            Ok(())
        }
        _ = async {
            if uuid_waiter {
                std::future::pending::<()>().await
            } else {
                notify.notified().await
            }
        } => {
            let map = state.lock().await;
            let consumed = map
                .get(&room_id)
                .map(|r| {
                    if peer_uuid.is_some() {
                        true // uuid 槽位不参与 notify 配对，直接视为已消费
                    } else if role == "HOST" {
                        r.relay_host.is_none()
                    } else {
                        r.relay_guest.is_none()
                    }
                })
                .unwrap_or(true);
            drop(map);
            if consumed {
                Ok(())
            } else {
                Err(anyhow::anyhow!("relay pairing state inconsistent for {room_id}"))
            }
        }
    }
}

/// 读取 `HELLO <room_id> <HOST|GUEST> [token] [uuid]\r\n` 行。
///
/// token 为可选第 4 字段（服务器设置密码时客户端携带）；
/// uuid 为可选第 5 字段（网格模式多访客时用于配对到指定对端）。
/// 逐字节读取，避免 BufReader 缓冲残留吞掉对端随后发来的数据（如 CNEW）。
async fn read_hello(
    stream: TcpStream,
) -> anyhow::Result<(String, String, TcpStream, Option<String>, Option<String>)> {
    stream.set_nodelay(true)?;
    let (mut rd, mut wr) = stream.into_split();
    let mut line = Vec::with_capacity(64);
    let mut byte = [0u8; 1];
    loop {
        let n = tokio::time::timeout(Duration::from_secs(10), rd.read(&mut byte)).await??;
        if n == 0 {
            return Err(anyhow::anyhow!("relay: connection closed during hello"));
        }
        if byte[0] == b'\n' {
            break;
        }
        if line.len() < 256 {
            line.push(byte[0]);
        }
    }
    let text = String::from_utf8_lossy(&line);
    let parts: Vec<&str> = text.split_whitespace().collect();
    if parts.len() < 3 || parts.len() > 5 || parts[0] != "HELLO" {
        let _ = wr.write_all(b"ERROR BAD_HELLO\r\n").await;
        return Err(anyhow::anyhow!("relay: bad hello line"));
    }
    let (room_id, role) = (parts[1].to_string(), parts[2].to_string());
    if role != "HOST" && role != "GUEST" {
        let _ = wr.write_all(b"ERROR BAD_ROLE\r\n").await;
        return Err(anyhow::anyhow!("relay: bad role {role}"));
    }
    // 字段顺序：HELLO <room> <ROLE> [token] [uuid]
    let token = parts.get(3).map(|s| s.to_string());
    let uuid = parts.get(4).map(|s| s.to_string());
    let stream = rd.reunite(wr)?;
    Ok((room_id, role, stream, token, uuid))
}
