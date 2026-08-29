//! 命令编排：serve / game create / game join 的核心流程。

use crate::config::Config;
use crate::error::{FrpError, Result};
use crate::p2p::hole_punch::{
    parse_ack, parse_punch, punch_targets_excluding, PunchEngine, PunchOutcome,
};
use crate::p2p::relay::{self, RelayRole};
use crate::p2p::stream;
use crate::room::state::{Role, RoomState};
use crate::signaling::server;
use crate::signaling::SignalingClient;
use crate::tunnel;
use crate::utils;
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
}

impl TunOpts {
    pub fn host_default() -> Self {
        Self {
            ip: "10.66.0.1".into(),
            netmask: "255.255.255.0".into(),
            mtu: 1400,
        }
    }

    pub fn guest_default() -> Self {
        Self {
            ip: "10.66.0.2".into(),
            netmask: "255.255.255.0".into(),
            mtu: 1400,
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

/// 数据面分发：`--tun` 时走虚拟网卡，否则走 TCP 端口转发。
async fn run_data_plane(
    transport: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    tun: Option<&TunOpts>,
    mode: ForwardMode,
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
        println!(
            ">>> 虚拟网卡模式: IP {} / {} (MTU {}, 设备 {dev_name})",
            o.ip, o.netmask, o.mtu
        );
        println!("    对端现可访问本虚拟网段（如 ping {}）", o.ip);
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
    reqwest::Url::parse(&with_scheme)
        .map_err(|e| anyhow::anyhow!("地址格式不正确（示例：101.43.41.195:8080）：{e}"))?;
    Ok(with_scheme)
}

/// `frp-sh config`：交互式配置信令服务器（首次运行向导）。
///
/// `save_path`：显式保存路径（`--config` 指定）；否则保存到平台默认路径。
pub async fn run_config(save_path: Option<PathBuf>) -> anyhow::Result<()> {
    let mut out = std::io::stdout();

    println!("\n  frp-sh 配置向导");
    println!("  ==================");
    println!("  请填写信令服务器信息（直接回车使用默认值）:\n");

    // 1. 信令服务器地址
    print!("  1. 信令服务器地址 (HTTP) [默认 http://127.0.0.1:8080]\n  > ");
    out.flush()?;
    let signaling_input = read_line();
    let signaling_addr = normalize_signaling(&signaling_input)?;

    // 2. 中继地址（默认由信令地址推导：同主机 + 8081）
    let host = reqwest::Url::parse(&signaling_addr)?
        .host_str()
        .unwrap_or("127.0.0.1")
        .to_string();
    let default_relay = format!("{host}:8081");
    print!("  2. 中继服务器地址 (TCP) [默认 {default_relay}]\n  > ");
    out.flush()?;
    let relay_input = read_line();
    let relay_addr = if relay_input.trim().is_empty() {
        default_relay
    } else {
        relay_input.trim().to_string()
    };

    // 3. UDP 探测独立端口（可选）
    print!("  3. UDP 探测使用独立端口？(y/N) ");
    out.flush()?;
    let udp_input = read_line();
    let want_udp = matches!(udp_input.trim().to_lowercase().as_str(), "y" | "yes");
    let signaling_udp = if want_udp {
        print!("     独立 UDP 探测地址 (如 {host}:8082):\n  > ");
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
        None => Config::default_path()
            .ok_or_else(|| anyhow::anyhow!("无法确定默认配置目录（未设置 APPDATA/HOME）"))?,
    };
    cfg.save(&path)?;
    println!("\n  已保存配置: {}", path.display());
    println!("    信令服务器: {}", cfg.signaling_addr);
    println!("    中继服务器: {}", cfg.relay_addr);
    if let Some(u) = &cfg.signaling_udp {
        println!("    UDP 探测   : {u}");
    }

    // 软连通性检查（不阻塞）
    println!("\n  正在检查服务器连通性 ...");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()?;
    match client
        .get(format!("{}/health", cfg.signaling_addr))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => {
            println!("  [OK] 信令服务器可达");
        }
        Ok(_) => println!("  [WARN] 服务器返回了异常状态"),
        Err(_) => println!(
            "  [WARN] 无法连接信令服务器（请检查地址与防火墙，可稍后运行 frp-sh config 重试）"
        ),
    }

    println!("\n  下一步:");
    println!("    frp-sh game create             # 房主：创建房间");
    println!("    frp-sh game join game-xxxxxx   # 访客：加入房间");
    println!("  重新配置: frp-sh config\n");
    Ok(())
}

// ---------- game create ----------

/// `game create`：注册房间，进入房主会话；返回房间号。
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
) -> anyhow::Result<String> {
    let signaling = SignalingClient::new(&cfg.signaling_addr);
    let engine = PunchEngine::bind().await?;
    let token = utils::rand_token();
    let my_ext = signaling
        .learn_public_addr(engine.socket(), cfg.signaling_udp_addr()?, &token)
        .await?;
    log::info!("public address: {my_ext}");
    let tun_ip = tun.as_ref().map(|t| t.ip.clone());
    let resp = signaling.create_room(&prefix, ttl, my_ext, tun_ip).await?;
    let room_id = resp.room_id.clone();
    println!("\n  Room created : {room_id}");
    println!("  Signaling    : {}", cfg.signaling_addr);
    if let Some(u) = &cfg.uuid {
        println!("  Your ID      : {u}");
    }
    if let Some(t) = &tun {
        println!("  Vnet IP      : {}（对端可 ping/直连此 IP）", t.ip);
        println!("  Mode         : virtual NIC (--tun)");
    } else {
        println!("  Local service: {service}");
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
            println!("\n>>> 连接已断开，{wait} 秒后自动重连（Ctrl-C 退出）...");
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
        // 每次（重）连接绑定新 socket 并刷新公网地址（NAT 映射可能已过期）
        let engine = PunchEngine::bind().await?;
        let token = utils::rand_token();
        let my_ext = signaling
            .learn_public_addr(engine.socket(), cfg.signaling_udp_addr()?, &token)
            .await?;
        log::info!("public address: {my_ext}");
        if let Err(e) = signaling.refresh_room(room_id, my_ext).await {
            // 房间已失效（过期/被删）→ 无法继续，结束
            return Err(e.into());
        }

        let outcome =
            match host_punch_phase(&engine, &signaling, room_id, &token, force_relay, spread).await
            {
                Ok(o) => o,
                Err(e) => {
                    log::warn!("打洞阶段异常: {e}");
                    continue;
                }
            };
        let run_result = match outcome {
            PunchOutcome::Direct { peer, first } => {
                state.peer_addr = Some(peer);
                println!(">>> P2P direct link established with {peer}");
                let stream = engine.into_stream(peer, first, key_bytes);
                run_data_plane(
                    stream,
                    tun.as_ref(),
                    ForwardMode::Host {
                        service: service_addr,
                        max_conns,
                    },
                )
                .await
            }
            PunchOutcome::TimedOut => {
                state.relay_mode = true;
                println!(">>> UDP hole punching failed, falling back to relay ...");
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
                        log::warn!("中继连接失败: {e}");
                        continue;
                    }
                };
                println!(">>> relay connected, waiting for guest ...");
                run_data_plane(
                    stream,
                    tun.as_ref(),
                    ForwardMode::Host {
                        service: service_addr,
                        max_conns,
                    },
                )
                .await
            }
        };
        if let Err(e) = run_result {
            log::warn!("会话异常结束: {e}");
        } else {
            println!(">>> 会话结束，准备重连 ...");
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
async fn host_punch_phase(
    engine: &PunchEngine,
    signaling: &SignalingClient,
    room_id: &str,
    token: &str,
    force_relay: bool,
    spread: u32,
) -> Result<PunchOutcome> {
    let mut guest_addr: Option<SocketAddr> = None;
    let mut deadline: Option<Instant> = None;
    let mut last_poll = Instant::now() - POLL_INTERVAL;
    let mut last_punch = Instant::now() - PUNCH_INTERVAL;
    let my_local = engine.local_addr().ok();
    let my_port = my_local.map(|a| a.port());
    loop {
        let now = Instant::now();
        // 1) 轮询房间，等待访客注册
        if guest_addr.is_none() && now.duration_since(last_poll) >= POLL_INTERVAL {
            last_poll = now;
            if let Ok(info) = signaling.get_room(room_id).await {
                guest_addr = info.guest_addr;
            }
        }
        // 2) 访客出现后启动打洞窗口（强制中继则直接转入中继，且不收发任何打洞数据报）
        if force_relay {
            if guest_addr.is_some() {
                return Ok(PunchOutcome::TimedOut);
            }
            continue;
        }
        if let Some(g) = guest_addr {
            if deadline.is_none() {
                log::info!("guest {g} registered, punching ...");
                deadline = Some(now + PUNCH_WINDOW);
            }
            if let Some(d) = deadline {
                if now < d && now.duration_since(last_punch) >= PUNCH_INTERVAL {
                    for target in punch_targets_excluding(g, spread, my_local) {
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
) -> anyhow::Result<()> {
    if !utils::validate_room_id(&room_id) {
        anyhow::bail!("invalid room id: {room_id} (expected format like game-a3f9c2)");
    }
    if let Some(u) = &cfg.uuid {
        println!("  Your ID      : {u}");
    }
    if let Some(t) = &tun {
        println!(
            "  Vnet IP      : {}（朋友可用此 IP 直连你的虚拟网卡）",
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
/// `max_rounds` 仅用于测试：限制重连轮数，`None` 表示无限重连直到 Ctrl-C 或房间失效。
#[allow(clippy::too_many_arguments)]
pub async fn guest_session(
    cfg: &Config,
    room_id: &str,
    listen: String,
    force_relay: bool,
    key: Option<String>,
    max_conns: u64,
    spread: u32,
    tun: Option<TunOpts>,
    max_rounds: Option<u64>,
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
            println!("\n>>> 连接已断开，{wait} 秒后自动重连（Ctrl-C 退出）...");
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
        first = false;
        // 每次（重）连接绑定新 socket（旧 NAT 映射已失效）
        let engine = PunchEngine::bind().await?;

        // 查询房间（刷新房主地址；房间失效则结束）
        let (host_addr, host_tun_ip) = match retry_get_room_info(&signaling, room_id).await {
            Ok(info) => (info.host_addr, info.tun_ip),
            Err(FrpError::RoomNotFound(_)) => {
                return Err(FrpError::RoomNotFound(room_id.to_string()).into());
            }
            Err(e) => {
                log::warn!("查询房间失败: {e}");
                continue;
            }
        };

        // 刷新本机公网地址并登记（NAT 映射可能已过期）
        let token = utils::rand_token();
        let my_ext = match signaling
            .learn_public_addr(engine.socket(), cfg.signaling_udp_addr()?, &token)
            .await
        {
            Ok(a) => a,
            Err(e) => {
                log::warn!("公网地址探测失败: {e}");
                continue;
            }
        };
        match signaling.join_room(room_id, my_ext).await {
            Ok(join) => {
                println!("\n  Joined room : {}", join.room_id);
                println!("  Host address: {}", join.host_addr);
                if let Some(ip) = &host_tun_ip {
                    println!("  Host vnet IP: {ip}");
                }
                if tun.is_some() {
                    println!("  Mode         : virtual NIC (--tun)");
                } else {
                    println!("  Local listen: {listen}");
                }
                if key.is_some() {
                    println!("  Encryption   : on (--key)");
                }
                println!("  Punching through NAT ...\n");
            }
            Err(e) => {
                log::warn!("登记房间失败: {e}");
                continue;
            }
        }

        let outcome = match guest_punch_phase(&engine, host_addr, &token, force_relay, spread).await
        {
            Ok(o) => o,
            Err(e) => {
                log::warn!("打洞阶段异常: {e}");
                continue;
            }
        };
        let run_result = match outcome {
            PunchOutcome::Direct { peer, first } => {
                state.peer_addr = Some(peer);
                println!(">>> P2P direct link established with {peer}");
                let stream = engine.into_stream(peer, first, key_bytes);
                run_data_plane(
                    stream,
                    tun.as_ref(),
                    ForwardMode::Guest {
                        listen: listen_addr,
                        max_conns,
                    },
                )
                .await
            }
            PunchOutcome::TimedOut => {
                state.relay_mode = true;
                println!(">>> UDP hole punching failed, trying relay ...");
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
                        log::warn!("中继连接失败: {e}");
                        continue;
                    }
                };
                if force_relay {
                    // 强制中继：不再尝试任何打洞
                    println!(">>> relay connected, waiting for host ...");
                    run_data_plane(
                        stream,
                        tun.as_ref(),
                        ForwardMode::Guest {
                            listen: listen_addr,
                            max_conns,
                        },
                    )
                    .await
                } else {
                    // 中继连接后再做一次直连复查（应对"房主已打通、访客 ACK 丢失"的不对称场景）
                    match final_punch_check(&engine, host_addr, &token, spread).await {
                        Ok(PunchOutcome::Direct { peer, first }) => {
                            println!(">>> late P2P link established with {peer}");
                            drop(stream);
                            let stream = engine.into_stream(peer, first, key_bytes);
                            run_data_plane(
                                stream,
                                tun.as_ref(),
                                ForwardMode::Guest {
                                    listen: listen_addr,
                                    max_conns,
                                },
                            )
                            .await
                        }
                        _ => {
                            println!(">>> relay connected, waiting for host ...");
                            run_data_plane(
                                stream,
                                tun.as_ref(),
                                ForwardMode::Guest {
                                    listen: listen_addr,
                                    max_conns,
                                },
                            )
                            .await
                        }
                    }
                }
            }
        };
        if let Err(e) = run_result {
            log::warn!("会话异常结束: {e}");
        } else {
            println!(">>> 会话结束，准备重连 ...");
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

/// 访客打洞阶段：向房主地址持续发送 PUNCH 并等待 ACK / 数据帧。
async fn guest_punch_phase(
    engine: &PunchEngine,
    host_addr: SocketAddr,
    token: &str,
    force_relay: bool,
    spread: u32,
) -> Result<PunchOutcome> {
    if force_relay {
        return Ok(PunchOutcome::TimedOut);
    }
    let my_local = engine.local_addr().ok();
    let my_port = my_local.map(|a| a.port());
    let targets = punch_targets_excluding(host_addr, spread, my_local);
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
    host_addr: SocketAddr,
    token: &str,
    spread: u32,
) -> Result<PunchOutcome> {
    let my_local = engine.local_addr().ok();
    let my_port = my_local.map(|a| a.port());
    let targets = punch_targets_excluding(host_addr, spread, my_local);
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
