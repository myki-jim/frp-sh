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
            "；Windows 需要 wintun.dll 放在可执行文件旁（或用 WINTUN_DLL 环境变量指定），且需管理员权限"
        } else {
            ""
        };
        FrpError::Tun(format!("创建 TUN 设备失败{hint}: {e}"))
    })
}

/// 获取 TUN 设备的实际系统名称（macOS 为 `utunN`，Linux 为自定义名）。
pub fn device_name(dev: &tun::AsyncDevice) -> Option<String> {
    use tun::AbstractDevice;
    dev.tun_name().ok()
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
                            return Err(FrpError::Protocol("隧道帧超限".into()));
                        }
                    }
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
            "当前平台不支持自动添加路由，请手动执行: ip route add {cidr} dev {dev_name}"
        )))
    }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
fn run_cmd(program: &str, args: &[&str]) -> Result<()> {
    let out = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|e| FrpError::Tun(format!("{program} 执行失败（需要 root/管理员权限）: {e}")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(FrpError::Tun(format!(
            "{program} 返回错误: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )))
    }
}
