//! 信令 REST 客户端。

use super::{
    CreateRoomRequest, CreateRoomResponse, JoinRoomRequest, JoinRoomResponse, RefreshRoomRequest,
    RoomInfo,
};
use crate::error::{FrpError, Result};
use reqwest::StatusCode;
use std::net::SocketAddr;
use std::time::Duration;

/// 请求认证头（与服务器 `--password` 对应）。
const AUTH_HEADER: &str = "X-Frp-Sh-Token";

#[derive(Debug, Clone)]
pub struct SignalingClient {
    base_url: String,
    http: reqwest::Client,
    password: Option<String>,
}

impl SignalingClient {
    /// 不带密码的客户端。
    pub fn new(base_url: &str) -> Self {
        Self::new_with_password(base_url, None)
    }

    /// 带服务器密码的客户端：所有请求携带 `X-Frp-Sh-Token`。
    pub fn new_with_password(base_url: &str, password: Option<&str>) -> Self {
        let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(10));
        if let Some(pw) = password {
            let mut headers = reqwest::header::HeaderMap::new();
            if let Ok(v) = reqwest::header::HeaderValue::from_str(pw) {
                headers.insert(AUTH_HEADER, v);
            }
            builder = builder.default_headers(headers);
        }
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: builder.build().expect("build reqwest client"),
            password: password.map(str::to_string),
        }
    }

    /// 服务器是否要求密码（客户端配置了密码即随请求携带；服务器未设密码时无害）。
    pub fn has_password(&self) -> bool {
        self.password.is_some()
    }

    /// 注册房间。`addr` 是本机通过 UDP 探测得到的公网地址；
    /// `tun_ip` 为虚拟网卡模式时通告的虚拟 IP（可选）；
    /// `host_lan` 为本机局域网打洞地址（同局域网直连）；
    /// `host_subnets` 为本机局域网子网 CIDR（--tun 时访客据此访问房主局域网）；
    /// `guest_ips` 为房主预留的访客虚拟 IP 池（`--guest-ips`）；
    /// `version` 为本机程序版本（访客端兼容性提示用）。
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
        version: String,
        turn_relay: Option<SocketAddr>,
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
                version,
                turn_relay,
            })
            .send()
            .await
            .map_err(|e| FrpError::Signaling(format!("create room: {e}")))?;
        if resp.status() == StatusCode::UNAUTHORIZED {
            return Err(Self::auth_err());
        }
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

    /// 服务器需要密码但未提供/错误时的友好错误。
    fn auth_err() -> FrpError {
        FrpError::Signaling(
            "server requires a password (HTTP 401): configure 'password' in config (run `frp-sh config`)".into(),
        )
    }

    /// 访客登记加入房间，返回房主公网地址与分配的虚拟 IP。
    ///
    /// - `addr_lan`：本机局域网打洞地址
    /// - `visitor_id`：设备 UUID（房主启用 IP 池时用于稳定复用同一虚拟 IP）
    /// - `requested_ip`：显式指定的虚拟 IP（`--ip`），无则 None
    /// - `guest_subnets`：访客暴露的局域网子网（`--expose-lan` 时通告）
    #[allow(clippy::too_many_arguments)]
    pub async fn join_room(
        &self,
        room_id: &str,
        addr: SocketAddr,
        addr_lan: Vec<SocketAddr>,
        visitor_id: Option<String>,
        requested_ip: Option<String>,
        guest_subnets: Vec<String>,
        turn_relay: Option<SocketAddr>,
    ) -> Result<JoinRoomResponse> {
        let resp = self
            .http
            .post(format!("{}/room/{}/join", self.base_url, room_id))
            .json(&JoinRoomRequest {
                addr,
                addr_lan,
                visitor_id,
                requested_ip,
                guest_subnets,
                turn_relay,
            })
            .send()
            .await
            .map_err(|e| FrpError::Signaling(format!("join room: {e}")))?;
        match resp.status() {
            StatusCode::UNAUTHORIZED => Err(Self::auth_err()),
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
            StatusCode::UNAUTHORIZED => Err(Self::auth_err()),
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
        turn_relay: Option<SocketAddr>,
    ) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/room/{}/refresh", self.base_url, room_id))
            .json(&RefreshRoomRequest {
                addr,
                host_lan,
                host_subnets,
                turn_relay,
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

    /// 查询服务器版本、线协议版本与是否启用密码认证。
    ///
    /// 旧服务器没有 `/version` 端点（404）时返回 `None`（按协议 1、无认证兼容）。
    pub async fn get_version(&self) -> Result<Option<(String, u32, bool)>> {
        let resp = self
            .http
            .get(format!("{}/version", self.base_url))
            .send()
            .await
            .map_err(|e| FrpError::Signaling(format!("get version: {e}")))?;
        match resp.status() {
            StatusCode::NOT_FOUND => Ok(None),
            s if !s.is_success() => Err(FrpError::Signaling(format!("get version: HTTP {s}"))),
            _ => {
                let v: serde_json::Value = resp
                    .json()
                    .await
                    .map_err(|e| FrpError::Signaling(format!("bad version response: {e}")))?;
                let ver = v.get("version").and_then(|x| x.as_str()).unwrap_or("");
                let proto = v.get("protocol").and_then(|x| x.as_u64()).unwrap_or(0);
                let auth = v.get("auth").and_then(|x| x.as_bool()).unwrap_or(false);
                Ok(Some((ver.to_string(), proto as u32, auth)))
            }
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

    /// 学习公网地址：配置了 STUN 时优先走 STUN（免费、不依赖自建 UDP 探测），
    /// 失败回退到自建服务器的 UDP echo。两种方式都使用打洞 socket 本身。
    pub async fn learn_public_addr_auto(
        &self,
        udp: &tokio::net::UdpSocket,
        server_udp: SocketAddr,
        token: &str,
        stun: Option<SocketAddr>,
    ) -> Result<SocketAddr> {
        if let Some(stun_addr) = stun {
            match crate::p2p::stun::binding_probe(udp, stun_addr).await {
                Ok(addr) => {
                    log::info!("public address via STUN {stun_addr}: {addr}");
                    return Ok(addr);
                }
                Err(e) => {
                    log::warn!("STUN binding failed ({e}), falling back to server UDP echo");
                }
            }
        }
        self.learn_public_addr(udp, server_udp, token).await
    }
}
