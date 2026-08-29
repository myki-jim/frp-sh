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
    /// 房主虚拟网卡 IP（--tun 时通告，访客可据此直连）
    #[serde(default)]
    pub tun_ip: Option<String>,
    /// 房主局域网打洞地址列表（同局域网时直连，端口与打洞 socket 一致）
    #[serde(default)]
    pub host_lan: Vec<SocketAddr>,
    /// 房主局域网子网 CIDR（--tun 时通告，访客据此加路由访问房主整个局域网）
    #[serde(default)]
    pub host_subnets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRoomResponse {
    pub room_id: String,
    pub host_addr: SocketAddr,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRoomRequest {
    pub addr: SocketAddr,
    /// 访客局域网打洞地址列表（房主反向打洞用）
    #[serde(default)]
    pub addr_lan: Vec<SocketAddr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRoomResponse {
    pub room_id: String,
    pub host_addr: SocketAddr,
}

/// 房主刷新自己的公网地址（重连时 NAT 映射可能已过期）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshRoomRequest {
    pub addr: SocketAddr,
    #[serde(default)]
    pub host_lan: Vec<SocketAddr>,
    #[serde(default)]
    pub host_subnets: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub room_id: String,
    pub host_addr: SocketAddr,
    pub guest_addr: Option<SocketAddr>,
    pub created_at: u64,
    pub expires_at: u64,
    /// 房主虚拟网卡 IP（--tun 时才有）
    #[serde(default)]
    pub tun_ip: Option<String>,
    /// 房主局域网打洞地址（同局域网直连）
    #[serde(default)]
    pub host_lan: Vec<SocketAddr>,
    /// 房主局域网子网 CIDR（--tun 时访客据此加路由）
    #[serde(default)]
    pub host_subnets: Vec<String>,
    /// 访客局域网打洞地址（房主反向打洞）
    #[serde(default)]
    pub guest_lan: Vec<SocketAddr>,
}
