//! 可靠 UDP 流（FRS1 帧协议）。
//!
//! 在打洞后的 UDP socket 之上提供字节流语义：
//! - 15 字节定长帧头 + 滑动窗口（go-back-N）+ 累积 ACK + 超时重传；
//! - 实现 [`AsyncRead`] / [`AsyncWrite`]，可直接用于 `copy_bidirectional`；
//! - 空闲时发送 keepalive ACK 帧，维持 NAT 映射；
//! - FIN 关闭握手 + 确认超时检测。
//!
//! 帧格式（头 15 字节 + payload）：
//! ```text
//! +------+------+------+------+------+------+------+------+------+------+------+------+------+------+------+
//! | magic "FRS1" (4B) | flags (1B) |  seq u32 BE   |  ack u32 BE   |  len u16 BE   | payload (len B)      |
//! +------+------+------+------+------+------+------+------+------+------+------+------+------+------+------+
//! flags: 0x01 = DATA, 0x02 = FIN；无标志位即纯 ACK 帧。
//! ```

use bytes::BytesMut;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::UdpSocket;
use tokio::sync::Notify;

pub const MAGIC: [u8; 4] = *b"FRS1";

const FLAG_DATA: u8 = 0x01;
const FLAG_FIN: u8 = 0x02;
/// 数据帧最大长度（含 Poly1305 标签时明文相应缩减）
const MAX_PAYLOAD: usize = 1200;
/// 加密时单帧明文上限（1200 - 16 字节标签）
const MAX_PLAINTEXT: usize = MAX_PAYLOAD - 16;
const WINDOW: usize = 32;
const RETRANSMIT_MS: u64 = 150;
const KEEPALIVE_MS: u64 = 1000;
/// 心跳存活超时：超过该时长未收到对端任何帧（数据/ACK/FIN）即判定连接已死，
/// 主动报错让上层触发自动重连——对端切换网络/掉线导致路径失效时快速恢复，
/// 而不是无限重传挂死。可用环境变量 `FRPSH_LIVENESS_MS` 覆盖（毫秒），
/// 默认 3 秒（keepalive 每 1s 一次，连续 3 次未收到即视为断线）。
const LIVENESS_TIMEOUT_MS: u64 = 3_000;
const OUT_CAP: usize = 256 * 1024;
const RX_CAP: usize = 1024 * 1024;
const CLOSE_TIMEOUT_MS: u64 = 5000;

/// 由帧序号派生 AEAD nonce（每帧唯一）。
fn nonce_for(seq: u32) -> Nonce {
    let mut n = [0u8; 12];
    n[..4].copy_from_slice(&seq.to_be_bytes());
    *Nonce::from_slice(&n)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameKind {
    Data,
    Fin,
    Ack,
}

fn encode_frame(kind: FrameKind, seq: u32, ack: u32, payload: &[u8]) -> BytesMut {
    let mut out = BytesMut::with_capacity(15 + payload.len());
    out.extend_from_slice(&MAGIC);
    let flags = match kind {
        FrameKind::Data => FLAG_DATA,
        FrameKind::Fin => FLAG_FIN,
        FrameKind::Ack => 0,
    };
    out.extend_from_slice(&[flags]);
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(&ack.to_be_bytes());
    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn parse_frame(buf: &[u8]) -> Option<(FrameKind, u32, u32, &[u8])> {
    if buf.len() < 15 || buf[..4] != MAGIC[..] {
        return None;
    }
    let flags = buf[4];
    let seq = u32::from_be_bytes(buf[5..9].try_into().ok()?);
    let ack = u32::from_be_bytes(buf[9..13].try_into().ok()?);
    let len = u16::from_be_bytes(buf[13..15].try_into().ok()?) as usize;
    if buf.len() != 15 + len {
        return None;
    }
    let kind = if flags & FLAG_FIN != 0 {
        FrameKind::Fin
    } else if flags & FLAG_DATA != 0 {
        FrameKind::Data
    } else {
        FrameKind::Ack
    };
    Some((kind, seq, ack, &buf[15..]))
}

/// 判断一个数据报是否为 FRS1 数据帧（打洞阶段用于识别直连成功）。
pub fn is_data_frame(buf: &[u8]) -> bool {
    buf.len() >= 4 && buf[..4] == MAGIC[..]
}

struct Shared {
    peer: SocketAddr,
    // 接收侧
    rx_buf: VecDeque<u8>,
    rx_closed: bool,
    next_expected: u32,
    // 发送侧
    out_queue: VecDeque<Vec<u8>>,
    out_queue_len: usize,
    unacked: VecDeque<(u32, Vec<u8>)>, // (seq, 已编码帧，重传时原样发送)
    next_seq: u32,
    fin_sent: bool,
    fin_seq: u32,
    fin_acked: bool,
    write_closed: bool,
    last_rx: Instant,
    /// 存活超时（默认 10s；`FRPSH_LIVENESS_MS` 可覆盖，测试可缩短）
    liveness: Duration,
    /// 是否收到过对端的任何帧。mesh 模式下房主先于访客流存在而建链（由打洞信号触发），
    /// 建链后到访客首帧到达前的静默是**正常现象**；此期间存活阈值放宽为 liveness×5，
    /// 收到首帧后恢复严格检测。访客单独流由收到的帧触发创建（had_rx=true），静默即真断。
    had_rx: bool,
    err: Option<String>,
    read_waker: Option<Waker>,
    write_waker: Option<Waker>,
    /// 数据帧负载加密（`--key` 启用；ACK/FIN 帧不加密）
    cipher: Option<ChaCha20Poly1305>,
    /// 测试用：数据帧首发丢包率（重传不受影响，保证最终可达）
    #[cfg(test)]
    loss_rate: f64,
}

fn wake(w: &mut Option<Waker>) {
    if let Some(w) = w.take() {
        w.wake();
    }
}

fn fail(shared: &Arc<Mutex<Shared>>, msg: String) {
    let mut g = shared.lock().unwrap();
    if g.err.is_none() {
        log::debug!("stream failed: {msg}");
        g.err = Some(msg);
        wake(&mut g.read_waker);
        wake(&mut g.write_waker);
    }
}

pub struct UdpStream {
    shared: Arc<Mutex<Shared>>,
    out_notify: Arc<Notify>,
    idle_close: Arc<Notify>,
}

/// 构造一个对端的流状态（UdpStream 与 UdpMesh 共用）。
fn make_shared(peer: SocketAddr, key: Option<[u8; 32]>, had_rx: bool) -> Shared {
    let liveness = std::env::var("FRPSH_LIVENESS_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_millis(LIVENESS_TIMEOUT_MS));
    let cipher = key.map(|k| ChaCha20Poly1305::new(Key::from_slice(&k)));
    Shared {
        peer,
        rx_buf: VecDeque::new(),
        rx_closed: false,
        next_expected: 1,
        out_queue: VecDeque::new(),
        out_queue_len: 0,
        unacked: VecDeque::new(),
        next_seq: 1,
        fin_sent: false,
        fin_seq: 0,
        fin_acked: false,
        write_closed: false,
        last_rx: Instant::now(),
        liveness,
        had_rx,
        err: None,
        read_waker: None,
        write_waker: None,
        cipher,
        #[cfg(test)]
        loss_rate: 0.0,
    }
}

impl UdpStream {
    /// `key`：32 字节共享密钥；为 `Some` 时所有数据帧负载用 ChaCha20-Poly1305 加密。
    ///
    /// `socket` 为数据报传输：`UdpSocket`（直连）或 `TurnClient`（TURN 中继）均可。
    pub fn new<S: crate::p2p::turn::DatagramSocket + 'static>(
        socket: S,
        peer: SocketAddr,
        first: Option<(Vec<u8>, SocketAddr)>,
        key: Option<[u8; 32]>,
    ) -> Self {
        // 由收到的帧触发生成的流（first 传入打洞/首帧）视作对端已证实存活；
        // 无首帧的流（mesh 建链）需等待对端首帧才进入严格存活检测
        let shared = Arc::new(Mutex::new(make_shared(peer, key, first.is_some())));
        let out_notify = Arc::new(Notify::new());
        let idle_close = Arc::new(Notify::new());
        // JoinHandle 直接丢弃：任务以 detached 方式运行，由 idle_close 通知退出
        tokio::spawn(run(
            socket,
            shared.clone(),
            out_notify.clone(),
            idle_close.clone(),
            first,
        ));
        Self {
            shared,
            out_notify,
            idle_close,
        }
    }

    pub fn peer(&self) -> SocketAddr {
        self.shared.lock().unwrap().peer
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap()
    }

    /// 测试用：设置数据帧首发丢包率（0.0 ~ 1.0）。
    #[cfg(test)]
    pub fn set_loss_rate(&self, rate: f64) {
        self.shared.lock().unwrap().loss_rate = rate;
    }

    /// 测试用：缩短存活超时，快速验证断线检测。
    #[cfg(test)]
    pub fn set_liveness(&self, timeout: Duration) {
        self.shared.lock().unwrap().liveness = timeout;
    }
}

impl Drop for UdpStream {
    fn drop(&mut self) {
        // 句柄被丢弃 → 通知后台任务关闭 socket
        self.idle_close.notify_one();
    }
}

/// 网格对端注册表：来源地址 → (流状态, 关闭通知)。
type PeerRegistry = Arc<std::sync::Mutex<HashMap<SocketAddr, (Arc<Mutex<Shared>>, Arc<Notify>)>>>;

/// 别名注册表：对端的通告地址（公网映射 / 局域网）→ 注册流时观察到的规范地址。
///
/// 同一对端的数据帧可能从任一已知地址到达（LAN 直达 vs NAT 回环 hairpin），
/// 两者可能是不同的来源地址；别名表保证任一路径的帧都能路由到同一条对端流。
type AliasRegistry = Arc<std::sync::Mutex<HashMap<SocketAddr, SocketAddr>>>;

/// 共享 socket 的多对端网格流：一个 UDP socket 承载多个对端流（host 侧网格模式）。
///
/// 路由器任务统一收包：
/// - FRS1 数据帧 → 按来源地址（经别名表解析）路由到对应对端流（未注册对端的数据帧丢弃，发送方会重传）；
/// - 其他文本数据报（PUNCH/ACK/探测回复）→ 进入事件通道，交由上层打洞编排处理。
///
/// 每个对端流有独立的定时任务（flush/重传/keepalive/存活检测），发送共用同一 socket。
pub struct UdpMesh {
    socket: Arc<UdpSocket>,
    peers: PeerRegistry,
    aliases: AliasRegistry,
}

impl UdpMesh {
    /// 接管一个已绑定的 UDP socket 并启动路由器；返回打洞事件接收端。
    pub fn new(
        socket: UdpSocket,
    ) -> (
        Self,
        tokio::sync::mpsc::UnboundedReceiver<(Vec<u8>, SocketAddr)>,
    ) {
        let socket = Arc::new(socket);
        let peers = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let aliases: AliasRegistry = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let (punch_tx, punch_rx) = tokio::sync::mpsc::unbounded_channel();
        let s = socket.clone();
        let p = peers.clone();
        let a = aliases.clone();
        tokio::spawn(async move {
            mesh_router(s, p, a, punch_tx).await;
        });
        (Self {
            socket,
            peers,
            aliases,
        }, punch_rx)
    }

    /// 登记别名：`advertised`（对端通告的地址）与 `canonical`（注册流的观察地址）指向同一条流。
    ///
    /// 用于打洞信号从多个路径先后到达的场景：无论哪个来源先注册，
    /// 其余路径的帧都能路由到同一条流；流本身会从收到的帧动态学习发送地址。
    pub fn map_alias(&self, advertised: SocketAddr, canonical: SocketAddr) {
        if advertised != canonical {
            self.aliases.lock().unwrap().insert(advertised, canonical);
        }
    }

    /// 供打洞阶段使用的数据报发送（PUNCH/ACK）。
    pub async fn send(&self, data: &[u8], target: SocketAddr) -> io::Result<usize> {
        self.socket.send_to(data, target).await
    }

    /// 本网格 socket 的绑定地址。
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// 注册一个对端流（直连打洞成功后调用）。同一地址重复注册时先关闭旧流。
    ///
    /// mesh 建链由对端的打洞信号触发，此时对端的流可能尚未创建（双方建链时点
    /// 天然不对称）：建立后到对端首帧前的静默是正常的，存活检测放宽（had_rx=false）。
    pub fn add_peer(&self, peer: SocketAddr, key: Option<[u8; 32]>) -> UdpStream {
        let shared = Arc::new(Mutex::new(make_shared(peer, key, false)));
        let out_notify = Arc::new(Notify::new());
        let idle_close = Arc::new(Notify::new());
        if let Some((_, old_close)) = self
            .peers
            .lock()
            .unwrap()
            .insert(peer, (shared.clone(), idle_close.clone()))
        {
            old_close.notify_one();
        }
        let s = self.socket.clone();
        let sh = shared.clone();
        let on = out_notify.clone();
        let ic = idle_close.clone();
        tokio::spawn(async move {
            mesh_peer_timer(s, sh, on, ic).await;
        });
        UdpStream {
            shared,
            out_notify,
            idle_close,
        }
    }

    /// 移除一个对端流（停止其定时任务）。
    pub fn remove_peer(&self, peer: SocketAddr) {
        if let Some((_, ic)) = self.peers.lock().unwrap().remove(&peer) {
            ic.notify_one();
        }
    }
}

/// 路由器：收包 → FRS1 按来源路由 / 其他文本进事件通道。
async fn mesh_router(
    socket: Arc<UdpSocket>,
    peers: PeerRegistry,
    aliases: AliasRegistry,
    punch_tx: tokio::sync::mpsc::UnboundedSender<(Vec<u8>, SocketAddr)>,
) {
    let mut buf = [0u8; 2048];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, src)) => {
                let data = buf[..len].to_vec();
                if is_data_frame(&data) {
                    // 别名解析：帧可能来自对端的任一已知地址（LAN 直达 / NAT 回环）
                    let canonical = aliases.lock().unwrap().get(&src).copied().unwrap_or(src);
                    let entry = peers.lock().unwrap().get(&canonical).cloned();
                    if let Some((shared, _)) = entry {
                        handle_incoming(&socket, &shared, &data, Some(src)).await;
                    }
                    // 未注册对端的数据帧：可能打洞刚完成、首帧先于注册到达——发送方会重传
                } else {
                    log::debug!("mesh router: non-data datagram {len}B from {src}");
                    let _ = punch_tx.send((data, src));
                }
            }
            Err(e)
                if e.kind() == io::ErrorKind::ConnectionReset
                    || e.kind() == io::ErrorKind::ConnectionRefused =>
            {
                continue; // Windows ICMP 毒化，忽略
            }
            Err(e) => {
                log::warn!("mesh router socket error: {e}");
                break;
            }
        }
    }
}

/// 网格对端的定时任务：flush / 重传 / keepalive / 存活检测（与单对端 run() 的定时部分一致）。
async fn mesh_peer_timer(
    socket: Arc<UdpSocket>,
    shared: Arc<Mutex<Shared>>,
    out_notify: Arc<Notify>,
    idle_close: Arc<Notify>,
) {
    let mut last_tx = Instant::now();
    let mut retransmit = tokio::time::interval(Duration::from_millis(RETRANSMIT_MS));
    retransmit.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let done = {
            let g = shared.lock().unwrap();
            g.err.is_some() || (g.fin_acked && g.rx_closed)
        };
        if done {
            break;
        }
        tokio::select! {
            _ = out_notify.notified() => {
                flush_out(&socket, &shared).await;
            }
            _ = retransmit.tick() => {
                flush_out(&socket, &shared).await;
                on_timer(&socket, &shared, &mut last_tx).await;
            }
            _ = idle_close.notified() => {
                break;
            }
        }
    }
}

async fn run<S: crate::p2p::turn::DatagramSocket>(
    socket: S,
    shared: Arc<Mutex<Shared>>,
    out_notify: Arc<Notify>,
    idle_close: Arc<Notify>,
    first: Option<(Vec<u8>, SocketAddr)>,
) {
    if let Some((bytes, src)) = first {
        handle_incoming(&socket, &shared, &bytes, Some(src)).await;
    }
    let mut buf = [0u8; 2048];
    let mut last_tx = Instant::now();
    let mut retransmit = tokio::time::interval(Duration::from_millis(RETRANSMIT_MS));
    retransmit.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        // 双方均完成关闭 → 任务退出
        let done = {
            let g = shared.lock().unwrap();
            g.err.is_some() || (g.fin_acked && g.rx_closed)
        };
        if done {
            log::debug!("stream task exiting (fin_acked, rx_closed)");
            break;
        }
        tokio::select! {
            recv = socket.recv_from(&mut buf) => {
                match recv {
                    Ok((len, src)) => handle_incoming(&socket, &shared, &buf[..len], Some(src)).await,
                    Err(e) => {
                        // Windows：向已关闭的 UDP 端口发送过后，下次 recv 会返回
                        // WSAECONNRESET(10054)/WSAECONNREFUSED(10061)，属于正常现象，
                        // 错误已被清除、socket 仍可用，直接忽略继续。
                        if e.kind() == io::ErrorKind::ConnectionReset
                            || e.kind() == io::ErrorKind::ConnectionRefused
                        {
                            continue;
                        }
                        fail(&shared, format!("udp recv error: {e}"));
                    }
                }
            }
            _ = out_notify.notified() => {
                flush_out(&socket, &shared).await;
            }
            _ = retransmit.tick() => {
                // 先补发排队中的帧（ACK 释放窗口后可能积压数据/FIN），再重传与保活
                flush_out(&socket, &shared).await;
                on_timer(&socket, &shared, &mut last_tx).await;
            }
            _ = idle_close.notified() => {
                break;
            }
        }
    }
}

/// 处理收到的一个数据报（必须是合法 FRS1 帧）。
async fn handle_incoming<S: crate::p2p::turn::DatagramSocket>(
    socket: &S,
    shared: &Arc<Mutex<Shared>>,
    data: &[u8],
    src: Option<SocketAddr>,
) {
    let Some((kind, seq, ack, payload)) = parse_frame(data) else {
        return;
    };
    log::debug!(
        "recv kind={kind:?} seq={seq} ack={ack} len={}",
        payload.len()
    );
    // 所有共享状态操作集中在一个块内，确保锁在 await 前释放
    let reply_ack: Option<BytesMut> = {
        let mut g = shared.lock().unwrap();
        if let Some(s) = src {
            g.peer = s;
        }
        g.last_rx = Instant::now();
        g.had_rx = true; // 对端已证实存活：存活检测恢复严格阈值

        // 累积 ACK：丢弃已确认帧
        if ack > 0 {
            while let Some((s, _)) = g.unacked.front() {
                if *s < ack {
                    g.unacked.pop_front();
                } else {
                    break;
                }
            }
            if g.fin_sent && ack > g.fin_seq && !g.fin_acked {
                g.fin_acked = true;
                wake(&mut g.write_waker);
            }
            if g.unacked.len() < WINDOW {
                wake(&mut g.write_waker);
            }
        }

        let mut reply = None;
        match kind {
            FrameKind::Data | FrameKind::Fin => {
                if seq == g.next_expected {
                    if kind == FrameKind::Data {
                        // 解密数据帧负载
                        let plain: Option<Vec<u8>> = match &g.cipher {
                            Some(c) => match c.decrypt(&nonce_for(seq), payload) {
                                Ok(p) => Some(p),
                                Err(_) => {
                                    // 密钥不匹配或负载被篡改
                                    let msg = "decryption failed (wrong --key?)".to_string();
                                    drop(g);
                                    fail(shared, msg);
                                    return;
                                }
                            },
                            None => Some(payload.to_vec()),
                        };
                        if let Some(plain) = plain {
                            if g.rx_buf.len() + plain.len() <= RX_CAP {
                                g.rx_buf.extend(plain);
                                wake(&mut g.read_waker);
                            }
                            // 读缓冲满：丢弃该帧（不推进序号，发送方会重传）
                        }
                    }
                    g.next_expected += 1;
                    if kind == FrameKind::Fin {
                        g.rx_closed = true;
                        wake(&mut g.read_waker);
                    }
                }
                // 乱序或重复：go-back-N，等待发送方重传
                reply = Some(encode_frame(
                    FrameKind::Ack,
                    g.next_seq,
                    g.next_expected,
                    &[],
                ));
            }
            FrameKind::Ack => {}
        }
        reply
    };
    if let Some(frame) = reply_ack {
        let peer = {
            let g = shared.lock().unwrap();
            g.peer
        };
        let _ = socket.send_to(&frame, peer).await;
    }
}

/// 将用户写入的字节切帧发送（滑动窗口内）。
async fn flush_out<S: crate::p2p::turn::DatagramSocket>(socket: &S, shared: &Arc<Mutex<Shared>>) {
    loop {
        let send: Option<(BytesMut, SocketAddr)> = {
            let mut g = shared.lock().unwrap();
            if g.err.is_some() {
                return;
            }
            if g.unacked.len() >= WINDOW {
                return;
            }
            if let Some(payload) = g.out_queue.pop_front() {
                g.out_queue_len -= payload.len();
                let seq = g.next_seq;
                g.next_seq += 1;
                // 加密数据帧负载（重传复用同一密文）
                let wire_payload: Vec<u8> = match &g.cipher {
                    Some(c) => c
                        .encrypt(&nonce_for(seq), payload.as_slice())
                        .expect("chacha20poly1305 encrypt"),
                    None => payload,
                };
                let frame = encode_frame(FrameKind::Data, seq, g.next_expected, &wire_payload);
                let stored = frame.clone().to_vec();
                g.unacked.push_back((seq, stored));
                let peer = g.peer;
                #[cfg(test)]
                let drop_it = g.loss_rate > 0.0 && rand::random::<f64>() < g.loss_rate;
                #[cfg(not(test))]
                let drop_it = false;
                drop(g);
                if !drop_it {
                    Some((frame, peer))
                } else {
                    continue;
                }
            } else if g.write_closed && !g.fin_sent {
                let seq = g.next_seq;
                g.next_seq += 1;
                g.fin_sent = true;
                g.fin_seq = seq;
                let frame = encode_frame(FrameKind::Fin, seq, g.next_expected, &[]);
                let stored = frame.clone().to_vec();
                g.unacked.push_back((seq, stored));
                let peer = g.peer;
                drop(g);
                Some((frame, peer))
            } else {
                return;
            }
        };
        if let Some((frame, peer)) = send {
            log::debug!("send to {peer}: len={}", frame.len());
            let _ = socket.send_to(&frame, peer).await;
        }
    }
}

/// 定时任务：重传未确认帧 + keepalive + FIN 确认超时检测。
async fn on_timer<S: crate::p2p::turn::DatagramSocket>(
    socket: &S,
    shared: &Arc<Mutex<Shared>>,
    last_tx: &mut Instant,
) {
    let now = Instant::now();

    // FIN 确认超时：对端已不可达/已关闭 socket，视为关闭完成（尽力而为的 FIN 握手）
    {
        let mut g = shared.lock().unwrap();
        if g.fin_sent
            && !g.fin_acked
            && now.duration_since(g.last_rx) > Duration::from_millis(CLOSE_TIMEOUT_MS)
        {
            log::warn!(
                "FIN not acknowledged within {CLOSE_TIMEOUT_MS}ms; closing anyway (unacked={})",
                g.unacked.len()
            );
            g.fin_acked = true;
            wake(&mut g.write_waker);
            return;
        }
    }

    // 存活超时：对端长时间无任何帧（数据/ACK/FIN）→ 判定连接已死，主动报错，
    // 让上层的自动重连循环接手（对端切换网络/掉线后不再无限重传挂死）。
    // mesh 建链后从未收到对端首帧前放宽为 liveness×5：双方建链时点不对称，
    // 对端的流可能还在创建/打洞收尾，过早判死会造成"3 秒断链"重连循环。
    {
        let g = shared.lock().unwrap();
        if g.err.is_none() {
            let limit = if g.had_rx {
                g.liveness
            } else {
                g.liveness * 5
            };
            let idle = now.duration_since(g.last_rx);
            if idle > limit {
                let secs = idle.as_secs();
                drop(g);
                log::warn!("no heartbeat from peer for {secs}s; declaring connection lost");
                fail(
                    shared,
                    format!(
                        "connection lost: no heartbeat from peer for {secs}s (network switched?)"
                    ),
                );
                return;
            }
        }
    }

    // 重传所有未确认帧
    let frames: Vec<Vec<u8>> = {
        let g = shared.lock().unwrap();
        if g.err.is_some() {
            return;
        }
        g.unacked.iter().map(|(_, f)| f.clone()).collect()
    };
    if !frames.is_empty() {
        let peer = {
            let g = shared.lock().unwrap();
            g.peer
        };
        for f in frames {
            let _ = socket.send_to(&f, peer).await;
        }
        *last_tx = now;
        return;
    }

    // keepalive：空闲时发送 ACK 帧维持 NAT 映射
    if now.duration_since(*last_tx) >= Duration::from_millis(KEEPALIVE_MS) {
        let frame = {
            let g = shared.lock().unwrap();
            encode_frame(FrameKind::Ack, g.next_seq, g.next_expected, &[])
        };
        let peer = {
            let g = shared.lock().unwrap();
            g.peer
        };
        let _ = socket.send_to(&frame, peer).await;
        *last_tx = now;
    }
}

impl AsyncRead for UdpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut g = self.lock();
        if let Some(e) = &g.err {
            return Poll::Ready(Err(io::Error::other(e.clone())));
        }
        if !g.rx_buf.is_empty() {
            let n = buf.remaining().min(g.rx_buf.len());
            let bytes: Vec<u8> = g.rx_buf.drain(..n).collect();
            buf.put_slice(&bytes);
            return Poll::Ready(Ok(()));
        }
        if g.rx_closed {
            return Poll::Ready(Ok(()));
        }
        g.read_waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

impl AsyncWrite for UdpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut g = self.lock();
        if let Some(e) = &g.err {
            return Poll::Ready(Err(io::Error::other(e.clone())));
        }
        if g.write_closed {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "write after shutdown",
            )));
        }
        if g.out_queue_len >= OUT_CAP {
            g.write_waker = Some(cx.waker().clone());
            return Poll::Pending;
        }
        let to_take = buf.len().min(OUT_CAP - g.out_queue_len);
        // 加密时明文单帧上限为 1184（1200 - 16 字节 Poly1305 标签）
        let chunk = if g.cipher.is_some() {
            MAX_PLAINTEXT
        } else {
            MAX_PAYLOAD
        };
        let mut written = 0;
        while written < to_take {
            let end = (written + chunk).min(to_take);
            g.out_queue.push_back(buf[written..end].to_vec());
            g.out_queue_len += end - written;
            written = end;
        }
        drop(g);
        self.out_notify.notify_one();
        Poll::Ready(Ok(written))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        {
            let mut g = self.lock();
            if let Some(e) = &g.err {
                return Poll::Ready(Err(io::Error::other(e.clone())));
            }
            if !g.write_closed {
                g.write_closed = true;
                drop(g);
                self.out_notify.notify_one();
            } else if g.fin_acked {
                log::debug!("shutdown: fin acked");
                return Poll::Ready(Ok(()));
            }
        }
        {
            let mut g = self.lock();
            if g.fin_acked {
                Poll::Ready(Ok(()))
            } else {
                g.write_waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UdpSocket;

    #[test]
    fn frame_codec_roundtrip() {
        let f = encode_frame(FrameKind::Data, 7, 3, b"hello");
        let (kind, seq, ack, payload) = parse_frame(&f).unwrap();
        assert_eq!(kind, FrameKind::Data);
        assert_eq!(seq, 7);
        assert_eq!(ack, 3);
        assert_eq!(payload, b"hello");

        let f = encode_frame(FrameKind::Fin, 8, 4, &[]);
        let (kind, seq, ack, payload) = parse_frame(&f).unwrap();
        assert_eq!(kind, FrameKind::Fin);
        assert_eq!(seq, 8);
        assert_eq!(ack, 4);
        assert!(payload.is_empty());

        assert!(parse_frame(b"junk").is_none());
    }

    async fn pair() -> (UdpStream, UdpStream) {
        pair_with_key(None).await
    }

    async fn pair_with_key(key: Option<[u8; 32]>) -> (UdpStream, UdpStream) {
        let a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let b = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (a_addr, b_addr) = (a.local_addr().unwrap(), b.local_addr().unwrap());
        let sa = UdpStream::new(a, b_addr, None, key);
        let sb = UdpStream::new(b, a_addr, None, key);
        (sa, sb)
    }

    #[tokio::test]
    async fn stream_roundtrip() {
        let (mut a, mut b) = pair().await;
        let payload = vec![0xABu8; 64 * 1024];
        let send = payload.clone();
        let writer = tokio::spawn(async move {
            a.write_all(&send).await.unwrap();
            a.shutdown().await.unwrap();
        });
        let mut got = Vec::new();
        b.read_to_end(&mut got).await.unwrap();
        writer.await.unwrap();
        assert_eq!(got, payload);
    }

    #[tokio::test]
    async fn stream_encrypted_roundtrip() {
        let key = [7u8; 32];
        let (mut a, mut b) = pair_with_key(Some(key)).await;
        let payload: Vec<u8> = (0..65536u32).map(|i| (i % 253) as u8).collect();
        let send = payload.clone();
        let writer = tokio::spawn(async move {
            a.write_all(&send).await.unwrap();
            a.shutdown().await.unwrap();
        });
        let mut got = Vec::new();
        b.read_to_end(&mut got).await.unwrap();
        writer.await.unwrap();
        assert_eq!(got, payload);
    }

    /// 数据帧从对端的"别名"地址（如 NAT 回环映射地址）到达时，也应路由到
    /// 按"规范地址"（如局域网直达地址）注册的那条流——回归测试：
    /// 无别名路由时，公网映射来源的帧会被丢弃，造成单通死链。
    #[tokio::test]
    async fn mesh_alias_routes_frames_from_other_addr() {
        let mesh_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (mesh, _punch_rx) = UdpMesh::new(mesh_sock);
        let peer_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let canonical = peer_sock.local_addr().unwrap(); // LAN 直达来源
        let alias_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let alias_addr = alias_sock.local_addr().unwrap(); // NAT 回环来源

        let _stream = mesh.add_peer(canonical, None);
        mesh.map_alias(alias_addr, canonical);

        // 从别名地址发一帧数据 → 应被路由到规范流，且回 ACK 到别名地址
        // （流从收到的帧动态学习发送地址）
        let frame = encode_frame(FrameKind::Data, 1, 0, b"hello");
        alias_sock
            .send_to(&frame, mesh.local_addr().unwrap())
            .await
            .unwrap();

        let mut buf = [0u8; 128];
        let (n, from) = tokio::time::timeout(Duration::from_secs(2), alias_sock.recv_from(&mut buf))
            .await
            .expect("no ACK from mesh (alias frame was dropped)")
            .unwrap();
        assert_eq!(from, mesh.local_addr().unwrap());
        let (kind, _seq, ack, _) = parse_frame(&buf[..n]).expect("not an FRS1 frame");
        assert_eq!(kind, FrameKind::Ack);
        assert_eq!(ack, 2, "receiver should advance next_expected past seq=1");
    }

    /// mesh 建链后、对端首帧到达前的静默不应触发存活死亡（双方建链时点不对称）：
    /// 宽限期为 liveness×5，收到首帧后恢复严格检测。
    /// 回归测试：无宽限时，房主先建链、访客流晚 ~2s 出现，房主在访客第一帧
    /// 到达前就 3s 判死 → "3 秒断链"重连循环。
    #[tokio::test]
    async fn mesh_establish_grace_allows_late_first_frame() {
        let mesh_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let (mesh, _punch_rx) = UdpMesh::new(mesh_sock);
        let peer_sock = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let canonical = peer_sock.local_addr().unwrap();

        let mut stream = mesh.add_peer(canonical, None);
        stream.set_liveness(Duration::from_millis(400)); // 宽限 = 400ms×5 = 2s

        // 1.2s 无任何帧：严格阈值（400ms）早已超时，但宽限期内流必须存活
        tokio::time::sleep(Duration::from_millis(1200)).await;

        // 对端首帧到达 → 流应正常接收（证明宽限期未被误判）
        let frame = encode_frame(FrameKind::Data, 1, 0, b"late-hello");
        peer_sock
            .send_to(&frame, mesh.local_addr().unwrap())
            .await
            .unwrap();
        let mut buf = [0u8; 64];
        let n = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf))
            .await
            .expect("stream died during establish grace")
            .unwrap();
        assert_eq!(&buf[..n], b"late-hello");

        // 首帧后进入严格检测：对端静默 400ms → 流报错
        let r = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf)).await;
        assert!(matches!(r, Ok(Err(_))), "expected liveness error after grace, got {r:?}");
    }

    #[tokio::test]
    async fn stream_with_loss() {
        let (mut a, mut b) = pair().await;
        a.set_loss_rate(0.2);
        let payload: Vec<u8> = (0..32768u32).map(|i| (i % 251) as u8).collect();
        let send = payload.clone();
        let writer = tokio::spawn(async move {
            a.write_all(&send).await.unwrap();
            a.shutdown().await.unwrap();
        });
        let mut got = Vec::new();
        b.read_to_end(&mut got).await.unwrap();
        writer.await.unwrap();
        assert_eq!(got, payload);
    }

    #[tokio::test]
    async fn liveness_timeout_detects_dead_peer() {
        let (mut a, b) = pair().await;
        // 关闭 b：a 将永远收不到任何帧，存活超时后应报错（触发上层自动重连）
        drop(b);
        a.set_liveness(Duration::from_millis(400));
        let start = Instant::now();
        let mut buf = [0u8; 16];
        let r = a.read(&mut buf).await;
        assert!(r.is_err(), "expected liveness error, got {r:?}");
        let elapsed = start.elapsed();
        assert!(
            elapsed >= Duration::from_millis(350),
            "failed too early: {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn graceful_bidirectional_close() {
        let (mut a, mut b) = pair().await;
        let h = tokio::spawn(async move {
            let mut buf = [0u8; 32];
            let n = a.read(&mut buf).await.unwrap();
            a.write_all(&buf[..n]).await.unwrap();
            a.shutdown().await.unwrap();
            // 返回 a，保持其 socket 存活，直到对端完成关闭握手
            a
        });
        b.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 32];
        let n = b.read(&mut buf).await.unwrap();
        b.shutdown().await.unwrap();
        let _a = h.await.unwrap();
        assert_eq!(&buf[..n], b"ping");
    }
}
