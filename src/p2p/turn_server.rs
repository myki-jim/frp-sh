//! 内置 TURN 服务端（RFC 5766 UDP 子集）：`serve --turn` 提供 TURN 中继。
//!
//! 与自建信令/中继同进程（单文件哲学延续），认证复用 `--password`：
//! - 设置密码：long-term credential（用户名固定 `frp-sh`，密钥
//!   `MD5(frp-sh:realm:password)`），客户端用同一密码即可认证；
//! - 未设置密码：不要求认证（内网/可信场景）。
//!
//! 实现子集：Allocate / CreatePermission / Send indication / Data indication /
//! Refresh / STUN Binding。relay 端口随机分配，permission 限制只有被授权的
//! 对端地址能向 relay 地址发包（防滥用）。

use crate::p2p::stun;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

type HmacSha1 = Hmac<Sha1>;

/// TURN 服务端（RFC 5766 UDP 子集）。
pub struct TurnServer {
    listen: Arc<UdpSocket>,
    allocs: Arc<Mutex<HashMap<SocketAddr, Allocation>>>,
    /// 认证配置（None = 不要求认证）
    auth: Option<Auth>,
    /// relay 地址使用的公网 IP（None 时用监听地址推导）
    external_ip: Option<IpAddr>,
    /// 监听地址（推导 relay IP 用）
    local: SocketAddr,
}

struct Auth {
    realm: String,
    nonce: String,
    key: [u8; 16],
}

struct Allocation {
    /// relay socket（绑定随机端口，收 peer 数据）
    relay_socket: Arc<UdpSocket>,
    /// 授权对端地址（CreatePermission）
    permissions: HashSet<SocketAddr>,
    /// 客户端地址
    client: SocketAddr,
    /// 过期时间（Refresh 续期）
    expires: Instant,
}
const LIFETIME: u32 = 600;
const USERNAME: &str = "frp-sh";

impl TurnServer {
    /// 绑定监听端口并启动服务（认证密钥由 `password` 派生）。
    pub async fn start(
        addr: SocketAddr,
        password: Option<&str>,
        external_ip: Option<IpAddr>,
    ) -> crate::error::Result<Self> {
        let listen = Arc::new(
            UdpSocket::bind(addr)
                .await
                .map_err(crate::error::FrpError::Io)?,
        );
        let local = listen.local_addr().map_err(crate::error::FrpError::Io)?;
        let auth = password.map(|pw| {
            let realm = "frp.sh".to_string();
            let key = stun::long_term_key(USERNAME, &realm, pw);
            Auth {
                realm,
                nonce: stun::hex_token(8),
                key,
            }
        });
        let srv = Self {
            listen,
            allocs: Arc::new(Mutex::new(HashMap::new())),
            auth,
            external_ip,
            local,
        };
        // 清理过期 allocation 的任务
        let a = srv.allocs.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(60)).await;
                let mut map = a.lock().await;
                map.retain(|_, x| x.expires > Instant::now());
            }
        });
        Ok(srv)
    }

    /// 监听地址（供客户端连接）。
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// 主循环：收包 → 校验认证 → 按方法分发（消耗自身，供 spawn）。
    pub async fn run(self) -> crate::error::Result<()> {
        let mut buf = [0u8; 4096];
        loop {
            let (n, src) = match self.listen.recv_from(&mut buf).await {
                Ok(x) => x,
                Err(e) => {
                    log::warn!("TURN listen error: {e}");
                    continue;
                }
            };
            let packet = &buf[..n];
            let Some(msg) = stun::parse(packet) else {
                continue;
            };
            // 认证：Allocate/CreatePermission/Refresh 需认证（设置密码时）
            if let Some(auth) = &self.auth {
                if msg.method != stun::METHOD_BINDING
                    && (!verify_integrity(packet, &auth.key) || !self.valid_credentials(&msg))
                {
                    self.challenge(src).await;
                    continue;
                }
            }
            match msg.method {
                stun::METHOD_ALLOCATE => self.handle_allocate(&msg, src).await,
                stun::METHOD_CREATE_PERMISSION => self.handle_permission(&msg, src).await,
                stun::METHOD_SEND => self.handle_send(&msg, src).await,
                stun::METHOD_REFRESH => self.handle_refresh(&msg, src).await,
                stun::METHOD_BINDING => self.handle_binding(&msg, src).await,
                _ => {}
            }
        }
    }

    /// 校验请求的 username/realm/nonce 与服务器一致。
    fn valid_credentials(&self, msg: &stun::Message) -> bool {
        let Some(auth) = &self.auth else {
            return true;
        };
        msg.get_str(stun::ATTR_USERNAME).as_deref() == Some(USERNAME)
            && msg.get_str(stun::ATTR_REALM).as_deref() == Some(auth.realm.as_str())
            && msg.get_str(stun::ATTR_NONCE).as_deref() == Some(auth.nonce.as_str())
    }

    async fn handle_allocate(&self, _msg: &stun::Message, src: SocketAddr) {
        // 分配 relay socket（随机端口）
        let Ok(relay_socket) = UdpSocket::bind("0.0.0.0:0").await else {
            return;
        };
        let relay_port = match relay_socket.local_addr() {
            Ok(a) => a.port(),
            Err(_) => return,
        };
        let relay_ip = self
            .external_ip
            .unwrap_or_else(|| relay_socket_ip(self.local.ip()));
        let relay = SocketAddr::new(relay_ip, relay_port);
        let relay_socket = Arc::new(relay_socket);
        let client_key = src;
        let alloc = Allocation {
            relay_socket: relay_socket.clone(),
            permissions: HashSet::new(),
            client: src,
            expires: Instant::now() + Duration::from_secs(LIFETIME as u64),
        };
        self.allocs.lock().await.insert(client_key, alloc);
        // relay 收包任务：peer 数据 → Data indication → 客户端
        let allocs = self.allocs.clone();
        let ck = client_key;
        tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                let (n, peer) = match relay_socket.recv_from(&mut buf).await {
                    Ok(x) => x,
                    Err(_) => break,
                };
                let map = allocs.lock().await;
                let Some(a) = map.get(&ck) else { break };
                if !a.permissions.contains(&peer) {
                    continue; // 未授权
                }
                let client = a.client;
                drop(map);
                let txid = stun::new_txid();
                let xpe = stun::encode_xor_addr(peer, &txid);
                let req = stun::build(
                    stun::METHOD_DATA,
                    stun::CLASS_INDICATION,
                    &txid,
                    &[
                        (stun::ATTR_XOR_PEER_ADDRESS, &xpe),
                        (stun::ATTR_DATA, &buf[..n]),
                    ],
                );
                let _ = relay_socket.send_to(&req, client).await;
            }
        });
        // 200 响应
        let txid2 = stun::new_txid();
        let xrel = stun::encode_xor_addr(relay, &txid2);
        let lt = stun::encode_lifetime(LIFETIME);
        let resp = stun::build(
            stun::METHOD_ALLOCATE,
            stun::CLASS_SUCCESS,
            &txid2,
            &[
                (stun::ATTR_XOR_RELAYED_ADDRESS, &xrel),
                (stun::ATTR_LIFETIME, &lt),
                (stun::ATTR_SOFTWARE, b"frp-sh"),
            ],
        );
        let _ = self.listen.send_to(&resp, src).await;
        log::info!("TURN allocation created for {src} -> relay {relay}");
    }

    async fn handle_permission(&self, msg: &stun::Message, src: SocketAddr) {
        let Some(pv) = msg.get(stun::ATTR_XOR_PEER_ADDRESS) else {
            return;
        };
        let Some(peer) = stun::decode_xor_addr(pv, &msg.txid) else {
            return;
        };
        let mut map = self.allocs.lock().await;
        let Some(a) = map.get_mut(&src) else {
            return;
        };
        if a.expires <= Instant::now() {
            return;
        }
        a.permissions.insert(peer);
        drop(map);
        let txid = stun::new_txid();
        let resp = stun::build(
            stun::METHOD_CREATE_PERMISSION,
            stun::CLASS_SUCCESS,
            &txid,
            &[],
        );
        let _ = self.listen.send_to(&resp, src).await;
    }

    async fn handle_send(&self, msg: &stun::Message, src: SocketAddr) {
        let (Some(pv), Some(dv)) = (
            msg.get(stun::ATTR_XOR_PEER_ADDRESS),
            msg.get(stun::ATTR_DATA),
        ) else {
            return;
        };
        let Some(peer) = stun::decode_xor_addr(pv, &msg.txid) else {
            return;
        };
        let map = self.allocs.lock().await;
        let Some(a) = map.get(&src) else {
            return;
        };
        if a.expires <= Instant::now() {
            return;
        }
        if !a.permissions.contains(&peer) {
            return; // 未授权对端
        }
        let sock = a.relay_socket.clone();
        drop(map);
        let _ = sock.send_to(dv, peer).await;
    }

    async fn handle_refresh(&self, _msg: &stun::Message, src: SocketAddr) {
        let mut map = self.allocs.lock().await;
        let Some(a) = map.get_mut(&src) else {
            return;
        };
        a.expires = Instant::now() + Duration::from_secs(LIFETIME as u64);
        drop(map);
        let txid = stun::new_txid();
        let lt = stun::encode_lifetime(LIFETIME);
        let resp = stun::build(
            stun::METHOD_REFRESH,
            stun::CLASS_SUCCESS,
            &txid,
            &[(stun::ATTR_LIFETIME, &lt)],
        );
        let _ = self.listen.send_to(&resp, src).await;
    }

    async fn handle_binding(&self, _msg: &stun::Message, src: SocketAddr) {
        let txid = stun::new_txid();
        let xma = stun::encode_xor_addr(src, &txid);
        let resp = stun::build(
            stun::METHOD_BINDING,
            stun::CLASS_SUCCESS,
            &txid,
            &[(stun::ATTR_XOR_MAPPED_ADDRESS, &xma)],
        );
        let _ = self.listen.send_to(&resp, src).await;
    }

    /// 回 401 挑战（带 realm + nonce）。
    async fn challenge(&self, src: SocketAddr) {
        let Some(auth) = &self.auth else {
            return;
        };
        let txid = stun::new_txid();
        let attrs: Vec<(u16, Vec<u8>)> = vec![
            (
                stun::ATTR_ERROR_CODE,
                vec![
                    0, 0, 4, 1, b'U', b'n', b'a', b'u', b't', b'h', b'o', b'r', b'i', b'z', b'e',
                    b'd',
                ],
            ),
            (stun::ATTR_NONCE, auth.nonce.as_bytes().to_vec()),
            (stun::ATTR_REALM, auth.realm.as_bytes().to_vec()),
        ];
        let refs: Vec<(u16, &[u8])> = attrs.iter().map(|(t, v)| (*t, v.as_slice())).collect();
        let resp = stun::build(stun::METHOD_ALLOCATE, stun::CLASS_ERROR, &txid, &refs);
        let _ = self.listen.send_to(&resp, src).await;
    }
}

/// 校验原始数据包的 MESSAGE-INTEGRITY（与客户端生成规则一致：
/// 输入 = 头（length 字段 = attrs + 24，到 MI value 末尾）+ 前置 attributes，不含 MI）。
fn verify_integrity(packet: &[u8], key: &[u8]) -> bool {
    let Some(mi_off) = mi_offset(packet) else {
        return false;
    };
    if mi_off + 24 > packet.len() {
        return false;
    }
    let expected = &packet[mi_off + 4..mi_off + 24];
    let mut b = packet.to_vec();
    // 兼容 FINGERPRINT：length 字段按"到 MI value 末尾"重写（= mi_off + 4）
    let new_len = (mi_off + 4) as u16;
    b[2..4].copy_from_slice(&new_len.to_be_bytes());
    let mut mac = match HmacSha1::new_from_slice(key) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(&b[..mi_off]);
    mac.finalize().into_bytes().as_slice() == expected
}

/// 定位 MESSAGE-INTEGRITY attribute 的偏移。
fn mi_offset(packet: &[u8]) -> Option<usize> {
    if packet.len() < 24 {
        return None;
    }
    let n = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    let mut off = 20;
    while off + 4 <= 20 + n && off + 4 <= packet.len() {
        let t = u16::from_be_bytes([packet[off], packet[off + 1]]);
        let l = u16::from_be_bytes([packet[off + 2], packet[off + 3]]) as usize;
        if t == stun::ATTR_MESSAGE_INTEGRITY {
            return Some(off);
        }
        off += 4 + l + (4 - l % 4) % 4;
    }
    None
}

fn relay_socket_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(v) if v.is_unspecified() => crate::utils::lan_socket_addrs(0)
            .first()
            .map(|a| a.ip())
            .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mi_offset_finds_integrity() {
        // 构造一个带 MI 的请求（用客户端函数生成）
        let key = [7u8; 16];
        let txid = stun::new_txid();
        let attrs = [
            (stun::ATTR_USERNAME, b"frp-sh".as_slice()),
            (stun::ATTR_REALM, b"frp.sh".as_slice()),
            (stun::ATTR_NONCE, b"abcd1234".as_slice()),
        ];
        let msg = stun::build_with_integrity(stun::METHOD_ALLOCATE, 0, &txid, &attrs, &key);
        assert!(mi_offset(&msg).is_some());
        assert!(verify_integrity(&msg, &key));
        assert!(!verify_integrity(&msg, &[8u8; 16]));
    }
}
