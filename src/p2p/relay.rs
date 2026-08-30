//! 中继客户端传输：连接信令服务器的 TCP 中继端点并完成 HELLO 配对。

use crate::error::{FrpError, Result};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug, Clone, Copy)]
pub enum RelayRole {
    Host,
    Guest,
}

impl RelayRole {
    fn as_str(&self) -> &'static str {
        match self {
            RelayRole::Host => "HOST",
            RelayRole::Guest => "GUEST",
        }
    }
}

/// 中继连接：服务器设置了密码时为加密流，否则为裸 TCP。
pub trait AsyncReadWrite: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> AsyncReadWrite for T {}

pub type RelayStream = Box<dyn AsyncReadWrite>;

/// 对 TCP 连接启用短周期保活：空闲约 5s 开始探测，约 15s 内感知死对端。
///
/// 中继路径没有应用层心跳，若对端断网/切换网络后保持空闲，裸 TCP 默认
/// 数小时都不会报错，导致隧道静默挂死、无法触发自动重连。此处用 socket2
/// 收紧 TCP keepalive（Linux 额外限制 3 次探测），让断线快速暴露。
pub fn enable_keepalive(stream: TcpStream) -> std::io::Result<TcpStream> {
    let std = stream.into_std()?;
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        use socket2::{SockRef, TcpKeepalive};
        let sock = SockRef::from(&std);
        sock.set_keepalive(true)?;
        let ka = TcpKeepalive::new()
            .with_time(Duration::from_secs(5))
            .with_interval(Duration::from_secs(3));
        #[cfg(target_os = "linux")]
        let ka = ka.with_retries(3);
        sock.set_tcp_keepalive(&ka)?;
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = &std;
    }
    TcpStream::from_std(std)
}

/// 连接信令服务器中继端点，完成 `HELLO <room_id> <ROLE> [token]` 握手。
///
/// - `token`：服务器密码（服务器设置密码时必须提供，作为认证与加密密钥）
/// - `encrypted`：是否启用中继流加密（由服务器 `/version` 的 auth 标志决定）
///
/// 服务器立即回复 `WAIT`（等待对端）或 `OK`（已配对）；
/// 配对完成后服务器负责双向拷贝，本函数返回的流可直接读写。
pub async fn connect(
    relay_addr: SocketAddr,
    room_id: &str,
    role: RelayRole,
    token: Option<&str>,
    encrypted: bool,
) -> Result<RelayStream> {
    let stream = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(relay_addr))
        .await
        .map_err(|_| FrpError::Relay(format!("connect {relay_addr} timed out")))?
        .map_err(FrpError::Io)?;
    // 断网/切换网络后快速感知死连接，触发上层自动重连
    let stream = enable_keepalive(stream).map_err(FrpError::Io)?;
    stream.set_nodelay(true).map_err(FrpError::Io)?;

    let hello = match token {
        Some(t) if !t.is_empty() => format!("HELLO {room_id} {} {t}\r\n", role.as_str()),
        _ => format!("HELLO {room_id} {}\r\n", role.as_str()),
    };
    let (mut rd, mut wr) = stream.into_split();
    wr.write_all(hello.as_bytes()).await.map_err(FrpError::Io)?;

    // 逐字节读取服务器回复行，避免缓冲残留吞掉对端随后发来的数据（如 CNEW）
    let mut line = Vec::with_capacity(16);
    let mut byte = [0u8; 1];
    loop {
        let n = tokio::time::timeout(Duration::from_secs(10), rd.read(&mut byte))
            .await
            .map_err(|_| FrpError::Relay("no response from relay server".into()))?
            .map_err(FrpError::Io)?;
        if n == 0 {
            return Err(FrpError::Relay("relay connection closed".into()));
        }
        if byte[0] == b'\n' {
            break;
        }
        if line.len() < 128 {
            line.push(byte[0]);
        }
    }
    let resp = String::from_utf8_lossy(&line);
    let trimmed = resp.trim();
    if let Some(err) = trimmed.strip_prefix("ERROR ") {
        return Err(FrpError::Relay(format!("relay rejected: {err}")));
    }
    if trimmed != "WAIT" && trimmed != "OK" {
        return Err(FrpError::Relay(format!(
            "unexpected relay response: {trimmed}"
        )));
    }

    let stream = rd
        .reunite(wr)
        .map_err(|_| FrpError::Relay("stream reunite failed".into()))?;
    if encrypted {
        let key = match token {
            Some(t) => crate::p2p::enc::key_from_password(t),
            None => {
                return Err(FrpError::Relay(
                    "server requires encrypted relay but no password configured".into(),
                ))
            }
        };
        Ok(Box::new(crate::p2p::enc::EncStream::new(stream, &key)))
    } else {
        Ok(Box::new(stream))
    }
}
