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
//!   或用 `WINTUN_DLL` 环境变量指定路径）
//!
//! 帧格式与隧道层一致：`[u32 len][payload]` 传输 IP 包。

use crate::error::{FrpError, Result};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// TUN 设备参数。
#[derive(Debug, Clone)]
pub struct TunConfig {
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
pub fn create(cfg: &TunConfig) -> Result<tun::AsyncDevice> {
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
        let dll = std::env::var("WINTUN_DLL")
            .map(std::ffi::OsString::from)
            .unwrap_or_else(|_| {
                std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.join("wintun.dll").into_os_string()))
                    .unwrap_or_else(|| std::ffi::OsString::from("wintun.dll"))
            });
        c.platform_config(|pc| pc.wintun_file(dll));
    }
    tun::create_as_async(&c).map_err(|e| {
        let hint = if cfg!(target_os = "windows") {
            "; Windows requires wintun.dll next to the executable (or set via the WINTUN_DLL env var), and administrator privileges"
        } else {
            ""
        };
        FrpError::Tun(format!("Failed to create TUN device{hint}: {e}"))
    })
}

/// 获取 TUN 设备的实际系统名称（macOS 为 `utunN`，Linux 为自定义名）。
pub fn device_name(dev: &tun::AsyncDevice) -> Option<String> {
    use tun::AbstractDevice;
    dev.tun_name().ok()
}

/// 为虚拟网段添加经 TUN 设备的网络路由。
///
/// 仅 macOS 需要：utun 是**点对点**接口，设置接口地址后内核不会自动生成
/// 整个网段的路由，导致对端虚拟 IP 的回包路由失败（ping 不通）。
/// 必须显式 `route -n add -net <cidr> -interface <dev>`。
/// Linux（普通接口，自动 on-link 路由）与 Windows（wintun 同理）无需处理。
pub fn add_subnet_route(cidr: &str, dev: &str) -> Result<()> {
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

/// Windows：放行虚拟网卡接口的入站流量与 ICMPv4。
///
/// Windows Defender 防火墙默认阻止**入站** ICMP 回显请求与其他入站连接，
/// 导致对端（房主/访客）ping 或访问本机失败。会话以管理员权限运行时
/// 自动添加两条规则：
/// - `frp-sh LAN mesh`：放行该接口（frp1）的所有入站流量
/// - `frp-sh ICMPv4-in`：放行 ICMPv4 入站（兜底，接口重建后仍生效）
///
/// 幂等：先删除同名旧规则再添加。
#[cfg(target_os = "windows")]
pub fn allow_firewall(iface: &str) -> Result<()> {
    let script = format!(
        "Remove-NetFirewallRule -DisplayName 'frp-sh LAN mesh' -ErrorAction SilentlyContinue; \
         New-NetFirewallRule -DisplayName 'frp-sh LAN mesh' -Direction Inbound -Action Allow -InterfaceAlias '{iface}' -Profile Any | Out-Null; \
         Remove-NetFirewallRule -DisplayName 'frp-sh ICMPv4-in' -ErrorAction SilentlyContinue; \
         New-NetFirewallRule -DisplayName 'frp-sh ICMPv4-in' -Direction Inbound -Action Allow -Protocol ICMPv4 -Profile Any | Out-Null"
    );
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
pub fn allow_firewall(_iface: &str) -> Result<()> {
    Ok(())
}

/// 在 TUN 设备与数据通道之间双向转发 IP 包，直到会话结束。
pub async fn run<TR>(mut transport: TR, mut dev: tun::AsyncDevice) -> Result<()>
where
    TR: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut pkt = [0u8; 65536];
    let mut acc: Vec<u8> = Vec::new();
    let mut rbuf = [0u8; 16384];
    loop {
        tokio::select! {
            // biased + 单次 read：未选中的分支要么未轮询要么处于 pending（未消费数据），不会丢包
            biased;
            r = dev.read(&mut pkt) => {
                match r {
                    Ok(0) | Err(_) => break, // 设备关闭
                    Ok(n) => {
                        crate::tunnel::write_frame(&mut transport, &pkt[..n])
                            .await
                            .map_err(FrpError::Io)?;
                    }
                }
            }
            n = transport.read(&mut rbuf) => {
                let n = match n {
                    Ok(0) | Err(_) => break, // 对端关闭会话
                    Ok(n) => n,
                };
                acc.extend_from_slice(&rbuf[..n]);
                loop {
                    match crate::tunnel::take_frame(&mut acc) {
                        Ok(Some(f)) if f.is_empty() => return Ok(()),
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
    Ok(())
}

// ---------- 网格（多访客）数据平面 ----------

/// 网格数据平面：TUN 与多个对端流之间按目标 IP 路由。
///
/// - TUN 读到的 IP 包 → 按目标 IP 最长前缀匹配 → 对应对端写通道（无匹配则丢弃）
/// - 对端流读到的 IP 包 → 目标为其他对端 → 直接转发；否则写入 TUN（交给本机网络栈）
pub struct MeshPlane {
    /// uuid → 路由表 (网络地址, 前缀)，含访客虚拟 IP(/32) 与暴露的局域网子网
    routes: std::sync::RwLock<std::collections::HashMap<String, Vec<(u32, u8)>>>,
    /// uuid → 对端写通道
    writers: std::sync::RwLock<
        std::collections::HashMap<String, tokio::sync::mpsc::UnboundedSender<Vec<u8>>>,
    >,
    /// uuid → 关闭通知（unregister 时唤醒 reader 退出）
    closes: std::sync::RwLock<std::collections::HashMap<String, Arc<tokio::sync::Notify>>>,
    /// 对端死亡通知（reader 结束时发送 uuid）
    dead_tx: tokio::sync::mpsc::UnboundedSender<String>,
}

impl MeshPlane {
    pub fn new() -> (Arc<Self>, tokio::sync::mpsc::UnboundedReceiver<String>) {
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
        dispatch_tx: tokio::sync::mpsc::UnboundedSender<(String, Vec<u8>)>,
        stats: Option<Arc<crate::stats::StreamStats>>,
    ) {
        self.unregister(uuid); // 清理旧链路（如中继 → 直连切换）
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let close = Arc::new(tokio::sync::Notify::new());
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
            loop {
                tokio::select! {
                    _ = close_r.notified() => break,
                    r = rd.read(&mut rbuf) => {
                        match r {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                acc.extend_from_slice(&rbuf[..n]);
                                loop {
                                    match crate::tunnel::take_frame(&mut acc) {
                                        Ok(Some(f)) if f.is_empty() => { let _ = dt.send((uuid_r.clone(), Vec::new())); }
                                        // 心跳：FRPING → 原路回 FRPONG；FRPONG → 记录 RTT
                                        Ok(Some(f)) if f == b"FRPING" => {
                                            let _ = tx_pong.send(b"FRPONG".to_vec());
                                        }
                                        Ok(Some(f)) if f == b"FRPONG" => {
                                            if let Some(st) = &st_r {
                                                let sent = ping_at_r.lock().unwrap().take();
                                                if let Some(t) = sent {
                                                    st.on_rtt(t.elapsed().as_micros() as u32);
                                                }
                                            }
                                        }
                                        Ok(Some(f)) => { let _ = dt.send((uuid_r.clone(), f)); }
                                        Ok(None) => break,
                                        Err(_) => break,
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let _ = dead_tx.send(uuid_r);
        });
        // writer：发包 → 帧封装 → 对端流（tx 被移除/丢弃时 rx 关闭 → 退出）；
        // 有统计句柄时每 3s 发送一次 FRPING 心跳（测量经中继的端到端 RTT）
        tokio::spawn(async move {
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
        });
    }

    /// 移除一个对端链路（停止其读写任务）。
    pub fn unregister(&self, uuid: &str) {
        self.routes.write().unwrap().remove(uuid);
        self.writers.write().unwrap().remove(uuid);
        if let Some(c) = self.closes.write().unwrap().remove(uuid) {
            c.notify_one();
        }
    }

    /// 为 IP 包选择目标链路（最长前缀匹配；无匹配返回 None）。
    fn route_for(&self, pkt: &[u8]) -> Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>> {
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
    mut dev: tun::AsyncDevice,
    plane: Arc<MeshPlane>,
    mut dispatch_rx: tokio::sync::mpsc::UnboundedReceiver<(String, Vec<u8>)>,
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
                            let _ = tx.send(pkt[..n].to_vec());
                        }
                    }
                }
            }
            msg = dispatch_rx.recv() => {
                match msg {
                    Some((_, f)) if f.is_empty() => { /* 对端结束会话，等 reader 报死亡 */ }
                    Some((_, f)) => {
                        // 目标为其他对端 → 直转；否则交给本机网络栈
                        if let Some(tx) = plane.route_for(&f) {
                            let _ = tx.send(f);
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

/// 尝试开启系统 IPv4 转发（房主侧：让访客能经隧道访问房主局域网）。
///
/// 需要 root/管理员权限；失败返回 `false`（调用方打印提示）。
pub fn enable_ip_forward() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("sysctl")
            .args(["-w", "net.ipv4.ip_forward=1"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sysctl")
            .args(["-w", "net.inet.ip.forwarding=1"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("netsh")
            .args(["routing", "ip", "set", "ipforwarding", "enable"])
            .output();
        // netsh 返回码不可靠，交给上层按"可能未生效"提示
        false
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        false
    }
}

/// 为访客添加一条经 TUN 设备访问房主局域网的路由（需 root/管理员）。
///
/// `cidr` 形如 `192.168.1.0/24`；`gateway` 为房主虚拟 IP（10.66.0.1）。
pub fn add_route(cidr: &str, dev_name: &str, gateway: &str) -> Result<()> {
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
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|e| {
            FrpError::Tun(format!(
                "{program} failed (requires root/administrator privileges): {e}"
            ))
        })?;
    if out.status.success() {
        Ok(())
    } else {
        Err(FrpError::Tun(format!(
            "{program} returned an error: {}",
            String::from_utf8_lossy(&out.stderr).trim()
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
        let (tx_a, _rx_a) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (tx_b, mut rx_b) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        plane.writers.write().unwrap().insert("A".into(), tx_a);
        plane.writers.write().unwrap().insert("B".into(), tx_b);

        // 精确命中 /32（最长前缀优先 → A）
        let r = plane.route_for(&pkt([10, 66, 0, 5]));
        assert!(r.is_some());
        let _ = r.unwrap().send(pkt([1, 2, 3, 4]));
        // 走 /24 → B
        let r = plane.route_for(&pkt([10, 66, 0, 9]));
        assert!(r.is_some());
        let _ = r.unwrap().send(pkt([5, 6, 7, 8]));
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
