//! 房间本地状态（当前连接的角色、对端地址、中继与否）。

use std::net::SocketAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// 房主：打洞监听方 + 本地服务拨号方
    Host,
    /// 访客：打洞发起方 + 本地监听方
    Guest,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Host => "HOST",
            Role::Guest => "GUEST",
        }
    }
}

#[derive(Debug)]
pub struct RoomState {
    pub room_id: String,
    pub role: Role,
    pub peer_addr: Option<SocketAddr>,
    pub relay_mode: bool,
    /// 房主：本地服务地址；访客：本地监听地址
    pub local: SocketAddr,
}

impl RoomState {
    pub fn new(room_id: String, role: Role, local: SocketAddr) -> Self {
        Self {
            room_id,
            role,
            peer_addr: None,
            relay_mode: false,
            local,
        }
    }
}
