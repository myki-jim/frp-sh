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
use std::io::Write;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::net::{TcpListener, UdpSocket};

/// 打洞窗口时长（超时后转入中继）
pub const PUNCH_WINDOW: Duration = Duration::from_secs(3);
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

/// 重连退避：2s, 4s, 8s, ... 上限 15s。
fn reconnect_delay(attempt: u64) -> u64 {
    let exp = 1u64 << attempt.min(4);
    exp.min(15)
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
pub async fn run_serve(http_addr: String, relay_addr: String) -> anyhow::Result<()> {
    let http_listener = TcpListener::bind(&http_addr).await?;
    let http_sock: SocketAddr = http_listener.local_addr()?;
    let udp = UdpSocket::bind(http_sock).await?;
    let relay_listener = TcpListener::bind(&relay_addr).await?;
    let relay_sock: SocketAddr = relay_listener.local_addr()?;

    println!("frp-sh signaling server");
    println!("  HTTP REST : {http_sock}");
    println!("  UDP echo  : {http_sock}");
    println!("  TCP relay : {relay_sock}");
    println!("  (Ctrl-C to stop)");

    let state = server::new_state();
    let http_task = tokio::spawn(server::run_http(http_listener, state.clone()));
    let udp_task = tokio::spawn(server::run_udp_echo(udp));
    let relay_task = tokio::spawn(server::run_relay(relay_listener, state));

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

    let cfg = Config {
        signaling_addr,
        relay_addr,
        signaling_udp,
        uuid: None, // 设备 UUID 存于独立 identity 文件
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
        Ok(Some((server_ver, server_proto))) => {
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
    let signaling = SignalingClient::new(&cfg.signaling_addr);
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
    let signaling = SignalingClient::new(&cfg.signaling_addr);
    let service_addr: SocketAddr = service
        .parse()
        .map_err(|e| FrpError::Config(format!("bad service addr {service}: {e}")))?;
    let mut state = RoomState::new(room_id.to_string(), Role::Host, service_addr);
    let key_bytes = derive_key(key.as_deref());
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
                let stream = match relay::connect(relay_addr, room_id, RelayRole::Host).await {
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
    loop {
        let now = Instant::now();
        // 1) 轮询房间，等待访客注册
        if guest_addrs.is_empty() && now.duration_since(last_poll) >= POLL_INTERVAL {
            last_poll = now;
            if let Ok(info) = signaling.get_room(room_id).await {
                if let Some(g) = info.guest_addr {
                    guest_addrs.push(g);
                    guest_addrs.extend(info.guest_lan);
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
        // 3) 接收数据报
        let recv_timeout = match deadline {
            Some(d) => d.saturating_duration_since(now),
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
        // 4) 超时判定
        if let Some(d) = deadline {
            if Instant::now() >= d {
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
    let signaling = SignalingClient::new(&cfg.signaling_addr);
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
    let signaling = SignalingClient::new(&cfg.signaling_addr);
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

        let outcome =
            match guest_punch_phase(&engine, &host_addrs, &token, force_relay, spread).await {
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
                let stream = match relay::connect(relay_addr, room_id, RelayRole::Guest).await {
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
                    match final_punch_check(&engine, &host_addrs, &token, spread).await {
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
async fn guest_punch_phase(
    engine: &PunchEngine,
    host_addrs: &[SocketAddr],
    token: &str,
    force_relay: bool,
    spread: u32,
) -> Result<PunchOutcome> {
    if force_relay {
        return Ok(PunchOutcome::TimedOut);
    }
    let my_local = engine.local_addr().ok();
    let my_port = my_local.map(|a| a.port());
    let targets = punch_targets_multi(host_addrs, spread, my_local);
    let deadline = Instant::now() + PUNCH_WINDOW;
    let mut last_punch = Instant::now() - PUNCH_INTERVAL;
    loop {
        let now = Instant::now();
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
        if Instant::now() >= deadline {
            return Ok(PunchOutcome::TimedOut);
        }
    }
}

/// 中继连接后的最终直连复查（约 400ms）。
async fn final_punch_check(
    engine: &PunchEngine,
    host_addrs: &[SocketAddr],
    token: &str,
    spread: u32,
) -> Result<PunchOutcome> {
    let my_local = engine.local_addr().ok();
    let my_port = my_local.map(|a| a.port());
    let targets = punch_targets_multi(host_addrs, spread, my_local);
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
