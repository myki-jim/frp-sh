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
pub fn create(cfg: &TunConfig) -> Result<tun::AsyncDevice> {
    let mut c = tun::configure();
    c.tun_name(&cfg.name)
        .address(&cfg.ip)
        .netmask(&cfg.netmask)
        .mtu(cfg.mtu)
        .up();
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
    tun::create_as_async(&c)
        .map_err(|e| FrpError::Tun(format!("创建 TUN 设备失败（需要 root/管理员权限）: {e}")))
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
