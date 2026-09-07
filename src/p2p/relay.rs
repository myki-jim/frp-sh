//! 中继客户端传输：连接信令服务器的 TCP 中继端点并完成 HELLO 配对。

use crate::error::{FrpError, Result};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
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

/// 流量计数包装层：中继 TCP 无应用层心跳（RTT 不可测），但字节数照常统计，
/// 终端链路卡片即可显示速率与活跃状态。
pub(crate) struct CountingStream<S> {
    inner: S,
    stats: std::sync::Arc<crate::stats::StreamStats>,
}

impl<S: AsyncRead + Unpin> AsyncRead for CountingStream<S> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        std::pin::Pin::new(&mut self.inner)
            .poll_read(cx, buf)
            .map(|r| {
                if let Ok(()) = &r {
                    let n = buf.filled().len() - before;
                    if n > 0 {
                        self.stats.on_recv(n);
                    }
                }
                r
            })
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for CountingStream<S> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.inner)
            .poll_write(cx, buf)
            .map(|r| {
                if let Ok(n) = &r {
                    if *n > 0 {
                        self.stats.on_sent(*n);
                    }
                }
                r
            })
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// 对 TCP 连接启用短周期保活：空闲约 5s 开始探测，Linux/Windows 约 30s、
/// macOS 约 30s 内感知死对端（默认系统参数下为 2 小时以上）。
///
/// 中继路径没有应用层心跳，若对端断网/切换网络后保持空闲，裸 TCP 默认
/// 数小时都不会报错，导致隧道静默挂死、无法触发自动重连。此处用 socket2
/// 收紧 TCP keepalive，让断线快速暴露。
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
        sock.set_tcp_keepalive(&ka)?;
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = &std;
    }
    TcpStream::from_std(std)
}

/// 完成 `HELLO2 <room_id> <ROLE> <uuid-or-dash> <owner-token-or-dash>` 握手。
///
/// - `token`：服务器密码，用于握手前建立认证加密流，不在 HELLO 中发送。
/// - `encrypted`：是否启用中继流加密（由服务器 `/version` 的 auth 标志决定）
/// - `peer_uuid`：网格模式对端 UUID（房主为中继特定访客单独建连时携带；
///   旧协议传 `None` 走单槽位配对）
///
/// 服务器立即回复 `WAIT`（等待对端）或 `OK`（已配对）；
/// 配对完成后服务器负责双向拷贝，本函数返回的流可直接读写。
#[allow(clippy::too_many_arguments)]
pub async fn connect(
    relay_addr: SocketAddr,
    room_id: &str,
    role: RelayRole,
    token: Option<&str>,
    encrypted: bool,
    peer_uuid: Option<&str>,
    owner_token: Option<String>,
) -> Result<(RelayStream, std::sync::Arc<crate::stats::StreamStats>)> {
    let tcp = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(relay_addr))
        .await
        .map_err(|_| FrpError::Relay("relay connect timeout".into()))??;
    tcp.set_nodelay(true)?;
    let mut tcp = enable_keepalive(tcp)?;
    if token.is_some_and(|t| t.starts_with("r3_")) {
        if !crate::utils::validate_room_id(room_id) {
            return Err(FrpError::Relay("invalid room ID".into()));
        }
        tcp.write_all(format!("R3 {room_id}\n").as_bytes()).await?;
        tcp.flush().await?;
    }
    let mut stream: RelayStream = if encrypted || token.is_some_and(|t| t.starts_with("r3_")) {
        let key = crate::p2p::enc::key_from_password(
            token.ok_or_else(|| FrpError::Relay("relay password required".into()))?,
        );
        Box::new(crate::p2p::enc::EncStream::new(tcp, &key))
    } else {
        Box::new(tcp)
    };
    let owner = owner_token.as_deref().unwrap_or("-");
    let uuid = peer_uuid.unwrap_or("-");
    if [room_id, uuid, owner]
        .iter()
        .any(|v| v.is_empty() || v.chars().any(char::is_whitespace))
    {
        return Err(FrpError::Relay("invalid relay identifier".into()));
    }
    let hello = format!("HELLO2 {room_id} {} {uuid} {owner}\r\n", role.as_str());
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        stream.write_all(hello.as_bytes()).await?;
        stream.flush().await?;
        let mut line = Vec::new();
        loop {
            let b = stream.read_u8().await?;
            if b == b'\n' {
                break;
            }
            if line.len() >= 128 {
                return Err(std::io::Error::other("relay response too long"));
            }
            line.push(b);
        }
        let line = String::from_utf8_lossy(&line);
        if line.trim() != "WAIT" && line.trim() != "OK" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "relay rejected authentication or room",
            ));
        }
        Ok::<(), std::io::Error>(())
    })
    .await
    .map_err(|_| FrpError::Relay("relay authentication timeout".into()))?;
    result?;
    let stats = crate::stats::StreamStats::new(crate::stats::KIND_RELAY);
    Ok((
        Box::new(CountingStream {
            inner: stream,
            stats: stats.clone(),
        }),
        stats,
    ))
}
