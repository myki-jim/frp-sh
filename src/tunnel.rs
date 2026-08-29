//! 隧道层：将打洞/中继数据通道桥接到本地 TCP，支持顺序多连接复用。
//!
//! 帧协议（两个方向对称）：
//! - 访客在每次本地连接建立时先写 4 字节 `CNEW` 标记；
//! - 之后数据以 `[u32 BE len][payload]` 帧传输；
//! - `len == 0` 为结束帧：本地连接关闭时发送；收到对端结束帧时回送（若尚未发送）并结束本连接。
//!
//! 双向泵由两个常驻任务驱动（各持有读写流的一侧，经有界 channel 交换数据），
//! 避免 `select!` 丢弃未选中分支的半读数据；结束帧对称确认避免关闭竞态死锁。

use crate::error::{FrpError, Result};
use std::io;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};

/// 访客新连接标记
const CNEW: [u8; 4] = *b"CNEW";
/// 单帧上限（防内存滥用）
const MAX_FRAME: usize = 1 << 20;

// ---------- 帧读写 ----------

async fn write_frame(w: &mut (impl AsyncWrite + Unpin), data: &[u8]) -> io::Result<()> {
    let len = data.len() as u32;
    w.write_all(&len.to_be_bytes()).await?;
    if !data.is_empty() {
        w.write_all(data).await?;
    }
    Ok(())
}

/// 从累计缓冲提取一个完整帧。
/// `Ok(Some(data))` 数据帧；`Ok(Some(&[]))` 结束帧；`Ok(None)` 缓冲不足；`Err` 帧超限。
fn take_frame(acc: &mut Vec<u8>) -> io::Result<Option<Vec<u8>>> {
    if acc.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_be_bytes(acc[..4].try_into().unwrap()) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("tunnel frame too large: {len}"),
        ));
    }
    if acc.len() < 4 + len {
        return Ok(None);
    }
    let data = if len == 0 {
        Vec::new()
    } else {
        acc[4..4 + len].to_vec()
    };
    acc.drain(..4 + len);
    Ok(Some(data))
}

/// 带重试地拨号本地服务。
async fn dial_service(service: SocketAddr) -> Result<TcpStream> {
    let mut last_err = None;
    for attempt in 1..=3 {
        match tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(service)).await {
            Ok(Ok(s)) => return Ok(s),
            Ok(Err(e)) => {
                log::warn!("connect local service {service} failed (attempt {attempt}): {e}");
                last_err = Some(e);
            }
            Err(_) => {
                last_err = Some(io::Error::new(io::ErrorKind::TimedOut, "connect timed out"));
                log::warn!("connect local service {service} timed out (attempt {attempt})");
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(FrpError::Protocol(format!(
        "cannot reach local service {service}: {}",
        last_err
            .map(|e| e.to_string())
            .unwrap_or_else(|| "unknown error".into())
    )))
}

// ---------- 双向泵 ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PumpDone {
    /// 本连接正常结束（可接受下一个连接）
    Conn,
    /// 会话结束（对端关闭）
    Session,
}

struct PumpHandle<T> {
    done_rx: mpsc::Receiver<PumpDone>,
    transport_rx: oneshot::Receiver<T>,
}

impl<T> PumpHandle<T> {
    async fn finish(mut self) -> io::Result<(PumpDone, T)> {
        let done = self.done_rx.recv().await.unwrap_or(PumpDone::Session);
        let transport = self
            .transport_rx
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "pump task failed"))?;
        Ok((done, transport))
    }
}

/// 启动双向泵：两个常驻任务分别驱动 `transport` 与 `local`，经有界 channel 交换数据。
///
/// 安全要点：对端任务用**单次原始 read + 持久累计缓冲**解析帧（pending 的 read
/// 不会消耗字节，select 丢弃时无损失），且使用 `biased` 顺序避免就绪读被丢弃。
fn spawn_pump<T>(transport: T, local: TcpStream) -> PumpHandle<T>
where
    T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (to_remote, mut from_local) = mpsc::channel::<Vec<u8>>(32); // 本地数据 → 对端
    let (to_local, mut from_remote) = mpsc::channel::<Vec<u8>>(32); // 对端数据 → 本地
    let (done_tx, done_rx) = mpsc::channel::<PumpDone>(1);
    let (transport_tx, transport_rx) = oneshot::channel::<T>();

    // 对端任务：唯一持有 transport（读 + 写帧）
    tokio::spawn(async move {
        let mut transport = transport;
        let mut acc: Vec<u8> = Vec::new();
        let mut buf = [0u8; 16384];
        let result = loop {
            tokio::select! {
                biased;
                n = transport.read(&mut buf) => {
                    let n = match n {
                        Ok(0) | Err(_) => break PumpDone::Session,
                        Ok(n) => n,
                    };
                    acc.extend_from_slice(&buf[..n]);
                    let mut outcome = None;
                    loop {
                        match take_frame(&mut acc) {
                            Ok(Some(f)) if f.is_empty() => {
                                // 对端结束帧：回送并结束本连接
                                if write_frame(&mut transport, &[]).await.is_err() {
                                    outcome = Some(PumpDone::Session);
                                } else {
                                    outcome = Some(PumpDone::Conn);
                                }
                                break;
                            }
                            Ok(Some(f)) => {
                                if to_local.send(f).await.is_err() {
                                    // 本地已关闭：发送结束帧并等待对端
                                    if write_frame(&mut transport, &[]).await.is_err() {
                                        outcome = Some(PumpDone::Session);
                                    } else {
                                        outcome = Some(wait_end(&mut transport, &mut acc, &mut buf).await);
                                    }
                                    break;
                                }
                            }
                            Ok(None) => break, // 残帧不足，继续读
                            Err(_) => {
                                outcome = Some(PumpDone::Session);
                                break;
                            }
                        }
                    }
                    if let Some(o) = outcome {
                        break o;
                    }
                }
                d = from_local.recv() => {
                    match d {
                        None => {
                            // 本地关闭：发送结束帧，等待对端结束帧
                            if write_frame(&mut transport, &[]).await.is_err() {
                                break PumpDone::Session;
                            }
                            break wait_end(&mut transport, &mut acc, &mut buf).await;
                        }
                        Some(bytes) => {
                            if write_frame(&mut transport, &bytes).await.is_err() {
                                break PumpDone::Session;
                            }
                        }
                    }
                }
            }
        };
        let _ = transport_tx.send(transport);
        let _ = done_tx.send(result).await;
    });

    // 本地任务：持有 local TCP
    tokio::spawn(async move {
        let mut local = local;
        let mut buf = [0u8; 16384];
        loop {
            tokio::select! {
                biased;
                r = local.read(&mut buf) => {
                    match r {
                        Ok(0) | Err(_) => break, // 本地结束：通道关闭即通知对端任务
                        Ok(n) => {
                            if to_remote.send(buf[..n].to_vec()).await.is_err() {
                                break;
                            }
                        }
                    }
                }
                d = from_remote.recv() => {
                    match d {
                        Some(bytes) => {
                            if local.write_all(&bytes).await.is_err() {
                                break;
                            }
                        }
                        None => break, // 对端任务结束
                    }
                }
            }
        }
    });

    PumpHandle {
        done_rx,
        transport_rx,
    }
}

/// 已发送结束帧后：丢弃残余数据帧，等待对端结束帧。
async fn wait_end(
    transport: &mut (impl AsyncRead + Unpin),
    acc: &mut Vec<u8>,
    buf: &mut [u8],
) -> PumpDone {
    loop {
        match take_frame(acc) {
            Ok(Some(f)) if f.is_empty() => return PumpDone::Conn,
            Ok(Some(_)) => continue,
            Ok(None) => {}
            Err(_) => return PumpDone::Session,
        }
        match transport.read(buf).await {
            Ok(0) | Err(_) => return PumpDone::Session,
            Ok(n) => acc.extend_from_slice(&buf[..n]),
        }
    }
}

// ---------- 访客侧 ----------

/// 访客侧：监听本地端口，循环接受连接（顺序多连接复用）。
pub async fn guest_forward(
    mut transport: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
    listen: SocketAddr,
    max_conns: u64,
) -> Result<()> {
    let listener = TcpListener::bind(listen).await.map_err(FrpError::Io)?;
    println!("listening on {listen}, waiting for local connections (Ctrl-C to end) ...");
    let mut conns: u64 = 0;
    loop {
        let (tcp, peer) = listener.accept().await.map_err(FrpError::Io)?;
        conns += 1;
        println!("connection {conns} from {peer}, opening tunnel ...");
        tcp.set_nodelay(true).map_err(FrpError::Io)?;
        transport.write_all(&CNEW).await.map_err(FrpError::Io)?;
        let pump = spawn_pump(transport, tcp);
        let (done, t) = pump.finish().await.map_err(FrpError::Io)?;
        transport = t;
        match done {
            PumpDone::Conn => println!("connection {conns} closed"),
            PumpDone::Session => {
                println!("session ended by peer");
                break;
            }
        }
        if max_conns > 0 && conns >= max_conns {
            println!("max connections ({max_conns}) reached, ending session");
            break;
        }
    }
    Ok(())
}

// ---------- 房主侧 ----------

/// 房主侧：等待访客连接（`CNEW`），拨号本地服务并双向转发。
pub async fn host_forward(
    mut transport: impl AsyncRead + AsyncWrite + Unpin + Send + 'static,
    service: SocketAddr,
    max_conns: u64,
) -> Result<()> {
    println!("waiting for the guest to connect (Ctrl-C to end) ...");
    let mut conns: u64 = 0;
    loop {
        // 等待 CNEW（竞态残余帧按帧丢弃）
        loop {
            let mut magic = [0u8; 4];
            match transport.read_exact(&mut magic).await {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    return Err(FrpError::Protocol("session ended by peer".into()));
                }
                Err(e) => return Err(FrpError::Io(e)),
            }
            if magic == CNEW {
                break;
            }
            // 访客关闭竞态期间仍在途的残余数据帧
            let len = u32::from_be_bytes(magic) as usize;
            if len == 0 || len > MAX_FRAME {
                return Err(FrpError::Protocol("bad tunnel frame".into()));
            }
            let mut skip = vec![0u8; len];
            transport
                .read_exact(&mut skip)
                .await
                .map_err(FrpError::Io)?;
            log::debug!("discarded {len} stray tunnel bytes");
        }

        conns += 1;
        println!("guest connection {conns}, dialing local service {service} ...");
        let tcp = dial_service(service).await?;
        tcp.set_nodelay(true).map_err(FrpError::Io)?;
        let pump = spawn_pump(transport, tcp);
        let (done, t) = pump.finish().await.map_err(FrpError::Io)?;
        transport = t;
        match done {
            PumpDone::Conn => println!("connection {conns} closed"),
            PumpDone::Session => {
                println!("session ended by peer");
                break;
            }
        }
        if max_conns > 0 && conns >= max_conns {
            println!("max connections ({max_conns}) reached, ending session");
            break;
        }
    }
    Ok(())
}
