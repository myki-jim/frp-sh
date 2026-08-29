//! 信令服务器：axum REST（房间注册/查询）+ UDP 公网探测 + TCP 中继配对。

use super::{
    CreateRoomRequest, CreateRoomResponse, JoinRoomRequest, JoinRoomResponse, RefreshRoomRequest,
    RoomInfo,
};
use crate::utils;
use axum::extract::{Path, State};
use axum::http::StatusCode;
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

#[derive(Debug)]
pub struct Room {
    pub room_id: String,
    pub host_addr: SocketAddr,
    pub guest_addr: Option<SocketAddr>,
    pub created_at: u64,
    pub expires_at: u64,
    /// 房主虚拟网卡 IP（--tun 时通告）
    pub tun_ip: Option<String>,
    /// 中继等待槽位（配对完成前持有连接）
    pub relay_host: Option<TcpStream>,
    pub relay_guest: Option<TcpStream>,
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

pub async fn run_http(listener: TcpListener, state: SharedState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/room/create", post(create_room))
        .route("/room/{id}/join", post(join_room))
        .route("/room/{id}/refresh", post(refresh_room))
        .route("/room/{id}", get(get_room).delete(delete_room))
        .with_state(state);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn create_room(
    State(state): State<SharedState>,
    Json(req): Json<CreateRoomRequest>,
) -> Result<Json<CreateRoomResponse>, (StatusCode, String)> {
    let now = utils::now_unix();
    let room_id = utils::new_room_id(&req.prefix);
    let mut map = state.lock().await;
    // 惰性清理过期房间
    map.retain(|_, r| !r.expired());
    map.insert(
        room_id.clone(),
        Room {
            room_id: room_id.clone(),
            host_addr: req.addr,
            guest_addr: None,
            created_at: now,
            expires_at: now.saturating_add(req.ttl),
            tun_ip: req.tun_ip,
            relay_host: None,
            relay_guest: None,
            pair_notify: Arc::new(Notify::new()),
        },
    );
    Ok(Json(CreateRoomResponse {
        room_id,
        host_addr: req.addr,
    }))
}

async fn join_room(
    State(state): State<SharedState>,
    Path(room_id): Path<String>,
    Json(req): Json<JoinRoomRequest>,
) -> Result<Json<JoinRoomResponse>, (StatusCode, String)> {
    let mut map = state.lock().await;
    let room = map.get_mut(&room_id).ok_or_else(|| not_found(&room_id))?;
    if room.expired() {
        map.remove(&room_id);
        return Err(not_found(&room_id));
    }
    room.guest_addr = Some(req.addr);
    Ok(Json(JoinRoomResponse {
        room_id,
        host_addr: room.host_addr,
    }))
}

async fn get_room(
    State(state): State<SharedState>,
    Path(room_id): Path<String>,
) -> Result<Json<RoomInfo>, (StatusCode, String)> {
    let mut map = state.lock().await;
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
    }))
}

/// 房主重连时刷新自己的公网地址（NAT 映射可能已过期）。
async fn refresh_room(
    State(state): State<SharedState>,
    Path(room_id): Path<String>,
    Json(req): Json<RefreshRoomRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    let mut map = state.lock().await;
    let room = map.get_mut(&room_id).ok_or_else(|| not_found(&room_id))?;
    if room.expired() {
        map.remove(&room_id);
        return Err(not_found(&room_id));
    }
    room.host_addr = req.addr;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_room(State(state): State<SharedState>, Path(room_id): Path<String>) -> StatusCode {
    let mut map = state.lock().await;
    if map.remove(&room_id).is_some() {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
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

pub async fn run_relay(listener: TcpListener, state: SharedState) -> anyhow::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_relay_conn(stream, state).await {
                log::debug!("relay connection ended: {e}");
            }
        });
    }
}

async fn handle_relay_conn(stream: TcpStream, state: SharedState) -> anyhow::Result<()> {
    let (room_id, role, mut stream) = read_hello(stream).await?;

    let mut map = state.lock().await;
    let room = map
        .get_mut(&room_id)
        .ok_or_else(|| anyhow::anyhow!("room {room_id} not found"))?;
    if room.expired() {
        let _ = stream.write_all(b"ERROR ROOM_EXPIRED\r\n").await;
        return Ok(());
    }

    // 对方已在等待 → 立即配对并转发
    let other = if role == "HOST" {
        room.relay_guest.take()
    } else {
        room.relay_host.take()
    };
    if let Some(mut other) = other {
        let notify = room.pair_notify.clone();
        let _ = stream.write_all(b"OK\r\n").await;
        drop(map);
        notify.notify_waiters();
        tokio::spawn(async move {
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut other).await;
        });
        return Ok(());
    }

    // 尚未配对：占住槽位，回复 WAIT，等待对端
    let already = if role == "HOST" {
        room.relay_host.is_some()
    } else {
        room.relay_guest.is_some()
    };
    if already {
        let _ = stream.write_all(b"ERROR ALREADY_CONNECTED\r\n").await;
        return Ok(());
    }
    let _ = stream.write_all(b"WAIT\r\n").await;
    {
        let slot = if role == "HOST" {
            &mut room.relay_host
        } else {
            &mut room.relay_guest
        };
        *slot = Some(stream);
    }
    let notify = room.pair_notify.clone();
    drop(map);

    tokio::select! {
        _ = tokio::time::sleep(RELAY_PAIR_TIMEOUT) => {
            let mut map = state.lock().await;
            if let Some(room) = map.get_mut(&room_id) {
                let slot = if role == "HOST" {
                    &mut room.relay_host
                } else {
                    &mut room.relay_guest
                };
                if let Some(mut s) = slot.take() {
                    let _ = s.write_all(b"ERROR NO_PEER\r\n").await;
                }
            }
            Ok(())
        }
        _ = notify.notified() => {
            let map = state.lock().await;
            let consumed = map
                .get(&room_id)
                .map(|r| {
                    if role == "HOST" {
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

/// 读取 `HELLO <room_id> <HOST|GUEST>\r\n` 行。
///
/// 逐字节读取，避免 BufReader 缓冲残留吞掉对端随后发来的数据（如 CNEW）。
async fn read_hello(stream: TcpStream) -> anyhow::Result<(String, String, TcpStream)> {
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
    if parts.len() != 3 || parts[0] != "HELLO" {
        let _ = wr.write_all(b"ERROR BAD_HELLO\r\n").await;
        return Err(anyhow::anyhow!("relay: bad hello line"));
    }
    let (room_id, role) = (parts[1].to_string(), parts[2].to_string());
    if role != "HOST" && role != "GUEST" {
        let _ = wr.write_all(b"ERROR BAD_ROLE\r\n").await;
        return Err(anyhow::anyhow!("relay: bad role {role}"));
    }
    let stream = rd.reunite(wr)?;
    Ok((room_id, role, stream))
}
