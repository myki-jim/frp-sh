//! Panel 数据采集：链路统计与会话注册表。
//!
//! 每条链路（直连/TURN/TCP 中继）持有一个 [`StreamStats`]（原子计数，发送/接收
//! 出入口零锁累加）；会话层通过 [`set_links`] 把当前活跃链路注册进 [`SESSION`]，
//! 客户端面板任务从 [`links_snapshot`] 读取快照。RTT 由 FRS1 心跳往返（Ping 帧
//! 的 seq 与对端 Ack 的 ack 字段）计算。

use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// 链路类型（对应 `kind` 原子值）。
pub const KIND_DIRECT: u8 = 0;
pub const KIND_TURN: u8 = 1;
pub const KIND_RELAY: u8 = 2;

pub fn kind_name(kind: u8) -> &'static str {
    match kind {
        KIND_DIRECT => "direct",
        KIND_TURN => "turn",
        _ => "relay",
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 单条链路的统计（全部原子，收发路径零锁）。
#[derive(Default)]
pub struct StreamStats {
    pub kind: AtomicU8,
    pub connected_at: AtomicI64,
    pub sent_frames: AtomicU64,
    pub sent_bytes: AtomicU64,
    pub recv_frames: AtomicU64,
    pub recv_bytes: AtomicU64,
    /// 最近一次 RTT 采样（**微秒**；回环直连亚毫秒，毫秒精度会全变 0）
    pub rtt_last: AtomicU32,
    /// RTT 指数移动平均（微秒，α≈0.3）
    pub rtt_ewma: AtomicU32,
}

impl StreamStats {
    pub fn new(kind: u8) -> Arc<Self> {
        Arc::new(Self {
            kind: AtomicU8::new(kind),
            connected_at: AtomicI64::new(unix_now()),
            ..Default::default()
        })
    }

    pub fn on_sent(&self, bytes: usize) {
        self.sent_frames.fetch_add(1, Ordering::Relaxed);
        self.sent_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    pub fn on_recv(&self, bytes: usize) {
        self.recv_frames.fetch_add(1, Ordering::Relaxed);
        self.recv_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    /// 记录一次 RTT 采样（微秒）并更新 EWMA。
    pub fn on_rtt(&self, us: u32) {
        self.rtt_last.store(us, Ordering::Relaxed);
        let prev = self.rtt_ewma.load(Ordering::Relaxed);
        let ewma = if prev == 0 {
            us
        } else {
            // ewma = 0.7*prev + 0.3*us
            (prev.saturating_mul(7).saturating_add(us.saturating_mul(3))) / 10
        };
        self.rtt_ewma.store(ewma, Ordering::Relaxed);
    }
}

/// 一条注册进面板的链路：标识（对端/类型）+ 统计句柄。
pub struct LinkEntry {
    /// 对端显示标识（mesh 中为对端设备名，点对点为 "host"）
    pub peer: String,
    pub kind: &'static str,
    /// 详细地址（公网 UDP 地址 / 虚拟 IP 等，面板展示）
    pub detail: String,
    pub stats: Arc<StreamStats>,
}

/// 当前会话的活跃链路集合（单会话进程：一次只有一组，每次建链后整体替换）。
static LINKS: OnceLock<Mutex<Vec<LinkEntry>>> = OnceLock::new();

fn links_cell() -> &'static Mutex<Vec<LinkEntry>> {
    LINKS.get_or_init(|| Mutex::new(Vec::new()))
}

/// 用当前活跃链路整体替换注册表（会话建链/断链后调用）。
pub fn set_links(entries: Vec<LinkEntry>) {
    *links_cell().lock().unwrap() = entries;
}

/// 追加/替换链路：同 peer 已存在则替换（重连场景），否则追加。
pub fn push_link(entry: LinkEntry) {
    let mut links = links_cell().lock().unwrap();
    if let Some(slot) = links.iter_mut().find(|l| l.peer == entry.peer) {
        *slot = entry;
    } else {
        links.push(entry);
    }
}

/// 移除指定 peer 的链路（断链/对端退出）。
pub fn remove_link(peer: &str) {
    links_cell().lock().unwrap().retain(|l| l.peer != peer);
}

/// 清空链路表（新会话开始）。
pub fn clear_links() {
    links_cell().lock().unwrap().clear();
}

/// 面板快照：`[(peer, kind, detail, connected_at, sent, recv, rtt_last, rtt_ewma), …]`
pub fn links_snapshot() -> Vec<(String, String, String, i64, u64, u64, u32, u32)> {
    links_cell()
        .lock()
        .unwrap()
        .iter()
        .map(|l| {
            let s = &l.stats;
            (
                l.peer.clone(),
                l.kind.to_string(),
                l.detail.clone(),
                s.connected_at.load(Ordering::Relaxed),
                s.sent_bytes.load(Ordering::Relaxed),
                s.recv_bytes.load(Ordering::Relaxed),
                s.rtt_last.load(Ordering::Relaxed),
                s.rtt_ewma.load(Ordering::Relaxed),
            )
        })
        .collect()
}

/// 客户端会话基础信息（面板 /api/info）。会话启动时填充。
#[derive(Default, Clone)]
pub struct SessionInfo {
    pub mode: String,
    pub room: String,
    pub signaling: String,
    pub my_id: String,
    /// 设备显示名（--name / config name / 主机名）
    pub device_name: String,
    /// 本端公网 UDP 地址（探测所得）
    pub ext_addr: String,
    /// 本端局域网地址列表（逗号分隔，面板展示）
    pub lan_addrs: String,
    pub vnet_ip: String,
    pub tun: String,
    pub mtu: u32,
    pub encryption: bool,
    pub started_at: i64,
    pub reconnects: u64,
}

static SESSION_INFO: OnceLock<Mutex<SessionInfo>> = OnceLock::new();

/// 更新会话信息（面板展示用；只覆盖已填字段——空字符串保持不变）。
pub fn update_info(patch: SessionInfo) {
    let cell = SESSION_INFO.get_or_init(|| Mutex::new(SessionInfo::default()));
    let mut info = cell.lock().unwrap();
    if !patch.mode.is_empty() {
        info.mode = patch.mode;
    }
    if !patch.room.is_empty() {
        info.room = patch.room;
    }
    if !patch.signaling.is_empty() {
        info.signaling = patch.signaling;
    }
    if !patch.my_id.is_empty() {
        info.my_id = patch.my_id;
    }
    if !patch.device_name.is_empty() {
        info.device_name = patch.device_name;
    }
    if !patch.ext_addr.is_empty() {
        info.ext_addr = patch.ext_addr;
    }
    if !patch.lan_addrs.is_empty() {
        info.lan_addrs = patch.lan_addrs;
    }
    if !patch.vnet_ip.is_empty() {
        info.vnet_ip = patch.vnet_ip;
    }
    if !patch.tun.is_empty() {
        info.tun = patch.tun;
    }
    if patch.mtu > 0 {
        info.mtu = patch.mtu;
    }
    info.encryption = patch.encryption || info.encryption;
    if patch.started_at > 0 {
        info.started_at = patch.started_at;
    }
    if patch.reconnects > 0 {
        info.reconnects = patch.reconnects;
    }
}

pub fn info_snapshot() -> SessionInfo {
    SESSION_INFO
        .get_or_init(|| Mutex::new(SessionInfo::default()))
        .lock()
        .unwrap()
        .clone()
}

/// 收发速率（字节/秒）：由快照间隔与字节数差在前端计算，这里不维护历史。
pub type LinkRow = (String, String, i64, u64, u64, u32, u32);

/// 供 JSON 序列化的链路快照（rtt_* 输出为毫秒浮点；bps 由前端采样差值计算）。
pub fn links_json() -> Vec<serde_json::Map<String, serde_json::Value>> {
    links_snapshot()
        .into_iter()
        .map(
            |(peer, kind, detail, since, sent, recv, rtt_last, rtt_ewma)| {
                let mut m = serde_json::Map::new();
                m.insert("peer".into(), serde_json::json!(peer));
                m.insert("kind".into(), serde_json::json!(kind));
                m.insert("detail".into(), serde_json::json!(detail));
                m.insert("since".into(), serde_json::json!(since));
                m.insert("sent_bytes".into(), serde_json::json!(sent));
                m.insert("recv_bytes".into(), serde_json::json!(recv));
                m.insert(
                    "rtt_last".into(),
                    serde_json::json!((rtt_last as f64 / 1000.0 * 10.0).round() / 10.0),
                );
                m.insert(
                    "rtt_ewma".into(),
                    serde_json::json!((rtt_ewma as f64 / 1000.0 * 10.0).round() / 10.0),
                );
                m
            },
        )
        .collect()
}
