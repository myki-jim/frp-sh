//! 信令 REST 客户端。

use super::{
    CreateRoomRequest, CreateRoomResponse, JoinRoomRequest, JoinRoomResponse, RefreshRoomRequest,
    RoomInfo,
};
use crate::error::{FrpError, Result};
use reqwest::StatusCode;
use std::net::SocketAddr;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SignalingClient {
    base_url: String,
    http: reqwest::Client,
}

impl SignalingClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("build reqwest client"),
        }
    }

    /// 注册房间。`addr` 是本机通过 UDP 探测得到的公网地址；
    /// `tun_ip` 为虚拟网卡模式时通告的虚拟 IP（可选）；
    /// `host_lan` 为本机局域网打洞地址（同局域网直连）；
    /// `host_subnets` 为本机局域网子网 CIDR（--tun 时访客据此访问房主局域网）；
    /// `guest_ips` 为房主预留的访客虚拟 IP 池（`--guest-ips`）。
    #[allow(clippy::too_many_arguments)]
    pub async fn create_room(
        &self,
        prefix: &str,
        ttl: u64,
        addr: SocketAddr,
        tun_ip: Option<String>,
        host_lan: Vec<SocketAddr>,
        host_subnets: Vec<String>,
        guest_ips: Vec<String>,
    ) -> Result<CreateRoomResponse> {
        let resp = self
            .http
            .post(format!("{}/room/create", self.base_url))
            .json(&CreateRoomRequest {
                prefix: prefix.to_string(),
                ttl,
                addr,
                tun_ip,
                host_lan,
                host_subnets,
                guest_ips,
            })
            .send()
            .await
            .map_err(|e| FrpError::Signaling(format!("create room: {e}")))?;
        if !resp.status().is_success() {
            return Err(FrpError::Signaling(format!(
                "create room: HTTP {}",
                resp.status()
            )));
        }
        resp.json()
            .await
            .map_err(|e| FrpError::Signaling(format!("bad create response: {e}")))
    }

    /// 访客登记加入房间，返回房主公网地址与分配的虚拟 IP。
    ///
    /// - `addr_lan`：本机局域网打洞地址
    /// - `visitor_id`：设备 UUID（房主启用 IP 池时用于稳定复用同一虚拟 IP）
    /// - `requested_ip`：显式指定的虚拟 IP（`--ip`），无则 None
    pub async fn join_room(
        &self,
        room_id: &str,
        addr: SocketAddr,
        addr_lan: Vec<SocketAddr>,
        visitor_id: Option<String>,
        requested_ip: Option<String>,
    ) -> Result<JoinRoomResponse> {
        let resp = self
            .http
            .post(format!("{}/room/{}/join", self.base_url, room_id))
            .json(&JoinRoomRequest {
                addr,
                addr_lan,
                visitor_id,
                requested_ip,
            })
            .send()
            .await
            .map_err(|e| FrpError::Signaling(format!("join room: {e}")))?;
        match resp.status() {
            StatusCode::NOT_FOUND => Err(FrpError::RoomNotFound(room_id.to_string())),
            s if !s.is_success() => Err(FrpError::Signaling(format!("join room: HTTP {s}"))),
            _ => resp
                .json()
                .await
                .map_err(|e| FrpError::Signaling(format!("bad join response: {e}"))),
        }
    }

    /// 查询房间信息。
    pub async fn get_room(&self, room_id: &str) -> Result<RoomInfo> {
        let resp = self
            .http
            .get(format!("{}/room/{}", self.base_url, room_id))
            .send()
            .await
            .map_err(|e| FrpError::Signaling(format!("get room: {e}")))?;
        match resp.status() {
            StatusCode::NOT_FOUND => Err(FrpError::RoomNotFound(room_id.to_string())),
            s if !s.is_success() => Err(FrpError::Signaling(format!("get room: HTTP {s}"))),
            _ => resp
                .json()
                .await
                .map_err(|e| FrpError::Signaling(format!("bad room response: {e}"))),
        }
    }

    /// 关闭房间（尽力而为）。
    pub async fn delete_room(&self, room_id: &str) -> Result<()> {
        let resp = self
            .http
            .delete(format!("{}/room/{}", self.base_url, room_id))
            .send()
            .await
            .map_err(|e| FrpError::Signaling(format!("delete room: {e}")))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(FrpError::Signaling(format!(
                "delete room: HTTP {}",
                resp.status()
            )))
        }
    }

    /// 房主刷新自己的公网地址与局域网信息（重连时 NAT 映射可能已过期）。
    pub async fn refresh_room(
        &self,
        room_id: &str,
        addr: SocketAddr,
        host_lan: Vec<SocketAddr>,
        host_subnets: Vec<String>,
    ) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/room/{}/refresh", self.base_url, room_id))
            .json(&RefreshRoomRequest {
                addr,
                host_lan,
                host_subnets,
            })
            .send()
            .await
            .map_err(|e| FrpError::Signaling(format!("refresh room: {e}")))?;
        match resp.status() {
            StatusCode::NOT_FOUND => Err(FrpError::RoomNotFound(room_id.to_string())),
            s if !s.is_success() => Err(FrpError::Signaling(format!("refresh room: HTTP {s}"))),
            _ => Ok(()),
        }
    }

    /// 通过服务器 UDP 探测端口学习本机公网地址（STUN 简化版）。
    ///
    /// 注意：必须使用与后续打洞**相同的 socket**（同一本地端口），
    /// 这样 NAT 映射即为我们通告给对端的地址。
    pub async fn learn_public_addr(
        &self,
        udp: &tokio::net::UdpSocket,
        server_udp: SocketAddr,
        token: &str,
    ) -> Result<SocketAddr> {
        udp.send_to(format!("ECHO {token}").as_bytes(), server_udp)
            .await
            .map_err(FrpError::Io)?;
        let mut buf = [0u8; 256];
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            let remain = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remain.is_zero() {
                return Err(FrpError::Signaling("UDP echo timed out".into()));
            }
            let (len, _src) = tokio::time::timeout(remain, udp.recv_from(&mut buf))
                .await
                .map_err(|_| FrpError::Signaling("UDP echo timed out".into()))?
                .map_err(FrpError::Io)?;
            let text = String::from_utf8_lossy(&buf[..len]);
            // 服务器回复格式：ADDR <token> <ip>:<port>
            if let Some(rest) = text.strip_prefix("ADDR ") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() >= 2 && parts[0] == token {
                    return parts[1]
                        .parse()
                        .map_err(|_| FrpError::Signaling(format!("bad addr in echo: {text}")));
                }
            }
        }
    }
}
