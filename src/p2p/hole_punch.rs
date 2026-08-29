//! UDP 打洞引擎：PUNCH / ACK 同时握手。
//!
//! 双方先通过信令服务器 UDP 探测学习自己的公网映射（与打洞同一 socket），
//! 再互相向对方的公网地址持续发送 `PUNCH <token>` 数据报：
//! - 收到 PUNCH → 立即回复 `ACK <token>` 并记录对端地址；
//! - 收到与自己 token 匹配的 ACK → 直连成功；
//! - 收到 FRS1 数据帧 → 对端已进入数据阶段，直连成功。
//!
//! 超时未打通 → 交由上层转入中继回退。

use crate::error::{FrpError, Result};
use crate::p2p::stream;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;

/// 打洞结果。
#[derive(Debug)]
pub enum PunchOutcome {
    /// 直连成功：`peer` 为观测到的对端地址；
    /// `first` 为打洞期间收到的首个数据帧（若有，交给数据流处理）。
    Direct {
        peer: SocketAddr,
        first: Option<(Vec<u8>, SocketAddr)>,
    },
    /// 超时未打通（应转入中继）。
    TimedOut,
}

pub struct PunchEngine {
    socket: UdpSocket,
}

impl PunchEngine {
    /// 绑定随机本地 UDP 端口。
    pub async fn bind() -> Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").await.map_err(FrpError::Io)?;
        Ok(Self { socket })
    }

    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.socket.local_addr().map_err(FrpError::Io)
    }

    /// 供信令客户端做公网地址探测（与打洞共用同一 socket/映射）。
    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }

    pub async fn send_punch(&self, target: SocketAddr, token: &str) -> Result<()> {
        self.socket
            .send_to(format!("PUNCH {token}").as_bytes(), target)
            .await
            .map_err(FrpError::Io)?;
        Ok(())
    }

    pub async fn send_ack(&self, target: SocketAddr, token: &str) -> Result<()> {
        self.socket
            .send_to(format!("ACK {token}").as_bytes(), target)
            .await
            .map_err(FrpError::Io)?;
        Ok(())
    }

    /// 接收一个数据报；超时返回 `Ok(None)`。
    ///
    /// Windows 上向无人监听的端口发包会收到 ICMP，下一次操作（recv 或 send）
    /// 返回 WSAECONNRESET(10054)——视为正常现象忽略，错误已被清除。
    pub async fn recv(&self, timeout: Duration) -> Result<Option<(Vec<u8>, SocketAddr)>> {
        let mut buf = [0u8; 2048];
        loop {
            match tokio::time::timeout(timeout, self.socket.recv_from(&mut buf)).await {
                Ok(Ok((len, src))) => return Ok(Some((buf[..len].to_vec(), src))),
                Ok(Err(e))
                    if e.kind() == std::io::ErrorKind::ConnectionReset
                        || e.kind() == std::io::ErrorKind::ConnectionRefused =>
                {
                    continue; // ICMP 毒化，忽略并继续
                }
                Ok(Err(e)) => return Err(FrpError::Io(e)),
                Err(_) => return Ok(None),
            }
        }
    }

    /// 发送 ACK，带重试（清除 Windows ICMP 毒化，确保送达）。
    pub async fn send_ack_retry(&self, target: SocketAddr, token: &str) {
        for _ in 0..4 {
            if self.send_ack(target, token).await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// 打洞成功后，将 socket 转为可靠数据流。
    /// `key`：可选 32 字节共享密钥（`--key` 启用后数据帧加密）。
    pub fn into_stream(
        self,
        peer: SocketAddr,
        first: Option<(Vec<u8>, SocketAddr)>,
        key: Option<[u8; 32]>,
    ) -> stream::UdpStream {
        stream::UdpStream::new(self.socket, peer, first, key)
    }
}

/// 打洞目标集合：对端通告地址 ± spread 个端口（轻量端口预测）。
/// 一些 NAT 对连续端口做连续映射，散布打洞可提高命中率；未命中的端口
/// 通常无人监听，产生的 ICMP 错误已被上层忽略，无副作用。
pub fn punch_targets(addr: SocketAddr, spread: u32) -> Vec<SocketAddr> {
    let mut targets = vec![addr];
    if spread == 0 {
        return targets;
    }
    let port = addr.port() as u32;
    for offset in 1..=spread {
        if let Some(p) = port.checked_add(offset) {
            if p <= u16::MAX as u32 {
                targets.push(SocketAddr::new(addr.ip(), p as u16));
            }
        }
        if let Some(p) = port.checked_sub(offset) {
            targets.push(SocketAddr::new(addr.ip(), p as u16));
        }
    }
    targets
}

/// 打洞目标集合（排除自身端口——引擎绑定 0.0.0.0 而目标为 127.0.0.1，
/// 需按端口而非完整地址匹配，否则相邻临时端口会造成"自我打洞"误判）。
pub fn punch_targets_excluding(
    addr: SocketAddr,
    spread: u32,
    exclude: Option<SocketAddr>,
) -> Vec<SocketAddr> {
    match exclude {
        Some(ex) if ex.port() != 0 => punch_targets(addr, spread)
            .into_iter()
            .filter(|t| t.port() != ex.port())
            .collect(),
        _ => punch_targets(addr, spread),
    }
}

/// 单轮打洞目标数上限（公网 + 多个局域网地址 × 端口散布），避免数据报风暴。
pub const MAX_PUNCH_TARGETS: usize = 24;

/// 打洞目标集合：对多个候选地址（公网 + 局域网）逐一做端口散布，去重并限流。
pub fn punch_targets_multi(
    addrs: &[SocketAddr],
    spread: u32,
    exclude: Option<SocketAddr>,
) -> Vec<SocketAddr> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for a in addrs {
        for t in punch_targets_excluding(*a, spread, exclude) {
            if seen.insert(t) {
                out.push(t);
            }
        }
        if out.len() >= MAX_PUNCH_TARGETS {
            break;
        }
    }
    out.truncate(MAX_PUNCH_TARGETS);
    out
}

/// 解析 `PUNCH <token>`。
pub fn parse_punch(buf: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(buf).ok()?;
    let mut it = text.split_whitespace();
    if it.next()? == "PUNCH" {
        it.next().map(str::to_string)
    } else {
        None
    }
}

/// 解析 `ACK <token>`。
pub fn parse_ack(buf: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(buf).ok()?;
    let mut it = text.split_whitespace();
    if it.next()? == "ACK" {
        it.next().map(str::to_string)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punch_parse() {
        assert_eq!(parse_punch(b"PUNCH abc123"), Some("abc123".to_string()));
        assert_eq!(parse_punch(b"PUNCH"), None);
        assert_eq!(parse_ack(b"ACK 0000"), Some("0000".to_string()));
        assert_eq!(parse_ack(b"PUNCH x"), None);
    }

    #[test]
    fn multi_targets_dedup_and_cap() {
        let a = "192.168.1.5:1234".parse().unwrap();
        let b = "192.168.1.5:1234".parse().unwrap(); // 重复地址
        let c = "192.168.1.6:1234".parse().unwrap();
        let t = punch_targets_multi(&[a, b, c], 1, None);
        // 3 个基底地址 × (1 + 2*1) = 9 个目标，重复地址去重后 6 个
        assert_eq!(t.len(), 6);
        let mut seen = std::collections::HashSet::new();
        for x in &t {
            assert!(seen.insert(*x), "dup target {x}");
        }
        // 排除端口生效
        let ex = "10.0.0.9:9999".parse().unwrap();
        let t2 = punch_targets_multi(&[a], 2, Some(ex));
        assert!(t2.iter().all(|x| x.port() != 9999));
        // 上限截断
        let many: Vec<SocketAddr> = (1..=20)
            .map(|i| format!("10.0.{i}.1:5000").parse().unwrap())
            .collect();
        let t3 = punch_targets_multi(&many, 8, None);
        assert!(t3.len() <= MAX_PUNCH_TARGETS);
    }
}
