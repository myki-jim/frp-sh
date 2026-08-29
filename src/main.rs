use clap::Parser;
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
            .map_err(|e| anyhow::anyhow!("任务异常终止: {e}"))?
    })
}

async fn real_main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    let filter = if cli.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(filter)).init();

    let cli::Cli {
        command, config, ..
    } = cli;

    // 版本横幅：每次运行显示当前版本与线协议版本
    println!(
        "frp-sh v{} (protocol {})",
        frp_sh::version::VERSION,
        frp_sh::version::PROTOCOL_VERSION
    );

    // lan 组网需要管理员/root（创建虚拟网卡、设 IP、加路由）。
    // 未提权时：Windows 自动弹 UAC 以管理员重启自身（用户点一次"是"）；
    // macOS/Linux 提示用 sudo。
    if frp_sh::commands::needs_elevation(&command) && !frp_sh::commands::is_elevated() {
        if frp_sh::commands::handle_elevation(&command)? {
            return Ok(()); // 已在提权子进程中继续运行，本进程结束
        }
        std::process::exit(1);
    }

    // 更新检查（serve 只提示不询问；其余命令交互询问）
    let interactive = !matches!(command, Some(Commands::Serve { .. }));
    frp_sh::update::maybe_check_update(interactive).await?;

    match command {
        Some(Commands::Serve { addr, relay_addr }) => {
            frp_sh::commands::run_serve(addr, relay_addr).await?;
        }
        Some(Commands::Config) => {
            frp_sh::commands::run_config(config).await?;
        }
        Some(Commands::Game { cmd }) => {
            check_config_hint(&config);
            match cmd {
                GameCmd::Create(args) => {
                    let cfg = Config::load_auto(config.as_deref())?;
                    let prefix = args.prefix.unwrap_or_else(|| "game".into());
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
                    let prefix = args.prefix.unwrap_or_else(|| "dev".into());
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
                    let prefix = args.prefix.unwrap_or_else(|| "lan".into());
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
                        requested_ip,
                        args.expose_lan,
                    )
                    .await?;
                }
            }
        }
        None => {
            // 首次运行：无子命令 → 配置向导；已有配置 → 显示概要
            if !Config::default_exists() {
                println!("欢迎使用 frp-sh！首次运行需要先配置信令服务器。\n");
                frp_sh::commands::run_config(config).await?;
            } else {
                let cfg = Config::load_auto(config.as_deref())?;
                println!("frp-sh - 社交化 P2P 组网工具\n");
                println!("  当前配置:");
                println!("    信令服务器: {}", cfg.signaling_addr);
                println!("    中继服务器: {}", cfg.relay_addr);
                if let Some(u) = &cfg.signaling_udp {
                    println!("    UDP 探测   : {u}");
                }
                if let Some(id) = &cfg.uuid {
                    println!("    你的 ID    : {id}");
                }
                println!("\n  常用命令:");
                println!("    frp-sh lan create              # 组网：创建房间（虚拟网卡整机入网）");
                println!(
                    "    frp-sh lan join lan-xxxxxx    # 组网：加入房间（可访问对方整个局域网）"
                );
                println!("    frp-sh dev create              # 开发：应用层端口转发");
                println!("    frp-sh game create             # 游戏：纯端口转发（默认 25565）");
                println!("    frp-sh serve                   # 启动信令服务器");
                println!("    frp-sh config                  # 重新配置");
                println!("    frp-sh --help                  # 查看全部命令\n");
            }
        }
    }
    Ok(())
}

/// 未配置默认服务器时给出提示（不阻塞，使用内置默认 127.0.0.1）。
fn check_config_hint(config: &Option<std::path::PathBuf>) {
    if config.is_none() && !Config::default_exists() {
        println!(
            "提示：尚未配置信令服务器，正在使用内置默认 127.0.0.1:8080。\n\
             运行 `frp-sh config` 可交互式配置你的服务器。\n"
        );
    }
}
