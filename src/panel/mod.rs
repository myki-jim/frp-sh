//! Web 面板：服务端（信令端口挂载）与客户端（默认 127.0.0.1:6793）。
//!
//! 前端为单文件 HTML（MDUI 2 走 CDN + 内嵌扁平 CSS 兜底），`include_str!`
//! 编译期打入二进制；数据经 WebSocket 定时推送，无轮询。

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use std::time::Duration;

const CLIENT_HTML: &str = include_str!("client.html");
const SERVER_HTML: &str = include_str!("server.html");

/// 服务端启动时刻（uptime 用）。
static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// serve 启动时调用，锚定 uptime 起点。
pub fn init_uptime() {
    let _ = START.get_or_init(std::time::Instant::now);
}

fn uptime_secs() -> u64 {
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_secs()
}

// ---------- 服务端面板（挂载到信令 Router） ----------

pub fn server_routes() -> Router<crate::signaling::server::AppState> {
    Router::new()
        .route("/panel", get(panel_html))
        .route("/api/panel/summary", get(summary))
        .route("/api/panel/rooms", get(rooms))
        .route("/api/panel/ws", get(ws_handler))
}

async fn panel_html() -> Html<&'static str> {
    Html(SERVER_HTML)
}

async fn summary(
    State(state): State<crate::signaling::server::AppState>,
) -> Json<serde_json::Value> {
    let rooms = state.rooms.lock().await;
    let guests: usize = rooms.values().map(|r| r.guests.len()).sum();
    let relay_links =
        crate::signaling::server::ACTIVE_RELAYS.load(std::sync::atomic::Ordering::Relaxed);
    Json(json!({
        "version": crate::version::VERSION,
        "protocol": crate::version::PROTOCOL_VERSION,
        "uptime_s": uptime_secs(),
        "rooms": rooms.len(),
        "guests": guests,
        "relay_links": relay_links,
        "turn_allocs": crate::p2p::turn_server::TURN_ALLOCS
            .load(std::sync::atomic::Ordering::Relaxed),
        "auth": state.password.is_some(),
    }))
}

async fn rooms(State(state): State<crate::signaling::server::AppState>) -> Json<serde_json::Value> {
    let rooms = state.rooms.lock().await;
    let now = crate::utils::now_unix();
    let list: Vec<serde_json::Value> = rooms
        .values()
        .map(|r| {
            let guests: Vec<serde_json::Value> = r
                .guests
                .values()
                .map(|g| {
                    json!({
                        "uuid": g.uuid,
                        "name": g.name,
                        "addr": g.addr.to_string(),
                        "lan": g.lan.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
                        "vnet_ip": g.vnet_ip,
                        "turn_relay": g.turn_relay.map(|a| a.to_string()),
                    })
                })
                .collect();
            json!({
                "room_id": r.room_id,
                "created_at": r.created_at,
                "ttl_left_s": r.expires_at.saturating_sub(now),
                "host": {
                    "name": r.host_name,
                    "addr": r.host_addr.to_string(),
                    "version": r.host_version,
                    "vnet_ip": r.tun_ip,
                    "lan": r.host_lan.iter().map(|a| a.to_string()).collect::<Vec<_>>(),
                    "turn_relay": r.host_turn_relay.map(|a| a.to_string()),
                },
                "guests": guests,
                "relay_pairs": crate::signaling::server::ACTIVE_RELAYS
                    .load(std::sync::atomic::Ordering::Relaxed),
            })
        })
        .collect();
    Json(json!(list))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<crate::signaling::server::AppState>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| server_ws_loop(socket, state))
}

async fn server_ws_loop(mut socket: WebSocket, state: crate::signaling::server::AppState) {
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    loop {
        tokio::select! {
            _ = tick.tick() => {
                let summary = summary(State(state.clone())).await.0;
                let rooms = rooms(State(state.clone())).await.0;
                let payload = json!({"summary": summary, "rooms": rooms});
                if socket.send(Message::Text(payload.to_string().into())).await.is_err() {
                    break;
                }
            }
            msg = socket.recv() => {
                if matches!(msg, None | Some(Err(_))) { break; }
            }
        }
    }
}

// ---------- 客户端面板（独立小服务，默认 127.0.0.1:6793） ----------

/// 启动客户端面板服务（阻塞任务）。失败仅记日志，不影响会话。
pub async fn serve_client(addr: std::net::SocketAddr) {
    let app = Router::new()
        .route("/", get(|| async { Html(CLIENT_HTML) }))
        .route("/api/info", get(info_json))
        .route(
            "/api/links",
            get(|| async { Json(json!({ "links": crate::stats::links_json() })) }),
        )
        .route("/ws", get(client_ws));
    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => {
            log::info!("client panel listening on http://{addr}/");
            if let Err(e) = axum::serve(listener, app).await {
                log::warn!("client panel on {addr} exited: {e}");
            }
        }
        Err(e) => log::warn!("client panel bind {addr} failed: {e}"),
    }
}

fn client_info_json() -> serde_json::Value {
    let i = crate::stats::info_snapshot();
    json!({
        "version": crate::version::VERSION,
        "mode": if i.mode.is_empty() { "idle" } else { i.mode.as_str() },
        "room": i.room,
        "signaling": i.signaling,
        "my_id": i.my_id,
        "device_name": i.device_name,
        "ext_addr": i.ext_addr,
        "lan_addrs": i.lan_addrs,
        "vnet_ip": i.vnet_ip,
        "tun": i.tun,
        "mtu": i.mtu,
        "encryption": i.encryption,
        "started_at": i.started_at,
        "reconnects": i.reconnects,
    })
}

async fn info_json() -> Json<serde_json::Value> {
    Json(client_info_json())
}

async fn client_ws(ws: WebSocketUpgrade) -> impl axum::response::IntoResponse {
    ws.on_upgrade(|mut socket: WebSocket| async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    let payload =
                        json!({"info": client_info_json(), "links": crate::stats::links_json()});
                    if socket.send(Message::Text(payload.to_string().into())).await.is_err() {
                        break;
                    }
                }
                msg = socket.recv() => {
                    if matches!(msg, None | Some(Err(_))) { break; }
                }
            }
        }
    })
}
