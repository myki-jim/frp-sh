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
    /// 房主预留的访客虚拟 IP 池（`--guest-ips`）；访客未显式指定 IP 时按加入顺序分配，
    /// 同一设备（UUID）重连复用同一 IP
    #[serde(default)]
    pub guest_ips: Vec<String>,
    /// 房主分配的 TURN relay 地址（配置了 TURN 供应商时）
    #[serde(default)]
    pub turn_relay: Option<SocketAddr>,
    /// 房主程序版本（`major.minor.patch`，用于访客端兼容性提示）
    #[serde(default)]
    pub version: String,
    /// 房主设备显示名（面板显示；默认主机名）
    #[serde(default)]
    pub name: Option<String>,
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
    /// 访客设备 UUID（IP 池分配时用于稳定复用同一 IP）
    #[serde(default)]
    pub visitor_id: Option<String>,
    /// 设备显示名（默认主机名，`--name`/config `name` 覆盖；房内重名自动加后缀）
    #[serde(default)]
    pub name: Option<String>,
    /// 访客显式指定的虚拟 IP（--ip）
    #[serde(default)]
    pub requested_ip: Option<String>,
    /// 访客暴露的局域网子网 CIDR（`--expose-lan` 时通告，房主据此加路由访问访客局域网）
    #[serde(default)]
    pub guest_subnets: Vec<String>,
    /// 本端分配的 TURN relay 地址（配置了 TURN 供应商时通告，供对端经 TURN 中继建立数据面）
    #[serde(default)]
    pub turn_relay: Option<SocketAddr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinRoomResponse {
    pub room_id: String,
    pub host_addr: SocketAddr,
    /// 服务器从房主预留 IP 池中分配给该访客的虚拟 IP（未启用 IP 池时为 None）
    #[serde(default)]
    pub assigned_ip: Option<String>,
    /// 服务器去重后的设备显示名（重名自动加 -2/-3 后缀）
    #[serde(default)]
    pub name: Option<String>,
}

/// 房主刷新自己的公网地址（重连时 NAT 映射可能已过期）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshRoomRequest {
    pub addr: SocketAddr,
    #[serde(default)]
    pub host_lan: Vec<SocketAddr>,
    #[serde(default)]
    pub host_subnets: Vec<String>,
    /// 本端分配的 TURN relay 地址（配置了 TURN 供应商时）
    #[serde(default)]
    pub turn_relay: Option<SocketAddr>,
}

/// 房间内一个访客的完整信息（网格模式多访客；旧客户端忽略该字段）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuestInfo {
    /// 访客设备 UUID（匿名访客为地址字符串）
    pub uuid: String,
    /// 设备显示名（默认主机名；房内重名自动加后缀）
    #[serde(default)]
    pub name: Option<String>,
    /// 访客公网 UDP 地址
    pub addr: SocketAddr,
    /// 访客局域网打洞地址（同局域网直连）
    #[serde(default)]
    pub lan: Vec<SocketAddr>,
    /// 分配给该访客的虚拟 IP（房主 IP 池 / UUID 派生）
    #[serde(default)]
    pub vnet_ip: Option<String>,
    /// 访客暴露的局域网子网 CIDR（`--expose-lan`）
    #[serde(default)]
    pub subnets: Vec<String>,
    /// 访客分配的 TURN relay 地址（本端 TURN 客户端的映射地址）
    #[serde(default)]
    pub turn_relay: Option<SocketAddr>,
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
    /// 房主分配的 TURN relay 地址（配置了 TURN 供应商时）
    #[serde(default)]
    pub host_turn_relay: Option<SocketAddr>,
    /// 访客局域网打洞地址（房主反向打洞）
    #[serde(default)]
    pub guest_lan: Vec<SocketAddr>,
    /// 访客暴露的局域网子网 CIDR（`--expose-lan`）
    #[serde(default)]
    pub guest_subnets: Vec<String>,
    /// 访客分配的 TURN relay 地址（配置了 TURN 供应商时）
    #[serde(default)]
    pub guest_turn_relay: Option<SocketAddr>,
    /// 全部访客（网格模式多访客）
    #[serde(default)]
    pub guests: Vec<GuestInfo>,
    /// 房主程序版本（`major.minor.patch`）
    #[serde(default)]
    pub host_version: String,
    /// 房主设备显示名（默认主机名；旧服务器不返回时为 None）
    #[serde(default)]
    pub host_name: Option<String>,
    /// 信令服务器下发的内置 TURN 地址（`serve --turn` 且启用密码认证时才有；
    /// 客户端未配置 TURN 供应商时自动使用，凭据复用服务器密码）
    #[serde(default)]
    pub server_turn: Option<SocketAddr>,
}
