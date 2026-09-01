use clap::Parser;
use colored::Colorize;
use frp_sh::cli::{self, Commands, DevCmd, GameCmd, LanCmd};
use frp_sh::config::Config;

/// 手动构建 runtime 并在大栈 worker 线程上运行主逻辑。
///
/// `#[tokio::main]` 的 `block_on` 会把 async main 的整个 future（包含所有
/// 分支的异步状态机）压在主线程栈上（Windows 默认仅 1MB），分支增多后
/// 会直接栈溢出（启动即崩溃）。改用大栈 worker 线程承载主逻辑即可规避。
fn main() -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .thread_stack_size(8 * 1024 * 1024)
        .enable_all()
        .build()?;
    rt.block_on(async {
        let handle = rt.spawn(async { real_main().await });
        handle
            .await
            .map_err(|e| anyhow::anyhow!("task terminated abnormally: {e}"))?
    })
}

async fn real_main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    let filter = if cli.verbose { "debug" } else { "info" };
    // 日志终端静默：写入 <配置目录>/logs/frp-sh.log + 内存环形缓冲（面板 /api/debug）
    frp_sh::debuglog::init(filter);

    let cli::Cli {
        command,
        config,
        panel_addr,
        no_panel,
        name,
        punch_retries,
        ..
    } = cli;
    frp_sh::config::set_punch_retries(punch_retries);

    frp_sh::config::set_cli_name(name);

    // 客户端 Web 面板（默认 127.0.0.1:6793）：随进程常驻，无会话时显示 idle。
    // serve 模式不需要（服务端面板挂在 8080 的 /panel）。
    if !no_panel && !matches!(command, Some(Commands::Serve { .. })) {
        if let Ok(addr) = panel_addr.parse() {
            tokio::spawn(frp_sh::panel::serve_client(addr));
        } else {
            eprintln!("  ⚠ bad --panel-addr {panel_addr}; panel disabled");
        }
    }

    // 版本横幅：ASCII 字符画 logo + 版本/线协议
    println!(
        "{}",
        r#"
 ______ _____  _____   _____ _    _
|  ____|  __ \|  __ \ / ____| |  | |
| |__  | |__) | |__) | (___ | |__| |
|  __| |  _  /|  ___/ \___ \|  __  |
| |    | | \ \| |     ____) | |  | |
|_|    |_|  \_\_|    |_____/|_|  |_|"#
            .green()
    );
    println!(
        "  {} {}",
        format!("frp-sh v{}", frp_sh::version::VERSION)
            .cyan()
            .bold(),
        format!("protocol v{}", frp_sh::version::PROTOCOL_VERSION).dimmed()
    );
    println!();

    // lan 组网需要管理员/root（创建虚拟网卡、设 IP、加路由）。
    // 未提权时：Windows 自动弹 UAC 以管理员重启自身（用户点一次"是"）；
    // macOS/Linux 提示用 sudo。
    if frp_sh::commands::needs_elevation(&command) && !frp_sh::commands::is_elevated() {
        if frp_sh::commands::handle_elevation(&command)? {
            return Ok(()); // 已在提权子进程中继续运行，本进程结束
        }
        std::process::exit(1);
    }

    // 单实例锁（按角色）：同机同时只允许一个房主会话/一个访客会话/一个服务端，
    // 防止误开多个房间；host 与 guest 各一把锁，双端同机不受影响。
    // 注意：必须放在提权门之后——Windows 提权父进程会等待子进程，不能持有锁。
    if let Some(role) = frp_sh::commands::session_role(&command) {
        if let Err(e) = frp_sh::commands::acquire_role_lock(role) {
            eprintln!("\n  ✗ {e}\n");
            std::process::exit(1);
        }
    }

    // 更新检查（serve 只提示不询问；其余命令交互询问）
    let interactive = !matches!(command, Some(Commands::Serve { .. }));
    frp_sh::update::maybe_check_update(interactive).await?;

    match command {
        Some(Commands::Serve {
            addr,
            relay_addr,
            udp_addr,
            password,
            turn,
            external_ip,
        }) => {
            let ext = match &external_ip {
                Some(s) => Some(
                    s.parse()
                        .map_err(|e| anyhow::anyhow!("bad --external-ip {s}: {e}"))?,
                ),
                None => None,
            };
            frp_sh::commands::run_serve(addr, relay_addr, udp_addr, password, turn, ext).await?;
        }
        Some(Commands::Config) => {
            frp_sh::commands::run_config(config).await?;
        }
        Some(Commands::Profile { cmd }) => {
            frp_sh::commands::run_profile(cmd, config).await?;
        }
        Some(Commands::Game { cmd }) => {
            check_config_hint(&config);
            match cmd {
                GameCmd::Create(args) => {
                    let cfg = Config::load_auto(config.as_deref())?;
                    let prefix = args.prefix.unwrap_or_default();
                    // game 系列：纯端口转发，不组网
                    frp_sh::commands::run_create(
                        cfg,
                        prefix,
                        args.ttl,
                        args.service,
                        args.relay,
                        args.key,
                        args.max_conns,
                        args.spread,
                        None,
                        Vec::new(),
                        false,
                    )
                    .await?;
                }
                GameCmd::Join(args) => {
                    let cfg = Config::load_auto(config.as_deref())?;
                    frp_sh::commands::run_join(
                        cfg,
                        args.room_id,
                        args.listen,
                        args.relay,
                        args.key,
                        args.max_conns,
                        args.spread,
                        None,
                        None,
                        false,
                    )
                    .await?;
                }
            }
        }
        Some(Commands::Dev { cmd }) => {
            check_config_hint(&config);
            match cmd {
                DevCmd::Create(args) => {
                    let cfg = Config::load_auto(config.as_deref())?;
                    let prefix = args.prefix.unwrap_or_default();
                    // dev 系列：应用层端口转发，不组网
                    frp_sh::commands::run_create(
                        cfg,
                        prefix,
                        args.ttl,
                        args.service,
                        args.relay,
                        args.key,
                        args.max_conns,
                        args.spread,
                        None,
                        Vec::new(),
                        false,
                    )
                    .await?;
                }
                DevCmd::Join(args) => {
                    let cfg = Config::load_auto(config.as_deref())?;
                    frp_sh::commands::run_join(
                        cfg,
                        args.room_id,
                        args.listen,
                        args.relay,
                        args.key,
                        args.max_conns,
                        args.spread,
                        None,
                        None,
                        false,
                    )
                    .await?;
                }
            }
        }
        Some(Commands::Lan { cmd }) => {
            check_config_hint(&config);
            match cmd {
                LanCmd::Create(args) => {
                    let cfg = Config::load_auto(config.as_deref())?;
                    let prefix = args.prefix.unwrap_or_default();
                    // lan 系列：组网，虚拟网卡默认开启；service 为占位（TUN 模式不使用）
                    let d = frp_sh::commands::TunOpts::host_default();
                    let tun_opts = Some(frp_sh::commands::TunOpts {
                        ip: args.ip.unwrap_or(d.ip),
                        netmask: args.netmask,
                        mtu: args.mtu,
                        lan_routes: Vec::new(),
                    });
                    frp_sh::commands::run_create(
                        cfg,
                        prefix,
                        args.ttl,
                        "127.0.0.1:25565".into(),
                        args.relay,
                        args.key,
                        0,
                        args.spread,
                        tun_opts,
                        args.guest_ips,
                        args.expose_lan,
                    )
                    .await?;
                }
                LanCmd::Join(args) => {
                    let cfg = Config::load_auto(config.as_deref())?;
                    // 显式 --ip 优先；否则由 UUID 派生（稳定）；房主启用 IP 池时由服务器分配
                    let requested_ip = args.ip.clone();
                    let ip = requested_ip.clone().unwrap_or_else(|| {
                        cfg.uuid
                            .as_deref()
                            .map(frp_sh::utils::derive_vnet_ip)
                            .unwrap_or_else(|| frp_sh::commands::TunOpts::guest_default().ip)
                    });
                    let tun_opts = Some(frp_sh::commands::TunOpts {
                        ip,
                        netmask: args.netmask,
                        mtu: args.mtu,
                        lan_routes: Vec::new(), // 加入房间后由房主通告的子网填充
                    });
                    // lan join 恒为 TUN 模式：把本机实际使用的虚拟 IP 上报服务器，
                    // 房主据此建 /32 路由（否则房主→访客单播全部丢弃，ping 不通）
                    let reported_ip =
                        Some(tun_opts.as_ref().map(|t| t.ip.clone()).unwrap_or_default());
                    // listen 为占位（TUN 模式不使用）
                    frp_sh::commands::run_join(
                        cfg,
                        args.room_id,
                        "127.0.0.1:25565".into(),
                        args.relay,
                        args.key,
                        0,
                        args.spread,
                        tun_opts,
                        reported_ip,
                        args.expose_lan,
                    )
                    .await?;
                }
            }
        }
        None => {
            // 首次运行：无子命令 → 配置向导；已有配置 → 显示概要
            if !Config::default_exists() {
                println!(
                    "Welcome to frp-sh! The first run requires configuring the signaling server.\n"
                );
                frp_sh::commands::run_config(config).await?;
            } else {
                let cfg = Config::load_auto(config.as_deref())?;
                println!("{}", "frp-sh - social P2P mesh tool".cyan().bold());
                let mut home_rows: Vec<(&str, String)> = vec![
                    ("Signaling", cfg.signaling_addr.clone()),
                    ("Relay", cfg.relay_addr.clone()),
                ];
                if let Some(u) = &cfg.signaling_udp {
                    home_rows.push(("UDP probe", u.clone()));
                }
                if let Some(id) = &cfg.uuid {
                    home_rows.push(("Your ID", id.clone()));
                }
                println!("{}", frp_sh::utils::kv_table(&home_rows));
                println!("\n  {}", "Common commands:".cyan());
                println!(
                    "    {}  mesh: create a room (virtual NIC puts the whole machine on the mesh)",
                    "frp-sh lan create".dimmed()
                );
                println!(
                    "    {}  mesh: join a room (reach the peer's whole LAN)",
                    "frp-sh lan join 1234".dimmed()
                );
                println!(
                    "    {}  development: application-layer port forwarding",
                    "frp-sh dev create".dimmed()
                );
                println!(
                    "    {}  game: pure port forwarding (default 25565)",
                    "frp-sh game create".dimmed()
                );
                println!(
                    "    {}  start the signaling server",
                    "frp-sh serve".dimmed()
                );
                println!("    {}  reconfigure", "frp-sh config".dimmed());
                println!("    {}  show all commands\n", "frp-sh --help".dimmed());
            }
        }
    }
    Ok(())
}

/// 未配置默认服务器时给出提示（不阻塞，使用内置默认 127.0.0.1）。
fn check_config_hint(config: &Option<std::path::PathBuf>) {
    if config.is_none() && !Config::default_exists() {
        println!(
            "Note: no signaling server configured; using the built-in default 127.0.0.1:8080.\n\
             Run `frp-sh config` to configure your server interactively.\n"
        );
    }
}
