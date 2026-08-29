//! 信令层：REST 客户端 + axum 服务端 + UDP 公网探测 + TCP 中继。

pub mod client;
pub mod server;

pub use client::SignalingClient;

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRoomRequest {
    pub prefix: String,
    /// 有效时长（秒）
    pub ttl: u64,
    /// 发起者自测到的公网 UDP 地址（IP:Port）
    pub addr: SocketAddr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRoomResponse {
    pub room_id: String,
    pub host_addr: SocketAddr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRoomRequest {
    pub addr: SocketAddr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRoomResponse {
    pub room_id: String,
    pub host_addr: SocketAddr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub room_id: String,
    pub host_addr: SocketAddr,
    pub guest_addr: Option<SocketAddr>,
    pub created_at: u64,
    pub expires_at: u64,
}
