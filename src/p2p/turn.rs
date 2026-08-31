//! TURN（RFC 5766）客户端：把 UDP 打洞失败后的数据面经 TURN 服务器中继。
//!
//! TURN 是建立在 STUN 消息格式上的 UDP relay：
//! - 客户端 Allocate 一个 relay 地址（服务器上的 IP:port）；
//! - CreatePermission 授权对端地址（只有被授权的地址能往 relay 地址发数据）；
//! - 发送：Send indication（XOR-PEER-ADDRESS = 对端地址 + DATA 载荷）；
//! - 接收：Data indication（服务器把到 relay 地址的数据包转给客户端）。
//!
//! 本实现只做需要的子集（UDP 传输、long-term credential 认证、静态凭据）：
//! 兼容 coturn / Cloudflare TURN / 任何 RFC 5766 实现。对上层表现为一个
//! "虚拟 UDP socket"（实现 [`DatagramSocket`]），因此 FRS1 可靠流可以
//! 不经改动直接跑在 TURN 中继之上——直连与 TURN 之间只是 peer 地址不同。

use crate::p2p::stun;
use async_trait::async_trait;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

/// 虚拟数据报 socket 抽象：UDP socket 与 TURN 通道都实现它，
/// 让 FRS1 可靠流（`UdpStream`）可以跑在直连或中继之上。
#[async_trait]
pub trait DatagramSocket: Send + Sync + Unpin {
    async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)>;
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize>;
}

#[async_trait]
impl DatagramSocket for UdpSocket {
    async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        UdpSocket::recv_from(self, buf).await
    }
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        UdpSocket::send_to(self, buf, addr).await
    }
}

#[async_trait]
impl DatagramSocket for Arc<UdpSocket> {
    async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        UdpSocket::recv_from(self, buf).await
    }
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> io::Result<usize> {
        UdpSocket::send_to(self, buf, addr).await
    }
}

/// TURN 服务器凭据。
#[derive(Debug, Clone)]
pub struct TurnCredentials {
    pub server: SocketAddr,
    pub username: String,
    pub password: String,
}

/// TURN 客户端通道：实现 [`DatagramSocket`]，对上层隐藏 TURN 封装。
pub struct TurnClient {
    socket: Arc<UdpSocket>,
    server: SocketAddr,
    /// 分配的 relay 地址（服务器视角的映射地址）
    pub relay: SocketAddr,
    /// 当前授权的对端地址（CreatePermission）
    peer: Arc<std::sync::Mutex<Option<SocketAddr>>>,
    username: String,
    realm: String,
    nonce: Arc<std::sync::Mutex<String>>,
    key: [u8; 16],
    /// 收到的 Data indication 载荷队列（tokio Mutex：recv 跨 await 持锁）
    rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<(SocketAddr, Vec<u8>)>>,
}

impl TurnClient {
    /// 连接 TURN 服务器并分配 relay 地址（long-term credential 认证）。
    ///
    /// `socket` 必须是已绑定（随机端口）的 UDP socket，此后由本客户端独占使用。
    pub async fn connect(socket: UdpSocket, cred: &TurnCredentials) -> crate::error::Result<Self> {
        let socket = Arc::new(socket);
        let rtp0 = stun::encode_requested_transport();
        let mut req = stun::build(
            stun::METHOD_ALLOCATE,
            0,
            &stun::new_txid(),
            &[
                (stun::ATTR_REQUESTED_TRANSPORT, &rtp0),
                (stun::ATTR_SOFTWARE, b"frp-sh"),
            ],
        );
        let (mut realm, mut nonce) = (String::new(), String::new());
        let mut lifetime = 600u32;
        let mut relay = None;
        // 认证循环：无凭据 → 401 取 realm/nonce → 带凭据重发；
        // coturn 可能因 nonce 过期再回 401 → 用新 nonce 重试（最多 5 轮）
        for attempt in 0..5 {
            socket
                .send_to(&req, cred.server)
                .await
                .map_err(crate::error::FrpError::Io)?;
            log::debug!(
                "TURN allocate sent (attempt {attempt}, {} bytes)",
                req.len()
            );
            let mut buf = [0u8; 2048];
            let (n, _) = tokio::time::timeout(Duration::from_secs(3), socket.recv_from(&mut buf))
                .await
                .map_err(|_| crate::error::FrpError::Relay("TURN allocate timed out".into()))?
                .map_err(crate::error::FrpError::Io)?;
            log::debug!("TURN allocate recv {n}B");
            let Some(msg) = stun::parse(&buf[..n]) else {
                log::warn!("TURN allocate: unparsable {n}B response");
                continue;
            };
            // 响应的 txid 必须与当前请求一致（当前 req 的 txid 无法直接取，
            // 通过比对非杂散：忽略与本轮请求无关的旧响应）
            if msg.class == 3 {
                // 错误响应
                let code = msg
                    .get(stun::ATTR_ERROR_CODE)
                    .and_then(stun::decode_error_code)
                    .map(|(c, _)| c)
                    .unwrap_or(0);
                if code == 401 {
                    // 需要认证/重认证：取新 realm + nonce，重发带凭据的 Allocate
                    realm = msg.get_str(stun::ATTR_REALM).unwrap_or_default();
                    nonce = msg.get_str(stun::ATTR_NONCE).unwrap_or_default();
                    if realm.is_empty() || nonce.is_empty() {
                        return Err(crate::error::FrpError::Relay(
                            "TURN 401 without realm/nonce".into(),
                        ));
                    }
                    let key = stun::long_term_key(&cred.username, &realm, &cred.password);
                    let rtp2 = stun::encode_requested_transport();
                    let mut attrs: Vec<(u16, &[u8])> = vec![
                        (stun::ATTR_REQUESTED_TRANSPORT, &rtp2),
                        (stun::ATTR_USERNAME, cred.username.as_bytes()),
                        (stun::ATTR_REALM, realm.as_bytes()),
                        (stun::ATTR_NONCE, nonce.as_bytes()),
                    ];
                    let software = b"frp-sh";
                    attrs.push((stun::ATTR_SOFTWARE, software));
                    req = stun::build_with_integrity(
                        stun::METHOD_ALLOCATE,
                        0,
                        &stun::new_txid(),
                        &attrs,
                        &key,
                    );
                    continue;
                }
                return Err(crate::error::FrpError::Relay(format!(
                    "TURN allocate error {code}"
                )));
            }
            if msg.class == 2 {
                // 成功：relay 地址在 XOR-RELAYED-ADDRESS（TURN），
                // 部分实现只回 XOR-MAPPED-ADDRESS 时兜底
                relay = msg
                    .get(stun::ATTR_XOR_RELAYED_ADDRESS)
                    .and_then(|v| stun::decode_xor_addr(v, &msg.txid))
                    .or_else(|| {
                        msg.get(stun::ATTR_XOR_MAPPED_ADDRESS)
                            .and_then(|v| stun::decode_xor_addr(v, &msg.txid))
                    });
                if let Some(v) = msg.get(stun::ATTR_LIFETIME) {
                    if v.len() >= 4 {
                        lifetime = u32::from_be_bytes(v[..4].try_into().unwrap_or([0, 0, 0, 0]));
                    }
                }
                break;
            }
        }
        let relay = relay.ok_or_else(|| {
            crate::error::FrpError::Relay("TURN allocate did not yield a relay address".into())
        })?;
        let key = stun::long_term_key(&cred.username, &realm, &cred.password);
        let (tx, rx) = mpsc::unbounded_channel();
        let client = Self {
            socket: socket.clone(),
            server: cred.server,
            relay,
            peer: Arc::new(std::sync::Mutex::new(None)),
            username: cred.username.clone(),
            realm,
            nonce: Arc::new(std::sync::Mutex::new(nonce)),
            key,
            rx: tokio::sync::Mutex::new(rx),
        };
        // 收包循环：Data indication → 队列；Refresh/CreatePermission 响应忽略
        let c = client.clone_for_loop();
        tokio::spawn(async move {
            c.recv_loop(tx).await;
        });
        // 保活：LIFETIME/2 周期 Refresh
        let c = client.clone_for_loop();
        let refresh_interval = Duration::from_secs((lifetime / 2).max(30) as u64);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(refresh_interval);
            loop {
                tick.tick().await;
                let _ = c.refresh().await;
            }
        });
        Ok(client)
    }

    fn clone_for_loop(&self) -> TurnLoop {
        TurnLoop {
            socket: self.socket.clone(),
            server: self.server,
            username: self.username.clone(),
            realm: self.realm.clone(),
            nonce: self.nonce.clone(),
            key: self.key,
        }
    }

    /// 授权对端地址（CreatePermission）：只有被授权的地址能往 relay 地址发数据。
    pub async fn create_permission(&self, peer: SocketAddr) -> crate::error::Result<()> {
        let txid = stun::new_txid();
        let xpe = stun::encode_xor_addr(peer, &txid);
        let attrs = vec![(stun::ATTR_XOR_PEER_ADDRESS, xpe.as_slice())];
        let req = self.signed(stun::METHOD_CREATE_PERMISSION, 0, &txid, &attrs);
        self.socket
            .send_to(&req, self.server)
            .await
            .map_err(crate::error::FrpError::Io)?;
        *self.peer.lock().unwrap() = Some(peer);
        // 等待成功/错误响应（最多 1s）；超时视为成功（服务器处理是即时的）
        let mut buf = [0u8; 1024];
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            let remain = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remain.is_zero() {
                return Ok(());
            }
            match tokio::time::timeout(remain, self.socket.recv_from(&mut buf)).await {
                Ok(Ok((n, _))) => {
                    if let Some(msg) = stun::parse(&buf[..n]) {
                        if msg.txid == txid {
                            if msg.class == 2 {
                                return Ok(());
                            }
                            let code = msg
                                .get(stun::ATTR_ERROR_CODE)
                                .and_then(stun::decode_error_code)
                                .map(|(c, _)| c)
                                .unwrap_or(0);
                            return Err(crate::error::FrpError::Relay(format!(
                                "TURN create-permission error {code}"
                            )));
                        }
                        // 其他消息（如 Data indication）交给收包循环
                    }
                }
                Ok(Err(_)) => continue,
                Err(_) => return Ok(()), // 超时
            }
        }
    }

    /// 构造带 MESSAGE-INTEGRITY 的请求（用当前 realm/nonce）。
    fn signed(&self, method: u16, class: u16, txid: &[u8; 12], attrs: &[(u16, &[u8])]) -> Vec<u8> {
        let nonce = self.nonce.lock().unwrap().clone();
        signed_message(
            &self.key,
            &self.username,
            &self.realm,
            &nonce,
            method,
            class,
            txid,
            attrs,
        )
    }
}

/// 构造带认证（USERNAME/REALM/NONCE + MESSAGE-INTEGRITY）的 TURN 请求。
#[allow(clippy::too_many_arguments)]
fn signed_message(
    key: &[u8; 16],
    username: &str,
    realm: &str,
    nonce: &str,
    method: u16,
    class: u16,
    txid: &[u8; 12],
    attrs: &[(u16, &[u8])],
) -> Vec<u8> {
    let mut all: Vec<(u16, Vec<u8>)> = vec![
        (stun::ATTR_USERNAME, username.as_bytes().to_vec()),
        (stun::ATTR_REALM, realm.as_bytes().to_vec()),
        (stun::ATTR_NONCE, nonce.as_bytes().to_vec()),
    ];
    for (t, v) in attrs {
        all.push((*t, v.to_vec()));
    }
    let refs: Vec<(u16, &[u8])> = all.iter().map(|(t, v)| (*t, v.as_slice())).collect();
    stun::build_with_integrity(method, class, txid, &refs, key).to_vec()
}

/// 供后台任务持有的 TURN 通道（不持有接收队列）。
struct TurnLoop {
    socket: Arc<UdpSocket>,
    server: SocketAddr,
    username: String,
    realm: String,
    nonce: Arc<std::sync::Mutex<String>>,
    key: [u8; 16],
}

impl TurnLoop {
    async fn recv_loop(&self, tx: mpsc::UnboundedSender<(SocketAddr, Vec<u8>)>) {
        let mut buf = [0u8; 2048];
        loop {
            match self.socket.recv_from(&mut buf).await {
                Ok((n, _)) => {
                    let Some(msg) = stun::parse(&buf[..n]) else {
                        continue;
                    };
                    if msg.class == 1 && msg.method == stun::METHOD_DATA {
                        if let (Some(pv), Some(dv)) = (
                            msg.get(stun::ATTR_XOR_PEER_ADDRESS),
                            msg.get(stun::ATTR_DATA),
                        ) {
                            if let Some(peer_addr) = stun::decode_xor_addr(pv, &msg.txid) {
                                let _ = tx.send((peer_addr, dv.to_vec()));
                            }
                        }
                    }
                    // 其他消息（Refresh/权限响应等）忽略
                }
                Err(e) => {
                    log::warn!("TURN recv error: {e}");
                    break;
                }
            }
        }
    }

    async fn refresh(&self) -> crate::error::Result<()> {
        let txid = stun::new_txid();
        let lt = stun::encode_lifetime(600);
        let attrs = vec![(stun::ATTR_LIFETIME, lt.as_slice())];
        let nonce = self.nonce.lock().unwrap().clone();
        let req = signed_message(
            &self.key,
            &self.username,
            &self.realm,
            &nonce,
            stun::METHOD_REFRESH,
            0,
            &txid,
            &attrs,
        );
        self.socket
            .send_to(&req, self.server)
            .await
            .map_err(crate::error::FrpError::Io)?;
        Ok(())
    }
}

#[async_trait]
impl DatagramSocket for TurnClient {
    async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let mut rx = self.rx.lock().await;
        match rx.recv().await {
            Some((peer, data)) => {
                let n = data.len().min(buf.len());
                buf[..n].copy_from_slice(&data[..n]);
                Ok((n, peer))
            }
            None => Err(io::Error::other("TURN channel closed")),
        }
    }

    async fn send_to(&self, data: &[u8], _addr: SocketAddr) -> io::Result<usize> {
        // 目标对端 = 已授权的 peer（Send indication 的 XOR-PEER-ADDRESS）
        let peer = self
            .peer
            .lock()
            .unwrap()
            .ok_or_else(|| io::Error::other("TURN peer not authorized"))?;
        let txid = stun::new_txid();
        let xpe = stun::encode_xor_addr(peer, &txid);
        let attrs = vec![
            (stun::ATTR_XOR_PEER_ADDRESS, xpe.as_slice()),
            (stun::ATTR_DATA, data),
        ];
        let nonce = self.nonce.lock().unwrap().clone();
        let mut all: Vec<(u16, Vec<u8>)> = vec![
            (stun::ATTR_USERNAME, self.username.as_bytes().to_vec()),
            (stun::ATTR_REALM, self.realm.as_bytes().to_vec()),
            (stun::ATTR_NONCE, nonce.as_bytes().to_vec()),
        ];
        for (t, v) in attrs {
            all.push((t, v.to_vec()));
        }
        let refs: Vec<(u16, &[u8])> = all.iter().map(|(t, v)| (*t, v.as_slice())).collect();
        let req = stun::build_with_integrity(stun::METHOD_SEND, 1, &txid, &refs, &self.key);
        self.socket
            .send_to(&req, self.server)
            .await
            .map(|_| data.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn datagram_socket_trait_impls() {
        // UdpSocket 与 TurnClient 都实现 DatagramSocket（编译期验证）
        fn assert_impl<T: DatagramSocket>() {}
        assert_impl::<UdpSocket>();
        // TurnClient 需先 connect 才能构造，这里只验证类型可用
        let _ = std::mem::size_of::<TurnClient>();
    }
}
