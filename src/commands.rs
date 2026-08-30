//! 命令编排：serve / game create / game join 的核心流程。

use crate::config::Config;
use crate::error::{FrpError, Result};
use crate::p2p::hole_punch::{
    parse_ack, parse_punch, punch_targets_multi, PunchEngine, PunchOutcome,
};
use crate::p2p::relay::{self, RelayRole};
use crate::p2p::stream;
use crate::room::state::{Role, RoomState};
use crate::signaling::server;
use crate::signaling::SignalingClient;
use crate::tunnel;
use crate::utils;
use colored::Colorize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, UdpSocket};

/// 打洞窗口时长（超时后转入中继）
pub const PUNCH_WINDOW: Duration = Duration::from_secs(3);
/// 打洞阶段总时长上限：地址频繁变化时防止窗口被无限延长
const MAX_PUNCH_PHASE: Duration = Duration::from_secs(12);
/// 打洞发包间隔
const PUNCH_INTERVAL: Duration = Duration::from_millis(60);
/// 房主轮询房间间隔
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// 由口令派生 32 字节共享密钥（SHA-256）。
fn derive_key(passphrase: Option<&str>) -> Option<[u8; 32]> {
    passphrase.map(|p| {
        let mut h = Sha256::new();
        h.update(p.as_bytes());
        h.finalize().into()
    })
}

// ---------- 数据面：端口转发 或 虚拟网卡 ----------

/// 虚拟网卡（TUN）参数。
#[derive(Debug, Clone)]
pub struct TunOpts {
    pub ip: String,
    pub netmask: String,
    pub mtu: u16,
    /// 访客侧：需经 TUN 路由的房主局域网子网（如 `192.168.1.0/24`）。
    /// 房主侧为空。
    pub lan_routes: Vec<String>,
}

impl TunOpts {
    pub fn host_default() -> Self {
        Self {
            ip: "10.66.0.1".into(),
            netmask: "255.255.255.0".into(),
            mtu: 1400,
            lan_routes: Vec::new(),
        }
    }

    pub fn guest_default() -> Self {
        Self {
            ip: "10.66.0.2".into(),
            netmask: "255.255.255.0".into(),
            mtu: 1400,
            lan_routes: Vec::new(),
        }
    }
}

/// 端口转发模式的目标。
#[derive(Debug, Clone, Copy)]
enum ForwardMode {
    Host { service: SocketAddr, max_conns: u64 },
    Guest { listen: SocketAddr, max_conns: u64 },
}

/// 重连退避：1s, 2s, 4s, 8s, ... 上限 8s（首轮 1s，恢复速度优先）。
fn reconnect_delay(attempt: u64) -> u64 {
    let exp = 1u64 << (attempt.min(4) - 1);
    exp.min(8)
}

// ---------- 输出着色 ----------

/// 成功信息（绿色）。
fn ok(msg: impl AsRef<str>) -> String {
    msg.as_ref().green().to_string()
}

/// 警告信息（黄色）。
fn warn(msg: impl AsRef<str>) -> String {
    msg.as_ref().yellow().to_string()
}

/// 错误信息（红色）。
fn err(msg: impl AsRef<str>) -> String {
    msg.as_ref().red().to_string()
}

/// 次要信息（暗色）。
fn dim(msg: impl AsRef<str>) -> String {
    msg.as_ref().dimmed().to_string()
}

// ---------- 权限处理：lan 组网需要管理员/root ----------

/// 该命令是否需要管理员/root 权限（Windows：UAC 提权；macOS/Linux：sudo）。
pub fn needs_elevation(command: &Option<crate::cli::Commands>) -> bool {
    matches!(command, Some(crate::cli::Commands::Lan { .. }))
}

/// 当前进程是否已具备管理员/root 权限。
pub fn is_elevated() -> bool {
    #[cfg(target_os = "windows")]
    {
        // IsUserAnAdmin：判断当前进程是否提升（UAC 提权后返回 true）
        unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() != 0 }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // 非 Windows：检查 euid 是否为 0（root）
        std::process::Command::new("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
            .unwrap_or(false)
    }
}

/// 需要提权但当前未提权时的处理：
///
/// - Windows：通过 UAC 以管理员身份重启自身（用户点一次"是"即可），
///   原进程等待子进程结束后透传退出码，返回 `true`。
/// - macOS/Linux：打印 `sudo` 提示，返回 `false`（调用方应退出）。
pub fn handle_elevation(_command: &Option<crate::cli::Commands>) -> anyhow::Result<bool> {
    #[cfg(target_os = "windows")]
    {
        relaunch_elevated()
    }
    #[cfg(not(target_os = "windows"))]
    {
        println!(
            "\nLAN mesh mode requires root privileges (to create a virtual NIC).\nPlease run with sudo, e.g.:\n  sudo frp-sh lan create\n"
        );
        Ok(false)
    }
}

/// Windows：以管理员身份（UAC）重启自身并等待其退出。
#[cfg(target_os = "windows")]
fn relaunch_elevated() -> anyhow::Result<bool> {
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };

    let exe = std::env::current_exe()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmdline = args.join(" ");
    let cwd = std::env::current_dir()?;

    let mut file: Vec<u16> = exe.to_string_lossy().encode_utf16().collect();
    file.push(0);
    let mut params: Vec<u16> = cmdline.encode_utf16().collect();
    params.push(0);
    let mut verb: Vec<u16> = "runas".encode_utf16().collect();
    verb.push(0);
    let mut dir: Vec<u16> = cwd.to_string_lossy().encode_utf16().collect();
    dir.push(0);

    let mut si: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    si.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    si.fMask = SEE_MASK_NOCLOSEPROCESS;
    si.lpVerb = verb.as_ptr();
    si.lpFile = file.as_ptr();
    si.lpParameters = if cmdline.is_empty() {
        std::ptr::null()
    } else {
        params.as_ptr()
    };
    si.lpDirectory = dir.as_ptr();
    si.nShow = 1; // SW_SHOWNORMAL

    println!("Administrator privileges required, requesting UAC elevation (click \"Yes\" in the popup) ...");
    let launched = unsafe { ShellExecuteExW(&mut si) };
    if launched == 0 || si.hProcess.is_null() {
        eprintln!("elevation failed: possibly dismissed the UAC prompt. Please right-click \"Run as administrator\" and retry.");
        return Ok(false);
    }
    unsafe {
        WaitForSingleObject(si.hProcess, INFINITE);
        let mut code: u32 = 0;
        GetExitCodeProcess(si.hProcess, &mut code);
        std::process::exit(code as i32);
    }
    #[allow(unreachable_code)]
    Ok(true)
}

/// 打印直连建立信息，并区分本地局域网直连（同 WiFi/网线）与公网打洞直连。
fn announce_direct(peer: SocketAddr) {
    if utils::is_local_ip(peer.ip()) {
        println!(">>> {} with {peer}", ok("LAN direct"));
    } else {
        println!(">>> {} with {peer}", ok("P2P direct link established"));
    }
}

/// 数据面分发：`--tun` 时走虚拟网卡，否则走 TCP 端口转发。
///
/// `guest_subnets`：访客 `--expose-lan` 通告的局域网子网（房主侧据此加路由，访问访客局域网）。
async fn run_data_plane(
    transport: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    tun: Option<&TunOpts>,
    mode: ForwardMode,
    guest_subnets: &[String],
) -> anyhow::Result<()> {
    if let Some(o) = tun {
        // 设备名按角色区分（双端同机时不冲突）
        let dev_name = match mode {
            ForwardMode::Host { .. } => "frp0",
            ForwardMode::Guest { .. } => "frp1",
        };
        let dev = crate::p2p::tun::create(&crate::p2p::tun::TunConfig {
            name: dev_name.into(),
            ip: o.ip.clone(),
            netmask: o.netmask.clone(),
            mtu: o.mtu,
        })?;
        // 实际设备名（macOS 为系统分配的 utunN；Linux 为自定义名；Windows 为 wintun 适配器名）
        let real_name = crate::p2p::tun::device_name(&dev).unwrap_or_else(|| dev_name.to_string());
        println!(
            ">>> virtual NIC mode: IP {} / {} (MTU {}, device {real_name})",
            o.ip, o.netmask, o.mtu
        );
        // Windows：放行虚拟网卡入站流量（否则对端 ping / 访问本机被防火墙拦截）
        match crate::p2p::tun::allow_firewall(&real_name) {
            Ok(()) => {
                println!("    firewall opened for {real_name} (peer can ping/access this host)")
            }
            Err(e) => {
                println!("    firewall rule failed (peer may not ping/access this host): {e}")
            }
        }
        // macOS：utun 点对点，需显式添加虚拟网段路由，否则对端回包路由失败（ping 不通）
        if let Some(cidr) = utils::cidr_from_ip_netmask(&o.ip, &o.netmask) {
            match crate::p2p::tun::add_subnet_route(&cidr, &real_name) {
                Ok(()) => println!("    subnet route {cidr} → {real_name} added"),
                Err(e) => println!("    subnet route add failed: {e}"),
            }
        }
        println!(
            "    peer can now access this virtual subnet (e.g. ping {})",
            o.ip
        );
        match mode {
            ForwardMode::Host { .. } => {
                // 房主：允许内核转发；若访客 --expose-lan，为其局域网子网加路由
                if !guest_subnets.is_empty() {
                    let fwd = crate::p2p::tun::enable_ip_forward();
                    for cidr in guest_subnets {
                        match crate::p2p::tun::add_route(cidr, &real_name, &o.ip) {
                            Ok(()) => {
                                println!("    route {cidr} → {real_name} added (access guest LAN)")
                            }
                            Err(e) => println!("    route {cidr} add failed: {e}"),
                        }
                    }
                    if fwd {
                        println!(
                            "    {}",
                            ok("IPv4 forwarding enabled → guest LAN reachable")
                        );
                    } else {
                        println!(
                            "    hint: enable IPv4 forwarding to reach the guest LAN\n      Linux: sysctl -w net.ipv4.ip_forward=1"
                        );
                    }
                }
            }
            ForwardMode::Guest { .. } => {
                // 访客：为房主 --expose-lan 通告的局域网子网添加经 TUN 的路由
                for cidr in &o.lan_routes {
                    match crate::p2p::tun::add_route(cidr, &real_name, &o.ip) {
                        Ok(()) => {
                            println!("    route {cidr} → {real_name} added (access host LAN)")
                        }
                        Err(e) => println!("    route {cidr} add failed: {e}"),
                    }
                }
                if !o.lan_routes.is_empty() {
                    println!("    host LAN is now reachable (e.g. ping a device in the host LAN)");
                }
            }
        }
        crate::p2p::tun::run(transport, dev).await?;
        return Ok(());
    }
    match mode {
        ForwardMode::Host { service, max_conns } => {
            tunnel::host_forward(transport, service, max_conns).await?
        }
        ForwardMode::Guest { listen, max_conns } => {
            tunnel::guest_forward(transport, listen, max_conns).await?
        }
    }
    Ok(())
}

// ---------- serve ----------

/// `frp-sh serve`：同时提供 HTTP REST、UDP 公网探测与 TCP 中继。
///
/// `udp_addr`：可选独立 UDP 探测端口（云防火墙无法同端口开 TCP+UDP 时使用）。
pub async fn run_serve(
    http_addr: String,
    relay_addr: String,
    udp_addr: Option<String>,
    password: Option<String>,
) -> anyhow::Result<()> {
    let http_listener = TcpListener::bind(&http_addr).await?;
    let http_sock: SocketAddr = http_listener.local_addr()?;
    let udp = UdpSocket::bind(udp_addr.as_deref().unwrap_or(&http_addr)).await?;
    let relay_listener = TcpListener::bind(&relay_addr).await?;
    let relay_sock: SocketAddr = relay_listener.local_addr()?;

    println!("frp-sh signaling server");
    println!("  HTTP REST : {http_sock}");
    println!("  UDP echo  : {}", udp.local_addr()?);
    println!("  TCP relay : {relay_sock}");
    match &password {
        Some(_) => println!("  Auth       : enabled (--password) — clients must set the same password; relay traffic encrypted"),
        None => println!("  Auth       : disabled (no password)"),
    }
    println!("  (Ctrl-C to stop)");

    let state = server::new_state();
    let http_task = tokio::spawn(server::run_http(
        http_listener,
        state.clone(),
        password.clone(),
    ));
    let udp_task = tokio::spawn(server::run_udp_echo(udp));
    let relay_task = tokio::spawn(server::run_relay(relay_listener, state, password));

    tokio::select! {
        _ = tokio::signal::ctrl_c() => { println!("\nshutting down ..."); }
        r = http_task => { r??; }
        r = udp_task => { r??; }
        r = relay_task => { r??; }
    }
    Ok(())
}

// ---------- config ----------

/// 读取一行标准输入（EOF 返回空串）。
fn read_line() -> String {
    let mut line = String::new();
    use std::io::BufRead;
    let n = std::io::stdin().lock().read_line(&mut line).unwrap_or(0);
    if n == 0 {
        return String::new();
    }
    line.trim().to_string()
}

/// 规范化信令地址：空输入用默认值；无 scheme 时补 http://。
fn normalize_signaling(input: &str) -> anyhow::Result<String> {
    let s = input.trim();
    if s.is_empty() {
        return Ok("http://127.0.0.1:8080".to_string());
    }
    let with_scheme = if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("http://{s}")
    };
    reqwest::Url::parse(&with_scheme).map_err(|e| {
        anyhow::anyhow!("invalid address format (example: 101.43.41.195:8080): {e}")
    })?;
    Ok(with_scheme)
}

/// `frp-sh config`：交互式配置信令服务器（首次运行向导）。
///
/// `save_path`：显式保存路径（`--config` 指定）；否则保存到平台默认路径。
pub async fn run_config(save_path: Option<PathBuf>) -> anyhow::Result<()> {
    let mut out = std::io::stdout();

    println!("\n  frp-sh configuration wizard");
    println!("  ==================");
    println!("  Enter signaling server info (press Enter for defaults):\n");

    // 1. 信令服务器地址
    print!("  1. Signaling server address (HTTP) [default http://127.0.0.1:8080]\n  > ");
    out.flush()?;
    let signaling_input = read_line();
    let signaling_addr = normalize_signaling(&signaling_input)?;

    // 2. 中继地址（默认由信令地址推导：同主机 + 8081）
    let host = reqwest::Url::parse(&signaling_addr)?
        .host_str()
        .unwrap_or("127.0.0.1")
        .to_string();
    let default_relay = format!("{host}:8081");
    print!("  2. Relay server address (TCP) [default {default_relay}]\n  > ");
    out.flush()?;
    let relay_input = read_line();
    let relay_addr = if relay_input.trim().is_empty() {
        default_relay
    } else {
        relay_input.trim().to_string()
    };

    // 3. UDP 探测独立端口（可选）
    print!("  3. Use a separate port for UDP probing? (y/N) ");
    out.flush()?;
    let udp_input = read_line();
    let want_udp = matches!(udp_input.trim().to_lowercase().as_str(), "y" | "yes");
    let signaling_udp = if want_udp {
        print!("     Separate UDP probe address (e.g. {host}:8082):\n  > ");
        out.flush()?;
        let udp_addr = read_line().trim().to_string();
        if udp_addr.is_empty() {
            None
        } else {
            Some(udp_addr)
        }
    } else {
        None
    };

    // 4. 服务器密码（可选）
    print!("  4. Server password (optional, if the server runs with --password):\n  > ");
    out.flush()?;
    let pw_input = read_line();
    let password = if pw_input.trim().is_empty() {
        None
    } else {
        Some(pw_input.trim().to_string())
    };

    let cfg = Config {
        signaling_addr,
        relay_addr,
        signaling_udp,
        uuid: None, // 设备 UUID 存于独立 identity 文件
        password,
    };

    // 保存
    let path = match save_path {
        Some(p) => p,
        None => Config::default_path().ok_or_else(|| {
            anyhow::anyhow!("cannot determine default config directory (APPDATA/HOME not set)")
        })?,
    };
    cfg.save(&path)?;
    println!("\n  {}: {}", ok("config saved"), path.display());
    println!("    signaling server: {}", cfg.signaling_addr);
    println!("    relay server: {}", cfg.relay_addr);
    if let Some(u) = &cfg.signaling_udp {
        println!("    UDP probe   : {u}");
    }
    if cfg.password.is_some() {
        println!("    password    : configured (requests authenticated, relay encrypted)");
    }

    // 软连通性检查（不阻塞）
    println!("\n  checking server connectivity ...");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()?;
    match client
        .get(format!("{}/health", cfg.signaling_addr))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            println!("  {} signaling server reachable", ok("[OK]"));
        }
        Ok(_) => println!("  {} server returned an abnormal status", warn("[WARN]")),
        Err(_) => println!(
            "  {} cannot reach the signaling server (check the address and firewall; run frp-sh config again later)",
            warn("[WARN]")
        ),
    }

    println!("\n  next steps:");
    println!("    frp-sh game create             # host: create a room");
    println!("    frp-sh game join game-xxxxxx   # guest: join a room");
    println!("  re-configure: frp-sh config\n");
    Ok(())
}

/// 版本冲突控制：校验与信令服务器的线协议版本一致，并显示服务器版本。
///
/// - 服务器协议不一致 → 红色错误，拒绝运行
/// - 服务器版本与本地不同但协议兼容 → 黄色提示
/// - 连接不上服务器 → 红色错误
/// - 旧服务器无 `/version`（404）→ 视为协议 1，向后兼容
async fn check_server_protocol(signaling: &SignalingClient) -> anyhow::Result<()> {
    match signaling.get_version().await {
        Ok(Some((server_ver, server_proto, auth))) => {
            if server_proto != crate::version::PROTOCOL_VERSION {
                return Err(anyhow::anyhow!(
                    "{} {}",
                    err("[error]"),
                    err(format!(
                        "version conflict: server v{server_ver} (protocol v{server_proto}), \
                         local v{} (protocol v{}). Please upgrade frp-sh or the server.",
                        crate::version::VERSION,
                        crate::version::PROTOCOL_VERSION
                    ))
                ));
            }
            if server_ver != crate::version::VERSION {
                println!(
                    "  {} server v{server_ver} differs from local v{} ({} compatible)",
                    warn("[warn]"),
                    crate::version::VERSION,
                    dim("protocol")
                );
            } else {
                println!(
                    "  Server version: v{server_ver} ({})",
                    dim(format!("protocol v{server_proto}"))
                );
            }
            if auth {
                if signaling.has_password() {
                    println!(
                        "  {} server requires a password — {}",
                        ok("[OK]"),
                        dim("authenticated")
                    );
                } else {
                    return Err(anyhow::anyhow!(
                        "{} {}",
                        err("[error]"),
                        err(
                            "server requires a password (configure 'password' in config, or run `frp-sh config`)"
                        )
                    ));
                }
            }
        }
        Ok(None) => {
            println!(
                "  Server version: {} ({})",
                dim("legacy"),
                dim("protocol v1 assumed")
            );
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "{} {}",
                err("[error]"),
                err(format!("cannot reach signaling server: {e}"))
            ));
        }
    }
    Ok(())
}

/// 显示房主版本（访客加入后），版本不同但兼容标黄，可能冲突标红。
fn show_host_version(host_version: &str) {
    if host_version.is_empty() {
        return;
    }
    let local = crate::version::VERSION;
    if host_version == local {
        println!("  Host version  : v{host_version}");
    } else if crate::version::is_breaking_gap(local, host_version) {
        println!(
            "  {} host v{host_version} vs local v{local} ({})",
            warn("[warn]"),
            err("possible conflict, please align versions")
        );
    } else {
        println!(
            "  {} host v{host_version} vs local v{local} ({})",
            warn("[warn]"),
            dim("compatible")
        );
    }
}

// ---------- game create ----------

/// 中继连接的认证信息：服务器是否启用密码（决定加密）+ 本机配置的密码 token。
async fn relay_auth_info(signaling: &SignalingClient, cfg: &Config) -> (bool, Option<String>) {
    let auth = signaling
        .get_version()
        .await
        .ok()
        .flatten()
        .map(|(_, _, a)| a)
        .unwrap_or(false);
    let token = cfg.password.clone();
    (auth, token)
}

/// `game create`：注册房间，进入房主会话；返回房间号。
///
/// - `guest_ips`：房主预留的访客虚拟 IP 池（lan 系列 `--guest-ips`；转发系列为空）
/// - `expose_lan`：是否把本机局域网接入隧道（lan 系列 `--expose-lan`；默认不暴露）
#[allow(clippy::too_many_arguments)]
pub async fn run_create(
    cfg: Config,
    prefix: String,
    ttl: u64,
    service: String,
    force_relay: bool,
    key: Option<String>,
    max_conns: u64,
    spread: u32,
    tun: Option<TunOpts>,
    guest_ips: Vec<String>,
    expose_lan: bool,
) -> anyhow::Result<String> {
    let signaling =
        SignalingClient::new_with_password(&cfg.signaling_addr, cfg.password.as_deref());
    // 版本冲突控制：与信令服务器的线协议必须一致（不一致拒绝运行）
    check_server_protocol(&signaling).await?;
    let engine = PunchEngine::bind().await?;
    let token = utils::rand_token();
    let my_ext = signaling
        .learn_public_addr(engine.socket(), cfg.signaling_udp_addr()?, &token)
        .await?;
    log::info!("public address: {my_ext}");
    let tun_ip = tun.as_ref().map(|t| t.ip.clone());
    let lan_addrs = utils::lan_socket_addrs(engine.local_addr()?.port());
    // 默认不暴露本地局域网；仅 --expose-lan 时通告子网
    let lan_subnets = if tun.is_some() && expose_lan {
        utils::lan_subnet_cidrs()
    } else {
        Vec::new()
    };
    let resp = signaling
        .create_room(
            &prefix,
            ttl,
            my_ext,
            tun_ip,
            lan_addrs.clone(),
            lan_subnets.clone(),
            guest_ips.clone(),
            crate::version::VERSION.to_string(),
        )
        .await?;
    let room_id = resp.room_id.clone();
    println!("\n  {} : {room_id}", ok("Room created"));
    println!("  Signaling    : {}", cfg.signaling_addr);
    if let Some(u) = &cfg.uuid {
        println!("  Your ID      : {u}");
    }
    let lan_list: Vec<String> = lan_addrs.iter().map(|a| a.to_string()).collect();
    if !lan_list.is_empty() {
        println!("  LAN addrs    : {}", lan_list.join(", "));
    }
    if let Some(t) = &tun {
        println!(
            "  Vnet IP      : {} (peer can ping/connect directly to this IP)",
            t.ip
        );
        println!("  Mode         : LAN mesh (virtual NIC)");
        if !lan_subnets.is_empty() {
            println!(
                "  LAN subnets  : {} (--expose-lan: accessible by guest)",
                lan_subnets.join(", ")
            );
        }
        if !guest_ips.is_empty() {
            println!(
                "  Guest IPs    : {} (assigned in join order)",
                guest_ips.join(", ")
            );
        }
    } else {
        println!("  Local service: {service}");
        println!("  Mode         : port forwarding");
    }
    if key.is_some() {
        println!("  Encryption   : on (--key)");
    }
    println!("  Waiting for a guest to join ...\n");

    let session = host_session(
        &cfg,
        &room_id,
        service,
        force_relay,
        key,
        max_conns,
        spread,
        tun,
        None, // 无限重连
        expose_lan,
    );
    tokio::select! {
        r = session => r?,
        _ = tokio::signal::ctrl_c() => {
            println!("\nstopped by user");
            let _ = signaling.delete_room(&room_id).await;
        }
    }
    Ok(room_id)
}

/// 房主会话（房间已创建）：打洞监听（或强制中继），随后进入数据面（转发或虚拟网卡）。
///
/// 断线后自动重连（重新绑定 socket、刷新公网地址）。`max_rounds` 仅用于测试：
/// 限制重连轮数，`None` 表示无限重连直到 Ctrl-C 或房间失效。
#[allow(clippy::too_many_arguments)]
pub async fn host_session(
    cfg: &Config,
    room_id: &str,
    service: String,
    force_relay: bool,
    key: Option<String>,
    max_conns: u64,
    spread: u32,
    tun: Option<TunOpts>,
    max_rounds: Option<u64>,
    expose_lan: bool,
) -> anyhow::Result<()> {
    let signaling =
        SignalingClient::new_with_password(&cfg.signaling_addr, cfg.password.as_deref());
    let service_addr: SocketAddr = service
        .parse()
        .map_err(|e| FrpError::Config(format!("bad service addr {service}: {e}")))?;
    let mut state = RoomState::new(room_id.to_string(), Role::Host, service_addr);
    let key_bytes = derive_key(key.as_deref());
    let mut attempt: u64 = 0;

    // lan 模式（虚拟网卡）→ 网格会话：多访客全互联
    if let Some(t) = &tun {
        return host_mesh_session(
            cfg,
            room_id,
            force_relay,
            key,
            spread,
            Some(t.clone()),
            expose_lan,
            max_rounds,
        )
        .await;
    }

    loop {
        attempt += 1;
        if attempt > 1 {
            let wait = reconnect_delay(attempt);
            println!(
                "\n>>> {} ...",
                warn(format!(
                    "connection lost, reconnecting in {wait} seconds (Ctrl-C to exit)"
                ))
            );
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
        // 每次（重）连接绑定新 socket 并刷新公网地址（NAT 映射可能已过期）
        let engine = PunchEngine::bind().await?;
        let token = utils::rand_token();
        let my_ext = signaling
            .learn_public_addr(engine.socket(), cfg.signaling_udp_addr()?, &token)
            .await?;
        log::info!("public address: {my_ext}");
        let lan_addrs = utils::lan_socket_addrs(engine.local_addr()?.port());
        // 默认不暴露本地局域网；仅 --expose-lan 时通告子网
        let lan_subnets = if tun.is_some() && expose_lan {
            utils::lan_subnet_cidrs()
        } else {
            Vec::new()
        };
        if let Err(e) = signaling
            .refresh_room(room_id, my_ext, lan_addrs, lan_subnets)
            .await
        {
            // 房间已失效（过期/被删）→ 无法继续，结束
            return Err(e.into());
        }

        let outcome =
            match host_punch_phase(&engine, &signaling, room_id, &token, force_relay, spread).await
            {
                Ok(o) => o,
                Err(e) => {
                    log::warn!("punch phase error: {e}");
                    continue;
                }
            };
        // 访客若 --expose-lan，取其通告的局域网子网（房主据此加路由访问访客局域网）
        let guest_subnets: Vec<String> = signaling
            .get_room(room_id)
            .await
            .ok()
            .map(|i| i.guest_subnets)
            .unwrap_or_default();
        let run_result = match outcome {
            PunchOutcome::Direct { peer, first } => {
                state.peer_addr = Some(peer);
                announce_direct(peer);
                let stream = engine.into_stream(peer, first, key_bytes);
                run_data_plane(
                    stream,
                    tun.as_ref(),
                    ForwardMode::Host {
                        service: service_addr,
                        max_conns,
                    },
                    &guest_subnets,
                )
                .await
            }
            PunchOutcome::TimedOut => {
                state.relay_mode = true;
                println!(
                    ">>> {} ...",
                    warn("UDP hole punching failed, falling back to relay")
                );
                let relay_addr = match cfg.relay_addr.parse() {
                    Ok(a) => a,
                    Err(e) => {
                        log::warn!("bad relay addr: {e}");
                        continue;
                    }
                };
                // 服务器启用密码时：中继带 token 认证 + 流量加密
                let (auth, token_opt) = relay_auth_info(&signaling, cfg).await;
                let stream = match relay::connect(
                    relay_addr,
                    room_id,
                    RelayRole::Host,
                    token_opt.as_deref(),
                    auth,
                    None, // 单对端（game/dev）：单槽位配对
                )
                .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("relay connect failed: {e}");
                        continue;
                    }
                };
                println!(">>> {} ...", warn("relay connected, waiting for guest"));
                run_data_plane(
                    stream,
                    tun.as_ref(),
                    ForwardMode::Host {
                        service: service_addr,
                        max_conns,
                    },
                    &guest_subnets,
                )
                .await
            }
        };
        if let Err(e) = run_result {
            log::warn!("session ended abnormally: {e}");
        } else {
            println!(">>> session ended, preparing to reconnect ...");
        }
        if let Some(n) = max_rounds {
            if attempt >= n {
                return Ok(());
            }
        }
        // 循环 → 自动重连
    }
}

/// 房主打洞阶段：轮询房间发现访客，同时监听 PUNCH / ACK / 数据帧。
///
/// 访客注册后同时向访客的公网地址与局域网地址打洞（同局域网场景秒连）。
async fn host_punch_phase(
    engine: &PunchEngine,
    signaling: &SignalingClient,
    room_id: &str,
    token: &str,
    force_relay: bool,
    spread: u32,
) -> Result<PunchOutcome> {
    let mut guest_addrs: Vec<SocketAddr> = Vec::new();
    let mut deadline: Option<Instant> = None;
    let mut last_poll = Instant::now() - POLL_INTERVAL;
    let mut last_punch = Instant::now() - PUNCH_INTERVAL;
    let my_local = engine.local_addr().ok();
    let my_port = my_local.map(|a| a.port());
    // 打洞阶段开始时间（首个访客出现时）——防止地址频繁变化导致窗口无限延长
    let mut first_guest: Option<Instant> = None;
    loop {
        let now = Instant::now();
        // 1) 持续轮询房间获取访客最新地址：重连场景下访客重注册后地址会变，
        //    一直用旧地址会打到死端口；发现新地址时重开打洞窗口
        if now.duration_since(last_poll) >= POLL_INTERVAL {
            last_poll = now;
            if let Ok(info) = signaling.get_room(room_id).await {
                let mut addrs = Vec::new();
                if let Some(g) = info.guest_addr {
                    addrs.push(g);
                    addrs.extend(info.guest_lan);
                }
                if !addrs.is_empty() && addrs != guest_addrs {
                    if !guest_addrs.is_empty() {
                        log::info!("guest address changed to {addrs:?}, restarting punch window");
                    }
                    guest_addrs = addrs;
                    deadline = Some(now + PUNCH_WINDOW);
                }
            }
        }
        // 2) 访客出现后启动打洞窗口（强制中继则直接转入中继，且不收发任何打洞数据报）
        if force_relay {
            if !guest_addrs.is_empty() {
                return Ok(PunchOutcome::TimedOut);
            }
            continue;
        }
        if !guest_addrs.is_empty() {
            if first_guest.is_none() {
                first_guest = Some(now);
            }
            if deadline.is_none() {
                log::info!("guest {guest_addrs:?} registered, punching ...");
                deadline = Some(now + PUNCH_WINDOW);
            }
            if let Some(d) = deadline {
                if now < d && now.duration_since(last_punch) >= PUNCH_INTERVAL {
                    for target in punch_targets_multi(&guest_addrs, spread, my_local) {
                        let _ = engine.send_punch(target, token).await;
                    }
                    last_punch = now;
                }
            }
        }
        // 3) 接收数据报（每次最多 100ms：频繁轮询房间以尽快发现访客地址变化；
        //    重连场景下旧访客地址已死，持续打洞会产生 ICMP 毒化，需及时切换目标）
        let recv_timeout = match deadline {
            Some(d) => d
                .saturating_duration_since(now)
                .min(Duration::from_millis(100)),
            None => POLL_INTERVAL,
        };
        if let Some((bytes, src)) = engine.recv(recv_timeout).await? {
            if my_port == Some(src.port()) {
                continue; // 自我打洞回声，忽略
            }
            if stream::is_data_frame(&bytes) {
                return Ok(PunchOutcome::Direct {
                    peer: src,
                    first: Some((bytes, src)),
                });
            }
            if let Some(t) = parse_punch(&bytes) {
                engine.send_ack_retry(src, &t).await;
                log::info!("got PUNCH from {src}, replying ACK");
                return Ok(PunchOutcome::Direct {
                    peer: src,
                    first: None,
                });
            }
            if let Some(t) = parse_ack(&bytes) {
                if t == token {
                    return Ok(PunchOutcome::Direct {
                        peer: src,
                        first: None,
                    });
                }
            }
        }
        // 4) 超时判定：窗口到期，或总打洞时间超过上限（地址频繁变化时兜底）
        if let Some(d) = deadline {
            let total = first_guest
                .map(|f| Instant::now().duration_since(f))
                .unwrap_or_default();
            if Instant::now() >= d || total >= MAX_PUNCH_PHASE {
                return Ok(PunchOutcome::TimedOut);
            }
        }
    }
}

// ---------- game join ----------

/// `game join`：校验房间号并进入访客会话。
///
/// `requested_ip`：访客显式指定的虚拟 IP（`--ip`）；无则 None（由 UUID 派生或服务器分配）。
#[allow(clippy::too_many_arguments)]
pub async fn run_join(
    cfg: Config,
    room_id: String,
    listen: String,
    force_relay: bool,
    key: Option<String>,
    max_conns: u64,
    spread: u32,
    tun: Option<TunOpts>,
    requested_ip: Option<String>,
    expose_lan: bool,
) -> anyhow::Result<()> {
    let signaling =
        SignalingClient::new_with_password(&cfg.signaling_addr, cfg.password.as_deref());
    // 版本冲突控制：与信令服务器的线协议必须一致（不一致拒绝运行）
    check_server_protocol(&signaling).await?;
    if !utils::validate_room_id(&room_id) {
        anyhow::bail!("invalid room id: {room_id} (expected format like game-a3f9c2)");
    }
    if let Some(u) = &cfg.uuid {
        println!("  Your ID      : {u}");
    }
    if let Some(t) = &tun {
        println!(
            "  Vnet IP      : {} (mesh virtual IP, friends can connect directly to your whole machine)",
            t.ip
        );
    }
    let session = guest_session(
        &cfg,
        &room_id,
        listen,
        force_relay,
        key,
        max_conns,
        spread,
        tun,
        None, // 无限重连
        requested_ip,
        expose_lan,
    );
    tokio::select! {
        r = session => r?,
        _ = tokio::signal::ctrl_c() => {
            println!("\nstopped by user");
        }
    }
    Ok(())
}

/// 访客会话（房间需已存在）：断线自动重连。
///
/// - `requested_ip`：访客显式指定的虚拟 IP（`--ip`），无则 None；
///   房主启用 IP 池（`--guest-ips`）且访客未指定时，服务器分配并返回
/// - `expose_lan`：是否把本机局域网接入隧道（`--expose-lan`；默认不暴露）
/// - `max_rounds` 仅用于测试：限制重连轮数，`None` 表示无限重连直到 Ctrl-C 或房间失效
#[allow(clippy::too_many_arguments)]
pub async fn guest_session(
    cfg: &Config,
    room_id: &str,
    listen: String,
    force_relay: bool,
    key: Option<String>,
    max_conns: u64,
    spread: u32,
    mut tun: Option<TunOpts>,
    max_rounds: Option<u64>,
    requested_ip: Option<String>,
    expose_lan: bool,
) -> anyhow::Result<()> {
    let signaling =
        SignalingClient::new_with_password(&cfg.signaling_addr, cfg.password.as_deref());
    let listen_addr: SocketAddr = listen
        .parse()
        .map_err(|e| FrpError::Config(format!("bad listen addr {listen}: {e}")))?;
    let key_bytes = derive_key(key.as_deref());
    let mut state = RoomState::new(room_id.to_string(), Role::Guest, listen_addr);
    let mut attempt: u64 = 0;
    let mut first = true;

    loop {
        attempt += 1;
        if !first {
            let wait = reconnect_delay(attempt);
            println!(
                "\n>>> {} ...",
                warn(format!(
                    "connection lost, reconnecting in {wait} seconds (Ctrl-C to exit)"
                ))
            );
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
        first = false;
        // 每次（重）连接绑定新 socket（旧 NAT 映射已失效）
        let engine = PunchEngine::bind().await?;

        // 查询房间（刷新房主地址；房间失效则结束）
        let info = match retry_get_room_info(&signaling, room_id).await {
            Ok(info) => info,
            Err(FrpError::RoomNotFound(_)) => {
                return Err(FrpError::RoomNotFound(room_id.to_string()).into());
            }
            Err(e) => {
                log::warn!("failed to query room: {e}");
                continue;
            }
        };
        let host_addr = info.host_addr;
        let host_tun_ip = info.tun_ip.clone();
        // 房主打洞候选：公网地址 + 局域网地址（同局域网直连）
        let mut host_addrs = vec![host_addr];
        host_addrs.extend(info.host_lan.iter().copied());

        // --tun 时：为房主局域网子网准备路由（跳过与本机子网重叠的）
        if let Some(t) = tun.as_mut() {
            let local = utils::lan_subnet_cidrs();
            let mut skipped = Vec::new();
            t.lan_routes = info
                .host_subnets
                .iter()
                .filter(|c| {
                    let overlap = local.iter().any(|l| utils::cidrs_overlap(c, l));
                    if overlap {
                        skipped.push(c.to_string());
                    }
                    !overlap
                })
                .cloned()
                .collect();
            if !skipped.is_empty() {
                println!(
                    "  {}: {}",
                    warn("skipping host subnets overlapping local subnets"),
                    skipped.join(", ")
                );
            }
        }

        // 刷新本机公网地址并登记（NAT 映射可能已过期）
        let token = utils::rand_token();
        let my_ext = match signaling
            .learn_public_addr(engine.socket(), cfg.signaling_udp_addr()?, &token)
            .await
        {
            Ok(a) => a,
            Err(e) => {
                log::warn!("public address probe failed: {e}");
                continue;
            }
        };
        let my_lan = utils::lan_socket_addrs(engine.local_addr()?.port());
        // 默认不暴露本地局域网；仅 --expose-lan 时通告子网
        let guest_subnets = if expose_lan {
            utils::lan_subnet_cidrs()
        } else {
            Vec::new()
        };
        match signaling
            .join_room(
                room_id,
                my_ext,
                my_lan,
                cfg.uuid.clone(),
                requested_ip.clone(),
                guest_subnets,
            )
            .await
        {
            Ok(join) => {
                println!("\n  Joined room : {}", ok(&join.room_id));
                println!("  Host address: {}", join.host_addr);
                if let Some(ip) = &host_tun_ip {
                    println!("  Host vnet IP: {ip}");
                }
                show_host_version(&info.host_version);
                // 服务器从房主预留 IP 池分配虚拟 IP（访客未指定 --ip 时）
                if let (Some(t), Some(ip)) = (tun.as_mut(), &join.assigned_ip) {
                    if t.ip != *ip {
                        println!("  Assigned IP  : {ip} (host-assigned)");
                        t.ip = ip.clone();
                    }
                }
                if !info.host_subnets.is_empty() {
                    println!("  Host LAN     : {}", info.host_subnets.join(", "));
                }
                if tun.is_some() {
                    println!("  Mode         : LAN mesh (virtual NIC)");
                } else {
                    println!("  Local listen: {listen}");
                    println!("  Mode         : port forwarding");
                }
                if key.is_some() {
                    println!("  Encryption   : on (--key)");
                }
                println!("  Punching through NAT ...\n");
            }
            Err(e) => {
                log::warn!("failed to join room: {e}");
                continue;
            }
        }

        let outcome = match guest_punch_phase(
            &engine,
            &signaling,
            room_id,
            &token,
            force_relay,
            spread,
        )
        .await
        {
            Ok(o) => o,
            Err(e) => {
                log::warn!("punch phase error: {e}");
                continue;
            }
        };
        let run_result = match outcome {
            PunchOutcome::Direct { peer, first } => {
                state.peer_addr = Some(peer);
                announce_direct(peer);
                let stream = engine.into_stream(peer, first, key_bytes);
                run_data_plane(
                    stream,
                    tun.as_ref(),
                    ForwardMode::Guest {
                        listen: listen_addr,
                        max_conns,
                    },
                    &[],
                )
                .await
            }
            PunchOutcome::TimedOut => {
                state.relay_mode = true;
                println!(">>> {} ...", warn("UDP hole punching failed, trying relay"));
                let relay_addr = match cfg.relay_addr.parse() {
                    Ok(a) => a,
                    Err(e) => {
                        log::warn!("bad relay addr: {e}");
                        continue;
                    }
                };
                // 服务器启用密码时：中继带 token 认证 + 流量加密
                let (auth, token_opt) = relay_auth_info(&signaling, cfg).await;
                // lan 模式（网格）中继携带本机 UUID，与房主为该访客建的中继连接按 UUID 配对
                let relay_uuid = if tun.is_some() {
                    cfg.uuid.clone()
                } else {
                    None
                };
                let stream = match relay::connect(
                    relay_addr,
                    room_id,
                    RelayRole::Guest,
                    token_opt.as_deref(),
                    auth,
                    relay_uuid.as_deref(),
                )
                .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("relay connect failed: {e}");
                        continue;
                    }
                };
                if force_relay {
                    // 强制中继：不再尝试任何打洞
                    println!(">>> {} ...", warn("relay connected, waiting for host"));
                    run_data_plane(
                        stream,
                        tun.as_ref(),
                        ForwardMode::Guest {
                            listen: listen_addr,
                            max_conns,
                        },
                        &[],
                    )
                    .await
                } else {
                    // 中继连接后再做一次直连复查（应对"房主已打通、访客 ACK 丢失"的不对称场景）
                    match final_punch_check(&engine, &signaling, room_id, &token, spread).await {
                        Ok(PunchOutcome::Direct { peer, first }) => {
                            announce_direct(peer);
                            drop(stream);
                            let stream = engine.into_stream(peer, first, key_bytes);
                            run_data_plane(
                                stream,
                                tun.as_ref(),
                                ForwardMode::Guest {
                                    listen: listen_addr,
                                    max_conns,
                                },
                                &[],
                            )
                            .await
                        }
                        _ => {
                            println!(">>> {} ...", warn("relay connected, waiting for host"));
                            run_data_plane(
                                stream,
                                tun.as_ref(),
                                ForwardMode::Guest {
                                    listen: listen_addr,
                                    max_conns,
                                },
                                &[],
                            )
                            .await
                        }
                    }
                }
            }
        };
        if let Err(e) = run_result {
            log::warn!("session ended abnormally: {e}");
        } else {
            println!(">>> session ended, preparing to reconnect ...");
        }
        if let Some(n) = max_rounds {
            if attempt >= n {
                return Ok(());
            }
        }
        // 循环 → 自动重连
    }
}

/// 查询房间信息（容忍房主刚创建时的时间差）。
async fn retry_get_room_info(
    signaling: &SignalingClient,
    room_id: &str,
) -> Result<crate::signaling::RoomInfo> {
    for attempt in 0..10 {
        match signaling.get_room(room_id).await {
            Ok(info) => return Ok(info),
            Err(FrpError::RoomNotFound(_)) if attempt < 9 => {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Err(e) => return Err(e),
        }
    }
    Err(FrpError::RoomNotFound(room_id.to_string()))
}

/// 访客打洞阶段：向房主候选地址（公网 + 局域网）持续发送 PUNCH 并等待 ACK / 数据帧。
///
/// 周期刷新房间获取房主最新地址——房主重连/刷新后地址会变，若一直用
/// 初始拿到（刷新前）的旧地址会打到死端口（e2e 偶发超时的根因）。
async fn guest_punch_phase(
    engine: &PunchEngine,
    signaling: &SignalingClient,
    room_id: &str,
    token: &str,
    force_relay: bool,
    spread: u32,
) -> Result<PunchOutcome> {
    if force_relay {
        return Ok(PunchOutcome::TimedOut);
    }
    let my_local = engine.local_addr().ok();
    let my_port = my_local.map(|a| a.port());
    let mut deadline = Instant::now() + PUNCH_WINDOW;
    let phase_start = Instant::now();
    let mut last_punch = Instant::now() - PUNCH_INTERVAL;
    let mut last_refresh = Instant::now() - POLL_INTERVAL;
    let mut targets: Vec<SocketAddr> = Vec::new();
    let mut last_targets: Vec<SocketAddr> = Vec::new();
    loop {
        let now = Instant::now();
        // 周期刷新房主地址（公网 + 局域网），避免打到旧地址；
        // 房主重连后地址变化 → 重开打洞窗口，确保能打到新地址
        if now.duration_since(last_refresh) >= POLL_INTERVAL {
            last_refresh = now;
            if let Ok(info) = signaling.get_room(room_id).await {
                let mut addrs = vec![info.host_addr];
                addrs.extend(info.host_lan.iter().copied());
                let t = punch_targets_multi(&addrs, spread, my_local);
                if !last_targets.is_empty() && t != last_targets {
                    log::info!("host address changed, restarting punch window");
                    deadline = now + PUNCH_WINDOW;
                }
                last_targets = t.clone();
                targets = t;
            }
        }
        if now < deadline && now.duration_since(last_punch) >= PUNCH_INTERVAL {
            for target in &targets {
                let _ = engine.send_punch(*target, token).await;
            }
            last_punch = now;
        }
        let recv_timeout = deadline
            .saturating_duration_since(now)
            .min(Duration::from_millis(100));
        if let Some((bytes, src)) = engine.recv(recv_timeout).await? {
            // "[dbg guest punch] recv {}B from {src}: {:?}", bytes.len(), String::from_utf8_lossy(&bytes));
            if my_port == Some(src.port()) {
                continue; // 自我打洞回声，忽略
            }
            if stream::is_data_frame(&bytes) {
                return Ok(PunchOutcome::Direct {
                    peer: src,
                    first: Some((bytes, src)),
                });
            }
            if let Some(t) = parse_punch(&bytes) {
                engine.send_ack_retry(src, &t).await;
                log::info!("got PUNCH from {src}, replying ACK");
                return Ok(PunchOutcome::Direct {
                    peer: src,
                    first: None,
                });
            }
            if let Some(t) = parse_ack(&bytes) {
                if t == token {
                    return Ok(PunchOutcome::Direct {
                        peer: src,
                        first: None,
                    });
                }
            }
        }
        // 超时判定：窗口到期，或总打洞时间超过上限（地址频繁变化时兜底）
        if Instant::now() >= deadline
            || Instant::now().duration_since(phase_start) >= MAX_PUNCH_PHASE
        {
            return Ok(PunchOutcome::TimedOut);
        }
    }
}

/// 中继连接后的最终直连复查（约 400ms）。
async fn final_punch_check(
    engine: &PunchEngine,
    signaling: &SignalingClient,
    room_id: &str,
    token: &str,
    spread: u32,
) -> Result<PunchOutcome> {
    let my_local = engine.local_addr().ok();
    let my_port = my_local.map(|a| a.port());
    // 复查前刷新一次房主地址（重连后可能已变化）
    let targets = match signaling.get_room(room_id).await {
        Ok(info) => {
            let mut addrs = vec![info.host_addr];
            addrs.extend(info.host_lan.iter().copied());
            punch_targets_multi(&addrs, spread, my_local)
        }
        Err(_) => Vec::new(),
    };
    for _ in 0..3 {
        for target in &targets {
            let _ = engine.send_punch(*target, token).await;
        }
        if let Some((bytes, src)) = engine.recv(Duration::from_millis(120)).await? {
            if my_port == Some(src.port()) {
                continue; // 自我打洞回声，忽略
            }
            if stream::is_data_frame(&bytes) {
                return Ok(PunchOutcome::Direct {
                    peer: src,
                    first: Some((bytes, src)),
                });
            }
            if let Some(t) = parse_ack(&bytes) {
                if t == token {
                    return Ok(PunchOutcome::Direct {
                        peer: src,
                        first: None,
                    });
                }
            }
        }
    }
    Ok(PunchOutcome::TimedOut)
}

// ---------- 网格模式（lan：多访客全互联，host 为枢纽） ----------

/// 访客打洞候选地址（公网 + 局域网）。
fn mesh_peer_addrs(info: &crate::signaling::GuestInfo) -> Vec<SocketAddr> {
    let mut addrs = vec![info.addr];
    addrs.extend(info.lan.iter().copied());
    addrs
}

/// 访客链路的路由表（虚拟 IP /32 + 暴露的局域网子网）。
fn mesh_routes_for(info: &crate::signaling::GuestInfo) -> Vec<(u32, u8)> {
    let mut routes = Vec::new();
    if let Some(ip) = &info.vnet_ip {
        if let Ok(a) = ip.parse::<std::net::Ipv4Addr>() {
            routes.push((u32::from(a), 32));
        }
    }
    for cidr in &info.subnets {
        if let Some((net, pfx)) = utils::parse_cidr(cidr) {
            routes.push((net, pfx));
        }
    }
    routes
}

/// 建链中的访客（host 侧）。
struct PendingHost {
    info: crate::signaling::GuestInfo,
    /// 是否已尝试中继（直连超时后）
    relay_attempted: bool,
    /// 上次尝试中继的时间（失败后 10s 重试）
    relay_at: Option<Instant>,
    /// 直连打洞窗口截止
    deadline: Option<Instant>,
}

/// host 侧网格会话（`lan create`）：虚拟网卡一次创建，重连只重建对端链路；
/// 到每个访客建立直连隧道（打洞失败转中继，按 UUID 配对），
/// 访客之间的流量由本机数据平面按目标 IP 转发（host 为枢纽，全互联）。
#[allow(clippy::too_many_arguments)]
pub async fn host_mesh_session(
    cfg: &Config,
    room_id: &str,
    force_relay: bool,
    key: Option<String>,
    spread: u32,
    tun: Option<TunOpts>,
    expose_lan: bool,
    max_guests: Option<u64>,
) -> anyhow::Result<()> {
    let signaling =
        SignalingClient::new_with_password(&cfg.signaling_addr, cfg.password.as_deref());
    let key_bytes = derive_key(key.as_deref());
    let (auth, relay_token) = relay_auth_info(&signaling, cfg).await;
    let relay_addr: SocketAddr = cfg
        .relay_addr
        .parse()
        .map_err(|e| anyhow::anyhow!("bad relay addr {}: {e}", cfg.relay_addr))?;

    // 网格数据平面：TUN 一次创建，重连只重建对端链路（测试可传 None 跳过 TUN）
    let (plane, mut dead_rx) = crate::p2p::tun::MeshPlane::new();
    let (dispatch_tx, dispatch_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut real_name = "frp0".to_string();
    if let Some(t) = &tun {
        let dev = crate::p2p::tun::create(&crate::p2p::tun::TunConfig {
            name: "frp0".into(),
            ip: t.ip.clone(),
            netmask: t.netmask.clone(),
            mtu: t.mtu,
        })?;
        real_name = crate::p2p::tun::device_name(&dev).unwrap_or_else(|| "frp0".to_string());
        println!(
            ">>> virtual NIC mode: IP {} / {} (MTU {}, device {real_name})",
            t.ip, t.netmask, t.mtu
        );
        match crate::p2p::tun::allow_firewall(&real_name) {
            Ok(()) => {
                println!("    firewall opened for {real_name} (peer can ping/access this host)")
            }
            Err(e) => {
                println!("    firewall rule failed (peer may not ping/access this host): {e}")
            }
        }
        if let Some(cidr) = utils::cidr_from_ip_netmask(&t.ip, &t.netmask) {
            match crate::p2p::tun::add_subnet_route(&cidr, &real_name) {
                Ok(()) => println!("    subnet route {cidr} → {real_name} added"),
                Err(e) => println!("    subnet route add failed: {e}"),
            }
        }
        println!(
            "    peer can now access this virtual subnet (e.g. ping {})",
            t.ip
        );
        tokio::spawn(crate::p2p::tun::run_mesh_plane(
            dev,
            plane.clone(),
            dispatch_rx,
        ));
    }

    let mut attempt: u64 = 0;
    loop {
        attempt += 1;
        if attempt > 1 {
            let wait = reconnect_delay(attempt);
            println!(
                "\n>>> {} ...",
                warn(format!(
                    "connection lost, reconnecting in {wait} seconds (Ctrl-C to exit)"
                ))
            );
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
        let engine = PunchEngine::bind().await?;
        let token = utils::rand_token();
        let my_ext = signaling
            .learn_public_addr(engine.socket(), cfg.signaling_udp_addr()?, &token)
            .await?;
        log::info!("public address: {my_ext}");
        let lan_addrs = utils::lan_socket_addrs(engine.local_addr()?.port());
        let lan_subnets = if expose_lan {
            utils::lan_subnet_cidrs()
        } else {
            Vec::new()
        };
        if let Err(e) = signaling
            .refresh_room(room_id, my_ext, lan_addrs, lan_subnets)
            .await
        {
            return Err(e.into()); // 房间失效（过期/被删）
        }
        let mesh_port = engine.local_addr()?.port();
        let (mesh, punch_rx) = engine.into_mesh();
        let r = mesh_host_loop(
            &signaling,
            &mesh,
            punch_rx,
            &plane,
            &dispatch_tx,
            &mut dead_rx,
            room_id,
            &token,
            force_relay,
            spread,
            key_bytes,
            relay_addr,
            auth,
            relay_token.as_deref(),
            tun.as_ref(),
            &real_name,
            max_guests,
            cfg.signaling_udp_addr()?,
            my_ext,
            mesh_port,
        )
        .await;
        // 测试用完成条件：达到目标访客数后 mesh_host_loop 正常返回 → 整个会话结束
        if r.is_ok() && max_guests.is_some() {
            return Ok(());
        }
        r?;
        // 循环 → 重连（重建 socket 与对端链路；TUN 数据平面常驻）
    }
}

/// host 网格主循环：轮询访客 → 打洞/中继建链 → 维护数据平面路由。
#[allow(clippy::too_many_arguments)]
async fn mesh_host_loop(
    signaling: &SignalingClient,
    mesh: &crate::p2p::stream::UdpMesh,
    mut punch_rx: tokio::sync::mpsc::UnboundedReceiver<(Vec<u8>, SocketAddr)>,
    plane: &Arc<crate::p2p::tun::MeshPlane>,
    dispatch_tx: &tokio::sync::mpsc::UnboundedSender<(String, Vec<u8>)>,
    dead_rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    room_id: &str,
    token: &str,
    force_relay: bool,
    spread: u32,
    key_bytes: Option<[u8; 32]>,
    relay_addr: SocketAddr,
    auth: bool,
    relay_token: Option<&str>,
    tun: Option<&TunOpts>,
    real_name: &str,
    max_guests: Option<u64>,
    udp_probe: SocketAddr,
    mut current_ext: SocketAddr,
    mesh_local_port: u16,
) -> anyhow::Result<()> {
    let mut pending: HashMap<String, PendingHost> = HashMap::new();
    let mut established: HashMap<String, String> = HashMap::new();
    let mut last_guests: HashMap<String, crate::signaling::GuestInfo> = HashMap::new();
    let (relay_tx, mut relay_rx) =
        tokio::sync::mpsc::unbounded_channel::<(String, Result<crate::p2p::relay::RelayStream>)>();
    // 网格轮询更快（250ms）：访客加入/地址变化尽快感知，缩小与首轮打洞的竞态窗口
    const MESH_POLL: Duration = Duration::from_millis(100);
    let mut last_poll = Instant::now() - MESH_POLL;
    let mut last_punch = Instant::now() - PUNCH_INTERVAL;
    // 定期重学公网地址：host 切换网络后房间地址刷新，访客可打洞到新地址恢复直连
    const EXT_REFRESH: Duration = Duration::from_secs(60);
    let mut last_ext_refresh = Instant::now();
    let mut probe_token: Option<String> = None;
    let mut probed_ext: Option<SocketAddr> = None;

    loop {
        let now = Instant::now();
        // 1) 轮询房间，同步访客列表（新访客进入打洞；地址变化在下轮打洞生效）
        if now.duration_since(last_poll) >= MESH_POLL {
            last_poll = now;
            if let Ok(info) = signaling.get_room(room_id).await {
                last_guests = info
                    .guests
                    .iter()
                    .map(|g| (g.uuid.clone(), g.clone()))
                    .collect();
                for g in info.guests {
                    if established.contains_key(&g.uuid) {
                        continue;
                    }
                    match pending.get_mut(&g.uuid) {
                        Some(p) => p.info = g,
                        None => {
                            log::info!(
                                "guest {} ({:?}) joined, punching ...",
                                g.uuid,
                                mesh_peer_addrs(&g)
                            );
                            pending.insert(
                                g.uuid.clone(),
                                PendingHost {
                                    info: g,
                                    relay_attempted: false,
                                    relay_at: None,
                                    deadline: Some(now + PUNCH_WINDOW),
                                },
                            );
                        }
                    }
                }
            }
        }
        // 2) 打洞（force_relay 跳过）
        if !force_relay && now.duration_since(last_punch) >= PUNCH_INTERVAL {
            last_punch = now;
            for p in pending.values() {
                let targets = punch_targets_multi(&mesh_peer_addrs(&p.info), spread, None);
                for a in targets {
                    let _ = mesh.send(format!("PUNCH {token}").as_bytes(), a).await;
                }
            }
        }
        // 3) 打洞事件：PUNCH → 回 ACK 并建立；ACK(我方 token) → 建立。
        //    事件可能先于轮询到达（首轮竞态）→ 找不到时现场查一次房间。
        while let Ok((bytes, src)) = punch_rx.try_recv() {
            let signal = if let Some(t) = parse_punch(&bytes) {
                let _ = mesh.send(format!("ACK {t}").as_bytes(), src).await;
                true
            } else if let Some(t) = parse_ack(&bytes) {
                t == token
            } else {
                // 公网地址探测回复：`ADDR <token> ip:port`
                let text = String::from_utf8_lossy(&bytes);
                if let (Some(pt), Some(rest)) = (&probe_token, text.strip_prefix("ADDR ")) {
                    let mut parts = rest.split_whitespace();
                    if parts.next() == Some(pt.as_str()) {
                        if let (Ok(ip), Ok(port)) = (
                            parts.next().unwrap_or("").parse::<std::net::IpAddr>(),
                            parts.next().unwrap_or("").parse::<u16>(),
                        ) {
                            probed_ext = Some(SocketAddr::new(ip, port));
                        }
                    }
                }
                false
            };
            if !signal {
                continue;
            }
            // 定位访客：pending → 房间快照 → 现场查询（补上"事件先于轮询"的窗口）
            let mut uuid = pending
                .iter()
                .find(|(_, p)| mesh_peer_addrs(&p.info).contains(&src))
                .map(|(u, _)| u.clone())
                .or_else(|| {
                    last_guests
                        .iter()
                        .find(|(_, g)| mesh_peer_addrs(g).contains(&src))
                        .map(|(u, _)| u.clone())
                });
            if uuid.is_none() {
                if let Ok(info) = signaling.get_room(room_id).await {
                    last_guests = info
                        .guests
                        .iter()
                        .map(|g| (g.uuid.clone(), g.clone()))
                        .collect();
                    uuid = info
                        .guests
                        .iter()
                        .find(|g| mesh_peer_addrs(g).contains(&src))
                        .map(|g| g.uuid.clone());
                }
            }
            let Some(uuid) = uuid else { continue };
            if established.contains_key(&uuid) {
                continue;
            }
            let info = pending
                .remove(&uuid)
                .map(|p| p.info)
                .or_else(|| last_guests.get(&uuid).cloned());
            if let Some(info) = info {
                log::info!("peer {uuid} signaled via {src}, establishing direct link");
                mesh_establish_link(
                    mesh,
                    plane,
                    dispatch_tx,
                    &uuid,
                    src,
                    key_bytes,
                    &info,
                    &mut established,
                    tun,
                    real_name,
                );
            }
        }
        // 4) 直连超时（或强制中继）→ 打开中继连接（按 UUID 配对）
        for (uuid, p) in pending.iter_mut() {
            if p.relay_attempted {
                continue;
            }
            let due = force_relay || p.deadline.map(|d| now >= d).unwrap_or(false);
            let retry_ok = p
                .relay_at
                .map(|t| now.duration_since(t) >= Duration::from_secs(30))
                .unwrap_or(true);
            if due && retry_ok {
                p.relay_attempted = true;
                p.relay_at = Some(now);
                let uuid_c = uuid.clone();
                let room_c = room_id.to_string();
                let tx = relay_tx.clone();
                let addr = relay_addr;
                let tok = relay_token.map(str::to_string);
                tokio::spawn(async move {
                    let r = relay::connect(
                        addr,
                        &room_c,
                        RelayRole::Host,
                        tok.as_deref(),
                        auth,
                        Some(&uuid_c),
                    )
                    .await;
                    let _ = tx.send((uuid_c, r));
                });
            }
        }
        // 5) 中继结果 → 注册链路
        while let Ok((uuid, res)) = relay_rx.try_recv() {
            match res {
                Ok(stream) => {
                    if let Some(p) = pending.remove(&uuid) {
                        log::info!("relay link with {uuid} established");
                        // 若已有链路（如直连刚建立）→ 替换
                        if established.contains_key(&uuid) {
                            plane.unregister(&uuid);
                        }
                        plane.register(
                            &uuid,
                            mesh_routes_for(&p.info),
                            stream,
                            dispatch_tx.clone(),
                        );
                        established.insert(uuid, "relay".into());
                    } else {
                        drop(stream); // 已直连 → 丢弃多余中继
                    }
                }
                Err(e) => {
                    log::warn!("relay connect to {uuid} failed: {e}");
                    if let Some(p) = pending.get_mut(&uuid) {
                        p.relay_attempted = false; // 稍后重试
                    }
                }
            }
        }
        // 6) 对端断线 → 重新入打洞队列
        while let Ok(uuid) = dead_rx.try_recv() {
            if established.remove(&uuid).is_some() {
                log::warn!("peer {uuid} link lost, re-punching ...");
                if let Some(info) = last_guests.get(&uuid) {
                    pending.insert(
                        uuid,
                        PendingHost {
                            info: info.clone(),
                            relay_attempted: false,
                            relay_at: None,
                            deadline: Some(now + PUNCH_WINDOW),
                        },
                    );
                }
            }
        }
        // 7) 路由器死亡（socket 错误）→ 触发外层重连
        if punch_rx.is_closed() {
            anyhow::bail!("mesh socket closed");
        }
        // 8) 定期重学公网地址：地址变化 → 刷新房间（host 切换网络后访客能重新打洞直连）
        if now.duration_since(last_ext_refresh) >= EXT_REFRESH {
            last_ext_refresh = now;
            probe_token = Some(utils::rand_token());
            let pt = probe_token.clone().unwrap();
            let _ = mesh.send(format!("ECHO {pt}").as_bytes(), udp_probe).await;
        }
        if let Some(ext) = probed_ext.take() {
            if ext != current_ext {
                current_ext = ext;
                let lan_addrs = utils::lan_socket_addrs(mesh_local_port);
                let lan_subnets = if tun.is_some() {
                    utils::lan_subnet_cidrs()
                } else {
                    Vec::new()
                };
                log::info!("host public address changed to {ext}, refreshing room ...");
                let _ = signaling
                    .refresh_room(room_id, ext, lan_addrs, lan_subnets)
                    .await;
            }
        }
        // 测试用完成条件：达到目标访客数且无在建链路 → 结束
        if let Some(n) = max_guests {
            if established.len() as u64 >= n && pending.is_empty() {
                return Ok(());
            }
        }
        // 节流，避免空转
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// 建立直连链路：注册网格流 + 数据平面路由 + 访客局域网内核路由。
#[allow(clippy::too_many_arguments)]
fn mesh_establish_link(
    mesh: &crate::p2p::stream::UdpMesh,
    plane: &Arc<crate::p2p::tun::MeshPlane>,
    dispatch_tx: &tokio::sync::mpsc::UnboundedSender<(String, Vec<u8>)>,
    uuid: &str,
    peer: SocketAddr,
    key_bytes: Option<[u8; 32]>,
    info: &crate::signaling::GuestInfo,
    established: &mut HashMap<String, String>,
    tun: Option<&TunOpts>,
    real_name: &str,
) {
    // 访客 --expose-lan 通告的局域网子网 → 经虚拟网卡路由（本机访问访客局域网）
    if let Some(t) = tun {
        for cidr in &info.subnets {
            match crate::p2p::tun::add_route(cidr, real_name, &t.ip) {
                Ok(()) => println!("    route {cidr} → {real_name} added (access guest LAN)"),
                Err(e) => println!("    route {cidr} add failed: {e}"),
            }
        }
    }
    if established.contains_key(uuid) {
        plane.unregister(uuid); // 替换旧链路（如中继 → 直连）
    }
    let stream = mesh.add_peer(peer, key_bytes);
    plane.register(
        uuid,
        mesh_routes_for(info),
        Box::new(stream),
        dispatch_tx.clone(),
    );
    established.insert(uuid.to_string(), "direct".into());
}
