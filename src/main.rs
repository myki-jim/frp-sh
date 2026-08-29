use clap::Parser;
use frp_sh::cli::{self, Commands, GameCmd};
use frp_sh::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    let filter = if cli.verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(filter)).init();

    let cli::Cli {
        command, config, ..
    } = cli;

    match command {
        Some(Commands::Serve { addr, relay_addr }) => {
            frp_sh::commands::run_serve(addr, relay_addr).await?;
        }
        Some(Commands::Config) => {
            frp_sh::commands::run_config(config).await?;
        }
        Some(Commands::Game { cmd }) => {
            // 未配置默认服务器时给出提示（不阻塞，使用内置默认 127.0.0.1）
            if config.is_none() && !Config::default_exists() {
                println!(
                    "提示：尚未配置信令服务器，正在使用内置默认 127.0.0.1:8080。\n\
                     运行 `frp-sh config` 可交互式配置你的服务器。\n"
                );
            }
            match cmd {
                GameCmd::Create {
                    prefix,
                    ttl,
                    service,
                    relay,
                    key,
                    max_conns,
                    spread,
                } => {
                    let cfg = Config::load_auto(config.as_deref())?;
                    frp_sh::commands::run_create(
                        cfg, prefix, ttl, service, relay, key, max_conns, spread,
                    )
                    .await?;
                }
                GameCmd::Join {
                    room_id,
                    relay,
                    listen,
                    key,
                    max_conns,
                    spread,
                } => {
                    let cfg = Config::load_auto(config.as_deref())?;
                    frp_sh::commands::run_join(cfg, room_id, listen, relay, key, max_conns, spread)
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
                println!("frp-sh - 社交化 P2P 打洞工具\n");
                println!("  当前配置:");
                println!("    信令服务器: {}", cfg.signaling_addr);
                println!("    中继服务器: {}", cfg.relay_addr);
                if let Some(u) = &cfg.signaling_udp {
                    println!("    UDP 探测   : {u}");
                }
                println!("\n  常用命令:");
                println!("    frp-sh game create             # 房主：创建房间");
                println!("    frp-sh game join game-xxxxxx   # 访客：加入房间");
                println!("    frp-sh serve                   # 启动信令服务器");
                println!("    frp-sh config                  # 重新配置");
                println!("    frp-sh --help                  # 查看全部命令\n");
            }
        }
    }
    Ok(())
}
