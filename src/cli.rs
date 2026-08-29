//! 命令行定义（clap derive）。

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "frp-sh",
    version,
    about = "Social P2P tunnel: room-based UDP hole punching with relay fallback"
)]
pub struct Cli {
    /// 配置文件路径（TOML，见 config/default.toml）
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// 详细日志
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 启动信令服务器（独立部署；HTTP + UDP 公网探测同端口）
    Serve {
        /// HTTP 监听地址
        #[arg(short, long, default_value = "0.0.0.0:8080")]
        addr: String,
        /// TCP 中继监听地址
        #[arg(long, default_value = "0.0.0.0:8081")]
        relay_addr: String,
    },
    /// 交互式配置信令服务器（首次运行向导）
    Config,
    /// 社交组网子命令
    Game {
        #[command(subcommand)]
        cmd: GameCmd,
    },
}

#[derive(Subcommand, Debug)]
pub enum GameCmd {
    /// 创建房间（房主）
    Create {
        /// 房间前缀，如 game / dev / lan
        #[arg(short, long, default_value = "game")]
        prefix: String,
        /// 房间有效时长（秒），默认 12 小时
        #[arg(short, long, default_value_t = 12 * 3600)]
        ttl: u64,
        /// 房主本地服务地址（隧道打通后向其转发流量）
        #[arg(long, default_value = "127.0.0.1:25565")]
        service: String,
        /// 跳过打洞，直接使用中继
        #[arg(long)]
        relay: bool,
        /// 共享口令：双方使用相同口令即启用端到端加密（ChaCha20-Poly1305）
        #[arg(long)]
        key: Option<String>,
        /// 最多接受的连接数（0 = 无限，默认无限）
        #[arg(long, default_value_t = 0)]
        max_conns: u64,
        /// 打洞端口散布范围（轻量端口预测，默认 ±2）
        #[arg(long, default_value_t = 2)]
        spread: u32,
        /// 虚拟网卡模式：创建 TUN 设备接入对端虚拟局域网（替代端口转发，需 root/管理员）
        #[arg(long)]
        tun: bool,
        /// TUN 虚拟 IP（默认房主 10.66.0.1）
        #[arg(long)]
        tun_ip: Option<String>,
        /// TUN 子网掩码
        #[arg(long, default_value = "255.255.255.0")]
        tun_netmask: String,
        /// TUN MTU
        #[arg(long, default_value_t = 1400)]
        tun_mtu: u16,
    },
    /// 加入房间（访客）
    Join {
        /// 房间号，如 game-a3f9c2
        room_id: String,
        /// 强制使用中继模式
        #[arg(short, long)]
        relay: bool,
        /// 访客本地监听地址（玩家连接此端口）
        #[arg(long, default_value = "127.0.0.1:25565")]
        listen: String,
        /// 共享口令：与房主一致即启用端到端加密
        #[arg(long)]
        key: Option<String>,
        /// 最多建立的连接数（0 = 无限，默认无限）
        #[arg(long, default_value_t = 0)]
        max_conns: u64,
        /// 打洞端口散布范围（默认 ±2）
        #[arg(long, default_value_t = 2)]
        spread: u32,
        /// 虚拟网卡模式：创建 TUN 设备接入对端虚拟局域网（替代端口转发，需 root/管理员）
        #[arg(long)]
        tun: bool,
        /// TUN 虚拟 IP（默认访客 10.66.0.2）
        #[arg(long)]
        tun_ip: Option<String>,
        /// TUN 子网掩码
        #[arg(long, default_value = "255.255.255.0")]
        tun_netmask: String,
        /// TUN MTU
        #[arg(long, default_value_t = 1400)]
        tun_mtu: u16,
    },
}
