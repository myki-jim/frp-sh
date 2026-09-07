//! 虚拟网卡（TUN/L3）模式：把打洞/中继数据通道封装成 IP 隧道。
//!
//! 两端各创建一个 TUN 设备并分配虚拟 IP（如 `10.66.0.1/24` 与 `10.66.0.2/24`），
//! 设备收到的 IP 包经数据通道转发到对端写入其 TUN 设备——形成虚拟局域网，
//! TCP/UDP/ICMP 等任意 IP 流量均可承载（含局域网游戏的 UDP/广播流量）。
//!
//! 平台支持：
//! - Linux：`/dev/net/tun`（需 root / CAP_NET_ADMIN）
//! - macOS：`utun`（需管理员权限）
//! - Windows：Wintun 驱动（需管理员权限；`wintun.dll` 放在可执行文件旁，
//!   位于辅助服务受保护的安装目录）
//!
//! 帧格式与隧道层一致：`[u32 len][payload]` 传输 IP 包。

use crate::error::{FrpError, Result};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(test)]
mod session_tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn replacing_peer_closes_both_old_halves_and_ignores_old_death() {
        let (plane, mut dead) = MeshPlane::new();
        let (tx, mut packets) = tokio::sync::mpsc::channel(256);
        let (old, mut remote) = tokio::io::duplex(1024);
        plane.register("peer", vec![], Box::new(old), tx.clone(), None);
        let (new, remote_new) = tokio::io::duplex(1024);
        plane.register("peer", vec![], Box::new(new), tx.clone(), None);
        let mut byte = [0u8];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), remote.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        assert!(
            dead.try_recv().is_err(),
            "intentional replacement must not report a dead new peer"
        );
        drop(remote_new);
        let old_death = tokio::time::timeout(Duration::from_secs(1), dead.recv())
            .await
            .unwrap()
            .unwrap();
        let (third, mut remote_third) = tokio::io::duplex(1024);
        plane.register("peer", vec![], Box::new(third), tx, None);
        assert!(!plane.is_current_death(&old_death));
        crate::tunnel::write_frame(&mut remote_third, b"new-link-packet")
            .await
            .unwrap();
        assert_eq!(packets.recv().await.unwrap().1, b"new-link-packet");
        plane.unregister("peer");
    }
    #[tokio::test(start_paused = true)]
    async fn silent_peer_is_detected_without_waiting_for_tcp_keepalive() {
        let (guest, _silent_peer) = tokio::io::duplex(1024);
        let (device, _helper) = tokio::io::duplex(1024);
        let task = tokio::spawn(run_packets(guest, device));
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(13)).await;
        assert!(task
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("heartbeat timeout"));
    }
    #[tokio::test(start_paused = true)]
    async fn idle_direct_mesh_answers_guest_heartbeats() {
        let (guest, host) = tokio::io::duplex(4096);
        let (device, _helper) = tokio::io::duplex(1024);
        let task = tokio::spawn(run_packets(guest, device));
        let (plane, _dead) = MeshPlane::new();
        let (tx, _packets) = tokio::sync::mpsc::channel(256);
        plane.register("peer", vec![], Box::new(host), tx, None);
        for _ in 0..10 {
            tokio::time::advance(Duration::from_secs(3)).await;
            for _ in 0..12 {
                tokio::task::yield_now().await;
            }
            assert!(!task.is_finished());
        }
        plane.unregister("peer");
        task.abort();
    }

    #[tokio::test]
    async fn guest_answers_mesh_heartbeats_without_sending_them_to_helper() {
        let (guest, mut host) = tokio::io::duplex(1024);
        let (device, mut helper) = tokio::io::duplex(1024);
        let session = tokio::spawn(run_packets(guest, device));
        for _ in 0..3 {
            crate::tunnel::write_frame(&mut host, b"FRPING")
                .await
                .unwrap();
            let mut pong = [0; 10];
            tokio::time::timeout(Duration::from_secs(1), host.read_exact(&mut pong))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&pong, b"\0\0\0\x06FRPONG");
            crate::tunnel::write_frame(&mut host, b"FRPONG")
                .await
                .unwrap();
        }
        let mut packet = [0u8; 20];
        packet[0] = 0x45;
        packet[3] = 20;
        crate::tunnel::write_frame(&mut host, &packet)
            .await
            .unwrap();
        let mut received = [0; 20];
        tokio::time::timeout(Duration::from_secs(1), helper.read_exact(&mut received))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            received, packet,
            "control frames must never reach the helper"
        );
        assert!(!session.is_finished());
        crate::tunnel::write_frame(&mut host, &[]).await.unwrap();
        session.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn encrypted_mesh_relay_stays_alive_across_heartbeats_and_reports_rtt() {
        let (host, guest) = tokio::io::duplex(65536);
        let key = [7; 32];
        let host = crate::p2p::enc::EncStream::new(host, &key);
        let guest = crate::p2p::enc::EncStream::new(guest, &key);
        let (device, mut helper) = tokio::io::duplex(1024);
        let session = tokio::spawn(run_packets(guest, device));
        let (plane, mut dead) = MeshPlane::new();
        let (dispatch, mut packets) = tokio::sync::mpsc::channel(256);
        let stats = crate::stats::StreamStats::new(crate::stats::KIND_RELAY);
        plane.register(
            "guest",
            vec![],
            Box::new(host),
            dispatch,
            Some(stats.clone()),
        );
        tokio::time::sleep(Duration::from_millis(6300)).await;
        assert!(!session.is_finished(), "guest died after mesh heartbeat");
        assert!(dead.try_recv().is_err());
        assert!(stats.rtt_last.load(std::sync::atomic::Ordering::Relaxed) > 0);
        let mut packet = [0u8; 20];
        packet[0] = 0x45;
        packet[3] = 20;
        helper.write_all(&packet).await.unwrap();
        let (_, received) = tokio::time::timeout(Duration::from_secs(1), packets.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(received, packet);
        plane.unregister("guest");
        tokio::time::timeout(Duration::from_secs(1), session)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
    }

    #[tokio::test]
    async fn helper_disconnect_is_reported_as_failure() {
        let (guest, _host) = tokio::io::duplex(1024);
        let (device, helper) = tokio::io::duplex(1024);
        drop(helper);
        let error = run_packets(guest, device).await.unwrap_err();
        assert!(error.to_string().contains("network helper closed"));
    }
}

/// TUN 设备参数。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TunConfig {
    #[serde(default)]
    pub allow_lan: bool,
    pub name: String,
    pub ip: String,
    pub netmask: String,
    pub mtu: u16,
}

/// 创建并启用 TUN 设备。
///
/// - Linux：设备名可自定义（如 `frp0`），需 root / CAP_NET_ADMIN
/// - macOS：utun 设备名由系统分配（`utunN`），**不能自定义**（否则报
///   `invalid device tun name`）；不设置名称即可，创建后实际名为 `utunN`
/// - Windows：设备名取自 Wintun；需要 `wintun.dll` 放在可执行文件旁，
///   或用 `WINTUN_DLL` 环境变量指定路径，且需管理员权限
pub(crate) fn create_local(cfg: &TunConfig) -> Result<tun::AsyncDevice> {
    let mut c = tun::configure();
    #[cfg(target_os = "macos")]
    {
        // macOS utun 名称由系统分配，跳过自定义名称
        let _ = &cfg.name;
    }
    #[cfg(not(target_os = "macos"))]
    c.tun_name(&cfg.name);
    c.address(&cfg.ip).netmask(&cfg.netmask).mtu(cfg.mtu).up();
    #[cfg(target_os = "windows")]
    {
        let dll = std::env::current_exe()
            .map_err(FrpError::Io)?
            .parent()
            .ok_or_else(|| FrpError::Tun("Missing installation directory".into()))?
            .join("wintun.dll")
            .into_os_string();
        c.platform_config(|pc| pc.wintun_file(dll));
    }
    tun::create_as_async(&c).map_err(|e| {
        let hint = if cfg!(target_os = "windows") {
            "; Windows requires wintun.dll in the protected helper installation directory; repair the installation if it is missing"
        } else {
            ""
        };
        FrpError::Tun(format!("Failed to create TUN device{hint}: {e}"))
    })
}

/// 获取 TUN 设备的实际系统名称（macOS 为 `utunN`，Linux 为自定义名）。
pub(crate) fn device_name_local(dev: &tun::AsyncDevice) -> Option<String> {
    use tun::AbstractDevice;
    dev.tun_name().ok()
}

/// 为虚拟网段添加经 TUN 设备的网络路由。
///
/// 仅 macOS 需要：utun 是**点对点**接口，设置接口地址后内核不会自动生成
/// 整个网段的路由，导致对端虚拟 IP 的回包路由失败（ping 不通）。
/// 必须显式 `route -n add -net <cidr> -interface <dev>`。
/// Linux（普通接口，自动 on-link 路由）与 Windows（wintun 同理）无需处理。
pub(crate) fn add_subnet_route_local(cidr: &str, dev: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        run_cmd("route", &["-n", "add", "-net", cidr, "-interface", dev])
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (cidr, dev);
        Ok(())
    }
}

// A Darwin point-to-point address is not automatically routed through loopback.
// Without this /32 exception, self traffic enters utun and is sent to the peer.
#[cfg(target_os = "macos")]
pub(crate) fn local_address_route(ip: &str, add: bool) -> Result<()> {
    run_cmd(
        "route",
        &[
            "-n",
            if add { "add" } else { "delete" },
            "-host",
            ip,
            "-interface",
            "lo0",
            "-ifa",
            ip,
            "-ifp",
            "lo0",
        ],
    )
}

/// Helper-owned firewall rule, scoped to this virtual interface only.
#[cfg(target_os = "windows")]
pub(crate) fn allow_firewall_local(iface: &str) -> Result<()> {
    let script = format!("New-NetFirewallRule -DisplayName 'frp-sh-{iface}' -Direction Inbound -Action Allow -InterfaceAlias '{iface}' -Profile Any -ErrorAction Stop | Out-Null");
    run_cmd(
        "powershell",
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ],
    )
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn allow_firewall_local(_iface: &str) -> Result<()> {
    Ok(())
}

/// 在 TUN 设备与数据通道之间双向转发 IP 包，直到会话结束。
pub async fn run<TR>(transport: TR, dev: crate::helper::Device) -> Result<()>
where
    TR: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    run_packets(transport, dev).await
}

async fn run_packets<TR, D>(mut transport: TR, mut dev: D) -> Result<()>
where
    TR: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    D: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut pkt = [0u8; 65536];
    let mut acc: Vec<u8> = Vec::new();
    let mut rbuf = [0u8; 16384];
    let mut last_received = tokio::time::Instant::now();
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(3));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await;
    loop {
        tokio::select! {
            // biased + 单次 read：未选中的分支要么未轮询要么处于 pending（未消费数据），不会丢包
            biased;
            _ = tokio::time::sleep_until(last_received + std::time::Duration::from_secs(12)) => {
                return Err(FrpError::Protocol("peer heartbeat timeout".into()));
            }
            _ = heartbeat.tick() => {
                crate::tunnel::write_frame(&mut transport, b"FRPING").await?;
                transport.flush().await?;
            }
            r = dev.read(&mut pkt) => {
                match r {
                    Ok(0) => return Err(FrpError::Tun("network helper closed the device session".into())),
                    Err(e) => return Err(FrpError::Tun(format!("network helper read failed: {e}"))),
                    Ok(n) => {
                        crate::tunnel::write_frame(&mut transport, &pkt[..n])
                            .await
                            .map_err(FrpError::Io)?;
                    }
                }
            }
            n = tokio::time::timeout_at(last_received + std::time::Duration::from_secs(12), transport.read(&mut rbuf)) => {
                let n = n.map_err(|_| FrpError::Protocol("peer heartbeat timeout".into()))?;
                let n = match n {
                    Ok(0) => return Err(FrpError::Protocol("peer transport closed".into())),
                    Err(e) => return Err(FrpError::Io(e)),
                    Ok(n) => n,
                };
                last_received = tokio::time::Instant::now();
                acc.extend_from_slice(&rbuf[..n]);
                loop {
                    match crate::tunnel::take_frame(&mut acc) {
                        Ok(Some(f)) if f.is_empty() => return Ok(()),
                        // Mesh heartbeat frames belong to the transport, never the IPv4 helper.
                        Ok(Some(f)) if f == b"FRPING" => {
                            crate::tunnel::write_frame(&mut transport, b"FRPONG").await?;
                            transport.flush().await?;
                        }
                        Ok(Some(f)) if f == b"FRPONG" => {}
                        Ok(Some(f)) => {
                            dev.write_all(&f).await.map_err(FrpError::Io)?;
                        }
                        Ok(None) => break,
                        Err(_) => {
                            return Err(FrpError::Protocol("tunnel frame exceeds the limit".into()));
                        }
                    }
                }
            }
        }
    }
}

// ---------- 网格（多访客）数据平面 ----------

/// 网格数据平面：TUN 与多个对端流之间按目标 IP 路由。
///
/// - TUN 读到的 IP 包 → 按目标 IP 最长前缀匹配 → 对应对端写通道（无匹配则丢弃）
/// - 对端流读到的 IP 包 → 目标为其他对端 → 直接转发；否则写入 TUN（交给本机网络栈）
pub struct LinkDeath {
    pub uuid: String,
    lifetime: Arc<tokio_util::sync::CancellationToken>,
}
pub struct MeshPlane {
    /// uuid → 路由表 (网络地址, 前缀)，含访客虚拟 IP(/32) 与暴露的局域网子网
    routes: std::sync::RwLock<std::collections::HashMap<String, Vec<(u32, u8)>>>,
    /// uuid → 对端写通道
    writers:
        std::sync::RwLock<std::collections::HashMap<String, tokio::sync::mpsc::Sender<Vec<u8>>>>,
    /// uuid → 关闭通知（unregister 时唤醒 reader 退出）
    closes: std::sync::RwLock<
        std::collections::HashMap<String, Arc<tokio_util::sync::CancellationToken>>,
    >,
    /// 对端死亡通知（reader 结束时发送 uuid）
    dead_tx: tokio::sync::mpsc::UnboundedSender<LinkDeath>,
}

impl MeshPlane {
    pub fn new() -> (Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<LinkDeath>) {
        let (dead_tx, dead_rx) = tokio::sync::mpsc::unbounded_channel();
        (
            Arc::new(Self {
                routes: Default::default(),
                writers: Default::default(),
                closes: Default::default(),
                dead_tx,
            }),
            dead_rx,
        )
    }

    /// 注册（或替换）一个对端链路：spawn 读/写任务，按 `routes` 参与选路。
    /// `dispatch_tx`：对端读到的 IP 包发往该通道，由分发器统一路由。
    /// `stats`：可选统计句柄 —— 提供时本链路每 3s 发送 `FRPING` 心跳帧并在收到
    /// `FRPONG` 后记录端到端 RTT（TCP 中继无 UDP 层 ping，由此补齐延迟显示；
    /// 旧版对端不识别该帧，只会将其作为非 IP 包丢弃，完全兼容）。
    pub fn register(
        &self,
        uuid: &str,
        routes: Vec<(u32, u8)>,
        transport: Box<dyn crate::p2p::relay::AsyncReadWrite>,
        dispatch_tx: tokio::sync::mpsc::Sender<(String, Vec<u8>)>,
        stats: Option<Arc<crate::stats::StreamStats>>,
    ) {
        self.unregister(uuid); // 清理旧链路（如中继 → 直连切换）
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        let close = Arc::new(tokio_util::sync::CancellationToken::new());
        let (mut rd, mut wr) = tokio::io::split(transport);
        self.routes
            .write()
            .unwrap()
            .insert(uuid.to_string(), routes);
        self.writers.write().unwrap().insert(uuid.to_string(), tx);
        self.closes
            .write()
            .unwrap()
            .insert(uuid.to_string(), close.clone());
        let uuid_r = uuid.to_string();
        let dead_tx = self.dead_tx.clone();
        let dt = dispatch_tx.clone();
        let close_r = close.clone();
        let tx_pong = self
            .writers
            .read()
            .unwrap()
            .get(uuid)
            .cloned()
            .expect("writer channel just inserted");
        // FRPING 发出时刻（writer 写入 / reader 收到 PONG 时读取）
        let ping_at: Arc<std::sync::Mutex<Option<std::time::Instant>>> =
            Arc::new(std::sync::Mutex::new(None));
        let ping_at_r = ping_at.clone();
        let st_r = stats.clone();
        // reader：对端流 → 帧解码 → dispatch（会话结束/关闭 → 通知死亡）
        tokio::spawn(async move {
            let mut acc: Vec<u8> = Vec::new();
            let mut rbuf = [0u8; 16384];
            'reader: loop {
                tokio::select! {
                    _ = close_r.cancelled() => return,
                    r = tokio::time::timeout(std::time::Duration::from_secs(if st_r.is_some() { 12 } else { 86400 }), rd.read(&mut rbuf)) => {
                        let r = match r { Ok(r) => r, Err(_) => { log::warn!("peer heartbeat timeout"); break; } };
                        match r {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                acc.extend_from_slice(&rbuf[..n]);
                                loop {
                                    match crate::tunnel::take_frame(&mut acc) {
                                        Ok(Some(f)) if f.is_empty() => { let _ = dt.try_send((uuid_r.clone(), Vec::new())); }
                                        // 心跳：FRPING → 原路回 FRPONG；FRPONG → 记录 RTT
                                        Ok(Some(f)) if f == b"FRPING" => {
                                            let _ = tx_pong.try_send(b"FRPONG".to_vec());
                                        }
                                        Ok(Some(f)) if f == b"FRPONG" => {
                                            if let Some(st) = &st_r {
                                                let sent = ping_at_r.lock().unwrap().take();
                                                if let Some(t) = sent {
                                                    st.on_rtt(t.elapsed().as_micros() as u32);
                                                }
                                            }
                                        }
                                        Ok(Some(f)) => { let _ = dt.try_send((uuid_r.clone(), f)); }
                                        Ok(None) => break,
                                        Err(_) => break 'reader,
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !close_r.is_cancelled() {
                let _ = dead_tx.send(LinkDeath {
                    uuid: uuid_r,
                    lifetime: close_r.clone(),
                });
                close_r.cancel();
            }
        });
        // writer：发包 → 帧封装 → 对端流（tx 被移除/丢弃时 rx 关闭 → 退出）；
        // 有统计句柄时每 3s 发送一次 FRPING 心跳（测量经中继的端到端 RTT）
        let writer_dead = self.dead_tx.clone();
        let writer_uuid = uuid.to_string();
        tokio::spawn(async move {
            let writing = async {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(3));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                tick.tick().await; // 首个 tick 立即返回，跳过
                loop {
                    tokio::select! {
                        pkt = rx.recv() => {
                            match pkt {
                                Some(p) => {
                                    if crate::tunnel::write_frame(&mut wr, &p).await.is_err() {
                                        break;
                                    }
                                }
                                None => break,
                            }
                        }
                        _ = tick.tick() => {
                            if stats.is_some() {
                                *ping_at.lock().unwrap() = Some(std::time::Instant::now());
                                if crate::tunnel::write_frame(&mut wr, b"FRPING").await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
            };
            tokio::select! { _ = close.cancelled() => {}, _ = writing => { if !close.is_cancelled() { let _ = writer_dead.send(LinkDeath { uuid: writer_uuid, lifetime: close.clone() }); close.cancel(); } } }
        });
    }

    pub fn is_current_death(&self, death: &LinkDeath) -> bool {
        self.closes
            .read()
            .unwrap()
            .get(&death.uuid)
            .is_some_and(|c| Arc::ptr_eq(c, &death.lifetime))
    }

    /// 移除一个对端链路（停止其读写任务）。
    pub fn unregister(&self, uuid: &str) {
        self.routes.write().unwrap().remove(uuid);
        self.writers.write().unwrap().remove(uuid);
        if let Some(c) = self.closes.write().unwrap().remove(uuid) {
            c.cancel();
        }
    }

    fn source_allowed(&self, uuid: &str, pkt: &[u8]) -> bool {
        if pkt.len() < 20 || pkt[0] >> 4 != 4 {
            return false;
        }
        let source = u32::from_be_bytes(pkt[12..16].try_into().unwrap());
        self.routes.read().unwrap().get(uuid).is_some_and(|routes| {
            routes.iter().any(|(net, prefix)| {
                *prefix <= 32
                    && source & (u32::MAX.checked_shl(32 - *prefix as u32).unwrap_or(0))
                        == *net & (u32::MAX.checked_shl(32 - *prefix as u32).unwrap_or(0))
            })
        })
    }
    /// 为 IP 包选择目标链路（最长前缀匹配；无匹配返回 None）。
    fn route_for(&self, pkt: &[u8]) -> Option<tokio::sync::mpsc::Sender<Vec<u8>>> {
        if pkt.len() < 20 || (pkt[0] >> 4) != 4 {
            return None; // 仅支持 IPv4（虚拟网段为 IPv4）
        }
        let dst = u32::from_be_bytes([pkt[16], pkt[17], pkt[18], pkt[19]]);
        let routes = self.routes.read().unwrap();
        let mut best: Option<(u8, String)> = None;
        for (uuid, rs) in routes.iter() {
            for (net, prefix) in rs {
                let mask = if *prefix == 0 {
                    0
                } else {
                    u32::MAX << (32 - *prefix)
                };
                if (dst & mask) == (net & mask)
                    && best.as_ref().map(|(p, _)| *prefix > *p).unwrap_or(true)
                {
                    best = Some((*prefix, uuid.clone()));
                }
            }
        }
        drop(routes);
        if let Some((_, uuid)) = best {
            self.writers.read().unwrap().get(&uuid).cloned()
        } else {
            None
        }
    }
}

/// 网格数据平面主循环：TUN 读 → 路由到对端；对端包 → 路由到对端或写入 TUN。
pub async fn run_mesh_plane(
    mut dev: crate::helper::Device,
    plane: Arc<MeshPlane>,
    mut dispatch_rx: tokio::sync::mpsc::Receiver<(String, Vec<u8>)>,
) -> Result<()> {
    let mut pkt = [0u8; 65536];
    loop {
        tokio::select! {
            biased;
            r = dev.read(&mut pkt) => {
                match r {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if let Some(tx) = plane.route_for(&pkt[..n]) {
                            let _ = tx.try_send(pkt[..n].to_vec());
                        }
                    }
                }
            }
            msg = dispatch_rx.recv() => {
                match msg {
                    Some((_, f)) if f.is_empty() => { /* 对端结束会话，等 reader 报死亡 */ }
                    Some((uuid, f)) => {
                        if !plane.source_allowed(&uuid,&f) { continue; }
                        // 目标为其他对端 → 直转；否则交给本机网络栈
                        if let Some(tx) = plane.route_for(&f) {
                            let _ = tx.try_send(f);
                        } else {
                            if dev.write_all(&f).await.is_err() { break; }
                        }
                    }
                    None => break,
                }
            }
        }
    }
    Ok(())
}

/// 为访客添加一条经 TUN 设备访问房主局域网的路由（需 root/管理员）。
///
/// `cidr` 形如 `192.168.1.0/24`；`gateway` 为房主虚拟 IP（10.66.0.1）。
pub(crate) fn add_route_local(cidr: &str, dev_name: &str, gateway: &str) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let _ = gateway; // Linux 用 dev 而非网关
        run_cmd("ip", &["route", "add", cidr, "dev", dev_name])
    }
    #[cfg(target_os = "macos")]
    {
        let _ = dev_name;
        run_cmd("route", &["-n", "add", "-net", cidr, gateway])
    }
    #[cfg(target_os = "windows")]
    {
        let _ = dev_name;
        // Windows：route add <network> mask <mask> <gateway>
        let (net, mask) = crate::utils::parse_cidr(cidr)
            .map(|(n, p)| {
                let mask = crate::utils::mask_from_prefix(p);
                let octets = mask.to_be_bytes();
                (
                    std::net::Ipv4Addr::from(n).to_string(),
                    format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3]),
                )
            })
            .ok_or_else(|| FrpError::Tun(format!("bad cidr {cidr}")))?;
        run_cmd("route", &["add", &net, "mask", &mask, gateway])
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(FrpError::Tun(format!(
            "auto route addition is not supported on this platform; run manually: ip route add {cidr} dev {dev_name}"
        )))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn run_cmd(program: &str, args: &[&str]) -> Result<()> {
    let mut command = std::process::Command::new(trusted_program(program)?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| {
            FrpError::Tun(format!(
                "{program} failed (requires root/administrator privileges): {e}"
            ))
        })?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(FrpError::Tun(format!(
                "{program} timed out in the network helper"
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    if status.success() {
        Ok(())
    } else {
        let mut error = String::new();
        if let Some(stderr) = child.stderr.take() {
            use std::io::Read;
            let _ = stderr.take(4096).read_to_string(&mut error);
        }
        Err(FrpError::Tun(format!(
            "{program} returned an error: {}",
            error.trim()
        )))
    }
}

// Public TUN operations use the installed helper, including when CLI happens to be elevated.
pub async fn create(cfg: &TunConfig) -> Result<crate::helper::Device> {
    crate::helper::open(cfg)
        .await
        .map_err(|e| FrpError::Tun(e.to_string()))
}
pub fn device_name(dev: &crate::helper::Device) -> Option<String> {
    Some(dev.name().into())
}
pub async fn allow_firewall(name: &str) -> Result<()> {
    // The helper completes scoped firewall setup before acknowledging Open.
    if name.is_empty() {
        return Err(FrpError::Tun("Missing device name".into()));
    }
    Ok(())
}
pub async fn add_subnet_route(_cidr: &str, name: &str) -> Result<()> {
    // The virtual-subnet route is owned by the same helper lease as the device.
    allow_firewall(name).await
}
pub async fn add_route(cidr: &str, name: &str, _gateway: &str) -> Result<()> {
    crate::helper::route(name, cidr)
        .await
        .map_err(|e| FrpError::Tun(e.to_string()))
}
pub(crate) fn delete_route_local(cidr: &str, name: &str, gateway: &str) -> Result<()> {
    let _ = gateway;
    #[cfg(target_os = "linux")]
    {
        let _ = gateway;
        run_cmd("ip", &["route", "del", cidr, "dev", name])
    }
    #[cfg(target_os = "macos")]
    {
        let _ = gateway;
        run_cmd("route", &["-n", "delete", "-net", cidr, "-interface", name])
    }
    #[cfg(windows)]
    {
        let _ = name;
        let (net, prefix) =
            crate::utils::parse_cidr(cidr).ok_or_else(|| FrpError::Tun("Invalid route".into()))?;
        let mask = std::net::Ipv4Addr::from(crate::utils::mask_from_prefix(prefix)).to_string();
        run_cmd(
            "route",
            &[
                "delete",
                &std::net::Ipv4Addr::from(net).to_string(),
                "mask",
                &mask,
                gateway,
            ],
        )
    }
}
pub(crate) fn remove_firewall_local(name: &str) -> Result<()> {
    #[cfg(windows)]
    {
        let script = format!(
            "Remove-NetFirewallRule -DisplayName 'frp-sh-{name}' -ErrorAction SilentlyContinue"
        );
        run_cmd(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", &script],
        )
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        Ok(())
    }
}
#[cfg(unix)]
static FORWARDING: std::sync::Mutex<(usize, Option<String>)> = std::sync::Mutex::new((0, None));
#[cfg(unix)]
fn forwarding_key() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "net.ipv4.ip_forward"
    }
    #[cfg(target_os = "macos")]
    {
        "net.inet.ip.forwarding"
    }
}
pub(crate) fn enable_forward_local(name: &str) -> Result<()> {
    #[cfg(windows)]
    {
        let script=format!("Set-NetIPInterface -InterfaceAlias '{name}' -AddressFamily IPv4 -Forwarding Enabled -ErrorAction Stop");
        run_cmd(
            "powershell",
            &["-NoProfile", "-NonInteractive", "-Command", &script],
        )
    }
    #[cfg(unix)]
    {
        let _ = name;
        let mut state = FORWARDING.lock().unwrap();
        if state.0 == 0 {
            let out = std::process::Command::new(trusted_program("sysctl")?)
                .args(["-n", forwarding_key()])
                .output()?;
            if !out.status.success() {
                return Err(FrpError::Tun("Cannot read forwarding state".into()));
            }
            let prior = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if prior != "0" && prior != "1" {
                return Err(FrpError::Tun("Unexpected forwarding state".into()));
            }
            run_cmd("sysctl", &["-w", &format!("{}=1", forwarding_key())])?;
            state.1 = Some(prior);
        }
        state.0 += 1;
        Ok(())
    }
}
pub(crate) fn release_forward_local() {
    #[cfg(unix)]
    {
        let mut state = FORWARDING.lock().unwrap();
        state.0 = state.0.saturating_sub(1);
        if state.0 == 0 {
            if let Some(prior) = state.1.take() {
                let _ = run_cmd("sysctl", &["-w", &format!("{}={prior}", forwarding_key())]);
            }
        }
    }
}
fn trusted_program(program: &str) -> Result<std::path::PathBuf> {
    #[cfg(windows)]
    {
        let root = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into());
        match program {
            "powershell" => Ok(root.join("System32/WindowsPowerShell/v1.0/powershell.exe")),
            "route" => Ok(root.join("System32/route.exe")),
            _ => Err(FrpError::Tun("Unsupported system operation".into())),
        }
    }
    #[cfg(unix)]
    {
        if !matches!(program, "ip" | "route" | "sysctl") {
            return Err(FrpError::Tun("Unsupported system operation".into()));
        }
        for prefix in ["/usr/sbin", "/sbin", "/usr/bin", "/bin"] {
            let path = std::path::Path::new(prefix).join(program);
            if path.is_file() {
                return Ok(path);
            }
        }
        Err(FrpError::Tun(format!(
            "Required system tool not installed: {program}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一个 IPv4 包（版本 4 + 目标 IP），用于验证选路。
    fn pkt(dst: [u8; 4]) -> Vec<u8> {
        let mut p = vec![0u8; 20];
        p[0] = 0x45; // IPv4, 20B header
        p[16..20].copy_from_slice(&dst);
        p
    }

    #[test]
    fn mesh_route_longest_prefix_match() {
        let (plane, _dead_rx) = MeshPlane::new();
        // 注册两个对端：A = 10.66.0.5/32，B = 10.66.0.0/24
        plane
            .routes
            .write()
            .unwrap()
            .insert("A".into(), vec![(u32::from_be_bytes([10, 66, 0, 5]), 32)]);
        plane
            .routes
            .write()
            .unwrap()
            .insert("B".into(), vec![(u32::from_be_bytes([10, 66, 0, 0]), 24)]);
        let (tx_a, _rx_a) = tokio::sync::mpsc::channel::<Vec<u8>>(256);
        let (tx_b, mut rx_b) = tokio::sync::mpsc::channel::<Vec<u8>>(256);
        plane.writers.write().unwrap().insert("A".into(), tx_a);
        plane.writers.write().unwrap().insert("B".into(), tx_b);

        // 精确命中 /32（最长前缀优先 → A）
        let r = plane.route_for(&pkt([10, 66, 0, 5]));
        assert!(r.is_some());
        let _ = r.unwrap().try_send(pkt([1, 2, 3, 4]));
        // 走 /24 → B
        let r = plane.route_for(&pkt([10, 66, 0, 9]));
        assert!(r.is_some());
        let _ = r.unwrap().try_send(pkt([5, 6, 7, 8]));
        // 网段外 → 无路由
        assert!(plane.route_for(&pkt([10, 66, 1, 1])).is_none());
        // 非 IPv4 / 太短 → 无路由
        assert!(plane.route_for(&[0x60, 0, 0, 0, 0, 0, 0, 0]).is_none());
        assert!(plane.route_for(&[0x45, 0, 0]).is_none());
        // B 的写通道应收到经 B 路由的包
        drop(plane);
        assert!(rx_b.try_recv().is_ok());
    }
}
