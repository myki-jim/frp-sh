use clap::{CommandFactory, FromArgMatches};
use colored::Colorize;
use frp_sh::cli::{self, Commands, DevCmd, GameCmd, LanCmd};
use frp_sh::config::Config;

/// 手动构建 runtime 并在大栈 worker 线程上运行主逻辑。
///
/// `#[tokio::main]` 的 `block_on` 会把 async main 的整个 future（包含所有
/// 分支的异步状态机）压在主线程栈上（Windows 默认仅 1MB），分支增多后
/// 会直接栈溢出（启动即崩溃）。改用大栈 worker 线程承载主逻辑即可规避。
fn main() {
    let result = run_main();
    if let Err(e) = result {
        log::error!(target: "runtime", "{e:#}");
        log::logger().flush();
        frp_sh::terminal::error("E_RUNTIME", &format!("{e:#}"));
        std::process::exit(1);
    }
    log::logger().flush();
}
fn run_main() -> anyhow::Result<()> {
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
    let args: Vec<_> = std::env::args_os().collect();
    let arg_value = |flag: &str| -> Option<String> {
        args.iter().enumerate().find_map(|(i, a)| {
            let s = a.to_string_lossy();
            if s == flag {
                args.get(i + 1).map(|v| v.to_string_lossy().into_owned())
            } else {
                s.strip_prefix(&format!("{flag}=")).map(str::to_owned)
            }
        })
    };
    let config_path = arg_value("--config")
        .or_else(|| arg_value("-c"))
        .map(std::path::PathBuf::from)
        .or_else(Config::default_path);
    let language = config_path
        .as_deref()
        .and_then(|p| Config::load(Some(p)).ok())
        .and_then(|c| c.language);
    let requested = arg_value("--lang")
        .or(language)
        .or_else(|| std::env::var("FRPSH_LANG").ok())
        .unwrap_or_else(|| "auto".into());
    frp_sh::i18n::choose(&requested);
    let has = |flag: &str| args.iter().any(|a| a == flag);
    frp_sh::terminal::configure(has("--plain"), has("--json"), has("--no-color"));
    for flag in ["--key", "--password"] {
        if let Some(v) = arg_value(flag) {
            frp_sh::debuglog::protect(&v);
        }
    }
    let mut command = frp_sh::i18n::command(cli::Cli::command());
    if has("--plain")
        || has("--no-color")
        || has("--json")
        || std::env::var_os("NO_COLOR").is_some()
    {
        command = command.color(clap::ColorChoice::Never);
    }
    let matches = match command.try_get_matches_from(args) {
        Ok(m) => m,
        Err(e) => {
            if e.exit_code() == 0 {
                frp_sh::terminal::output(e.to_string(), false);
                return Ok(());
            }
            frp_sh::terminal::error("E_ARGUMENT", &e.to_string());
            std::process::exit(2);
        }
    };
    let cli = cli::Cli::from_arg_matches(&matches)?;
    if let Some(Commands::Logs { cmd }) = &cli.command {
        match cmd {
            cli::LogCmd::Path => frp_sh::ui_println!("{}", frp_sh::debuglog::directory().display()),
            cli::LogCmd::Tail {
                lines,
                follow,
                level,
            } => frp_sh::debuglog::tail(*lines, *follow, level.as_deref()).await?,
        }
        return Ok(());
    }
    let filter = if cli.verbose { "debug" } else { "info" };
    // Diagnostic records go to bounded, rotating JSONL files.
    frp_sh::debuglog::init(filter);

    let cli::Cli {
        mut command,
        config,
        name,
        punch_retries,
        ..
    } = cli;
    command = match command {
        Some(Commands::Shortcut(values)) => {
            anyhow::ensure!(
                values.len() == 1
                    && values[0].to_str().is_some_and(|v| !v.is_empty()
                        && v.len() <= 16
                        && v.bytes().all(|c| c.is_ascii_digit())),
                "Use frp-sh <room number>, create, join, or --help"
            );
            Some(Commands::Join {
                room_id: values[0].to_string_lossy().into_owned(),
                network: true,
                listen: None,
                service: None,
                key: None,
            })
        }
        Some(Commands::Create(args)) => Some(Commands::Lan {
            cmd: LanCmd::Create(frp_sh::presets::resolve(
                args,
                &Config::load_auto(config.as_deref())?,
            )?),
        }),
        other => other,
    };
    frp_sh::config::set_punch_retries(punch_retries);

    frp_sh::config::set_cli_name(name);

    if let Some(Commands::Connect {
        invitation,
        save_only,
    }) = &command
    {
        let value = frp_sh::invite::Invitation::parse(invitation)?;
        let mut cfg = Config::load_auto(config.as_deref())?;
        let profile = value.save(&mut cfg);
        if let Some(path) = &config {
            cfg.save(path)?;
        } else {
            cfg.save_default()?;
        }
        if *save_only {
            frp_sh::ui_println!("Invitation saved");
            return Ok(());
        }
        command = Some(Commands::Profile {
            cmd: Some(cli::ProfileCmd::Run {
                name: Some(profile),
            }),
        });
    }
    frp_sh::pet::initialize(config.clone(), Config::load_auto(config.as_deref())?.pet);
    if frp_sh::terminal::interactive() {
        let page = match &command {
            None | Some(Commands::App) => Some(frp_sh::app::Page::Home),
            Some(Commands::Profile {
                cmd: None | Some(cli::ProfileCmd::List),
            }) => Some(frp_sh::app::Page::Profiles),
            Some(Commands::Pet) => Some(frp_sh::app::Page::Pets),
            Some(Commands::Preset) => Some(frp_sh::app::Page::Presets),
            Some(Commands::Config) => Some(frp_sh::app::Page::Settings),
            _ => None,
        };
        if let Some(page) = page {
            return frp_sh::app::run(config, page).await;
        }
    }
    match &mut command {
        Some(Commands::Dev {
            cmd: DevCmd::Create(args),
        }) => {
            if args.service.is_empty() && args.tcp.is_empty() && args.udp.is_empty() {
                args.service = required_endpoint("", "--service")?;
            }
            args.published()?;
        }
        Some(Commands::Game {
            cmd: GameCmd::Create(args),
        }) if !args.network() => {
            if args.service.is_empty() && args.tcp.is_empty() && args.udp.is_empty() {
                args.service = required_endpoint("", "--service")?;
            }
            args.published()?;
        }
        Some(Commands::Dev {
            cmd: DevCmd::Join(args),
        })
        | Some(Commands::Game {
            cmd: GameCmd::Join(args),
        }) if !args.listen.is_empty() => {
            args.listen = required_endpoint(&args.listen, "--listen")?;
        }
        _ => {}
    }
    let session_ui = matches!(
        frp_sh::commands::session_role(&command),
        Some("host" | "guest")
    );
    let _status = frp_sh::terminal::monitor(session_ui);
    frp_sh::terminal::banner();

    if frp_sh::commands::needs_network_helper(&command) {
        frp_sh::helper::status().await?;
    }

    // 单实例锁（按角色）：同机同时只允许一个房主会话/一个访客会话/一个服务端，
    // 防止误开多个房间；host 与 guest 各一把锁，双端同机不受影响。
    // Each process owns only its own session role.
    if let Some(role) = frp_sh::commands::session_role(&command) {
        frp_sh::commands::acquire_role_lock(role)?;
    }

    // Update checks are explicit and never delay a connection.

    match command {
        Some(
            Commands::Logs { .. }
            | Commands::Connect { .. }
            | Commands::Create(_)
            | Commands::Shortcut(_),
        ) => unreachable!(),
        Some(Commands::Pet) => {
            for id in 0..20 {
                frp_sh::ui_println!("{:02} {}", id + 1, frp_sh::pet::name(id));
            }
        }
        Some(Commands::Preset) => {
            let cfg = Config::load_auto(config.as_deref())?;
            frp_sh::ui_println!("lan / game / dev (built-in LAN presets)");
            for (name, p) in cfg.presets {
                frp_sh::ui_println!("{} · {} · MTU {} · {}s", name, p.scene, p.mtu, p.ttl);
            }
        }
        Some(Commands::App) => anyhow::bail!("The terminal app requires an interactive terminal"),
        Some(Commands::Update) => frp_sh::update::maybe_check_update(true).await?,
        Some(Commands::Doctor { network_test }) => {
            frp_sh::helper::status().await?;
            if network_test {
                let device = frp_sh::p2p::tun::create(&frp_sh::p2p::tun::TunConfig {
                    allow_lan: false,
                    name: "frp0".into(),
                    ip: "10.254.254.1".into(),
                    netmask: "255.255.255.252".into(),
                    mtu: 1400,
                })
                .await?;
                drop(device);
            }
            frp_sh::ui_println!("Network helper is ready");
        }
        #[cfg(feature = "server")]
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
            frp_sh::commands::run_profile(cmd.unwrap_or(cli::ProfileCmd::List), config).await?;
        }
        Some(Commands::Game { cmd }) => {
            check_config_hint(&config);
            match cmd {
                GameCmd::Create(args) => {
                    let cfg = Config::load_auto(config.as_deref())?;
                    if args.network() {
                        frp_sh::commands::run_create(
                            cfg,
                            args.prefix.unwrap_or_default(),
                            args.ttl,
                            "127.0.0.1:1".into(),
                            args.relay,
                            args.key,
                            0,
                            args.spread,
                            Some(frp_sh::commands::TunOpts::host_default()),
                            vec![],
                            false,
                        )
                        .await?;
                    } else {
                        let services = args.published()?;
                        frp_sh::services::host(
                            cfg,
                            args.prefix.unwrap_or_default(),
                            args.ttl,
                            services,
                            args.key,
                        )
                        .await?;
                    }
                }
                GameCmd::Join(args) => {
                    let cfg = Config::load_auto(config.as_deref())?;
                    let api = frp_sh::signaling::SignalingClient::new_with_password(
                        &cfg.signaling_addr,
                        cfg.password.as_deref(),
                    );
                    if !api.get_room(&args.room_id).await?.services.is_empty() {
                        return frp_sh::services::join(
                            cfg,
                            args.room_id,
                            (!args.listen.is_empty()).then_some(args.listen),
                            args.service,
                            args.key,
                        )
                        .await;
                    }
                    if args.listen.is_empty() {
                        frp_sh::helper::status().await?;
                    }

                    let tun = args.listen.is_empty().then(|| {
                        let mut t = frp_sh::commands::TunOpts::guest_default();
                        if let Some(id) = cfg.uuid.as_deref() {
                            t.ip = frp_sh::utils::derive_vnet_ip(id);
                        }
                        t
                    });
                    let requested = tun.as_ref().map(|t| t.ip.clone());
                    let listen = if tun.is_some() {
                        "127.0.0.1:1".into()
                    } else {
                        args.listen
                    };
                    frp_sh::commands::run_join(
                        cfg,
                        args.room_id,
                        listen,
                        args.relay,
                        args.key,
                        args.max_conns,
                        args.spread,
                        tun,
                        requested,
                        false,
                    )
                    .await?;
                }
            }
        }
        Some(Commands::Dev { cmd }) => match cmd {
            DevCmd::Create(args) => {
                let cfg = Config::load_auto(config.as_deref())?;
                let services = args.published()?;
                frp_sh::services::host(
                    cfg,
                    args.prefix.unwrap_or_default(),
                    args.ttl,
                    services,
                    args.key,
                )
                .await?;
            }
            DevCmd::Join(args) => {
                let cfg = Config::load_auto(config.as_deref())?;
                frp_sh::services::join(
                    cfg,
                    args.room_id,
                    (!args.listen.is_empty()).then_some(args.listen),
                    args.service,
                    args.key,
                )
                .await?;
            }
            DevCmd::Add {
                service,
                mut tcp,
                udp,
                label,
            } => {
                if let Some(s) = service {
                    tcp.insert(0, s);
                }
                frp_sh::services::change(
                    frp_sh::services::published(&tcp, &udp, label.as_deref())?,
                    None,
                )?;
            }
            DevCmd::Remove { service_id } => frp_sh::services::change(vec![], Some(service_id))?,
            DevCmd::Revoke => frp_sh::services::revoke().await?,
        },
        Some(Commands::Join {
            room_id,
            network: _,
            key,
            listen,
            service,
        }) => {
            let cfg = Config::load_auto(config.as_deref())?;
            let api = frp_sh::signaling::SignalingClient::new_with_password(
                &cfg.signaling_addr,
                cfg.password.as_deref(),
            );
            let info = api.get_room(&room_id).await?;
            if !info.services.is_empty() {
                frp_sh::services::join(cfg, room_id, listen, service, key.clone()).await?;
            } else {
                frp_sh::helper::status().await?;
                let mut tun = frp_sh::commands::TunOpts::guest_default();
                if let Some(id) = cfg.uuid.as_deref() {
                    tun.ip = frp_sh::utils::derive_vnet_ip(id);
                }
                let ip = Some(tun.ip.clone());
                frp_sh::commands::run_join(
                    cfg,
                    room_id,
                    "127.0.0.1:1".into(),
                    false,
                    key,
                    0,
                    2,
                    Some(tun),
                    ip,
                    false,
                )
                .await?;
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
                frp_sh::ui_println!(
                    "Welcome to frp-sh! The first run requires configuring the signaling server.\n"
                );
                frp_sh::commands::run_config(config).await?;
            } else {
                let cfg = Config::load_auto(config.as_deref())?;
                frp_sh::ui_println!("{}", "frp-sh - social P2P mesh tool".cyan().bold());
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
                frp_sh::ui_println!("{}", frp_sh::utils::kv_table(&home_rows));
                frp_sh::ui_println!("\n  {}", "Common commands:".cyan());
                frp_sh::ui_println!(
                    "    {}  mesh: create a room (virtual NIC puts the whole machine on the mesh)",
                    "frp-sh lan create".dimmed()
                );
                frp_sh::ui_println!(
                    "    {}  mesh: join a room (reach the peer's whole LAN)",
                    "frp-sh lan join 1234".dimmed()
                );
                frp_sh::ui_println!(
                    "    {}  development: application-layer port forwarding",
                    "frp-sh dev create".dimmed()
                );
                frp_sh::ui_println!(
                    "    {}  game: pure port forwarding (default 25565)",
                    "frp-sh game create".dimmed()
                );
                #[cfg(feature = "server")]
                frp_sh::ui_println!(
                    "    {}  start the signaling server",
                    "frp-sh serve".dimmed()
                );
                frp_sh::ui_println!("    {}  reconfigure", "frp-sh config".dimmed());
                frp_sh::ui_println!("    {}  show all commands\n", "frp-sh --help".dimmed());
            }
        }
    }
    Ok(())
}

/// 未配置默认服务器时给出提示（不阻塞，使用内置默认 127.0.0.1）。
fn check_config_hint(config: &Option<std::path::PathBuf>) {
    if config.is_none() && !Config::default_exists() {
        frp_sh::ui_println!(
            "Note: no signaling server configured; using the built-in default 127.0.0.1:8080.\n\
             Run `frp-sh config` to configure your server interactively.\n"
        );
    }
}

fn required_endpoint(value: &str, flag: &str) -> anyhow::Result<String> {
    use std::io::Write;
    let mut value = value.to_owned();
    if value.trim().is_empty() {
        anyhow::ensure!(
            frp_sh::terminal::interactive(),
            "missing {flag}: specify a local port, e.g. {flag} 3000"
        );
        print!(
            "{}: ",
            if frp_sh::i18n::chinese() {
                "请输入本机端口或回环地址（如 3000）"
            } else {
                "Local port or loopback address (e.g. 3000)"
            }
        );
        std::io::stdout().flush()?;
        std::io::stdin().read_line(&mut value)?;
    }
    Ok(frp_sh::access::local_endpoint(&value)?.to_string())
}
