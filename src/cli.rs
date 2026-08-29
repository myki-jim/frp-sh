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
    /// 游戏联机：应用层端口转发（如 Minecraft），纯转发不组网
    Game {
        #[command(subcommand)]
        cmd: GameCmd,
    },
    /// 开发调试：应用层端口转发（任意 TCP 服务）
    Dev {
        #[command(subcommand)]
        cmd: DevCmd,
    },
    /// 组网（类 Tailscale）：虚拟网卡整机入网，可访问对方整个局域网
    Lan {
        #[command(subcommand)]
        cmd: LanCmd,
    },
}

/// 端口转发模式的公共参数（game / dev 共用）。
#[derive(clap::Args, Debug)]
pub struct ForwardCreateArgs {
    /// 房间前缀（game 系列默认 game；dev 系列默认 dev）
    #[arg(short, long)]
    pub prefix: Option<String>,
    /// 房间有效时长（秒），默认 12 小时
    #[arg(short, long, default_value_t = 12 * 3600)]
    pub ttl: u64,
    /// 本机服务地址（隧道打通后向其转发流量）
    #[arg(long, default_value = "127.0.0.1:25565")]
    pub service: String,
    /// 跳过打洞，直接使用中继
    #[arg(long)]
    pub relay: bool,
    /// 共享口令：双方使用相同口令即启用端到端加密（ChaCha20-Poly1305）
    #[arg(long)]
    pub key: Option<String>,
    /// 最多接受的连接数（0 = 无限，默认无限）
    #[arg(long, default_value_t = 0)]
    pub max_conns: u64,
    /// 打洞端口散布范围（轻量端口预测，默认 ±2）
    #[arg(long, default_value_t = 2)]
    pub spread: u32,
}

#[derive(clap::Args, Debug)]
pub struct ForwardJoinArgs {
    /// 房间号，如 game-a3f9c2
    pub room_id: String,
    /// 强制使用中继模式
    #[arg(short, long)]
    pub relay: bool,
    /// 本地监听地址（玩家/程序连接此端口）
    #[arg(long, default_value = "127.0.0.1:25565")]
    pub listen: String,
    /// 共享口令：与房主一致即启用端到端加密
    #[arg(long)]
    pub key: Option<String>,
    /// 最多建立的连接数（0 = 无限，默认无限）
    #[arg(long, default_value_t = 0)]
    pub max_conns: u64,
    /// 打洞端口散布范围（默认 ±2）
    #[arg(long, default_value_t = 2)]
    pub spread: u32,
}

#[derive(Subcommand, Debug)]
pub enum GameCmd {
    /// 创建房间（房主）
    Create(ForwardCreateArgs),
    /// 加入房间（访客）
    Join(ForwardJoinArgs),
}

#[derive(Subcommand, Debug)]
pub enum DevCmd {
    /// 创建房间（房主）
    Create(ForwardCreateArgs),
    /// 加入房间（访客）
    Join(ForwardJoinArgs),
}

/// 组网模式公共参数（lan）。
#[derive(clap::Args, Debug)]
pub struct LanCreateArgs {
    /// 房间前缀（默认 lan）
    #[arg(short, long)]
    pub prefix: Option<String>,
    /// 房间有效时长（秒），默认 12 小时
    #[arg(short, long, default_value_t = 12 * 3600)]
    pub ttl: u64,
    /// 跳过打洞，直接使用中继
    #[arg(long)]
    pub relay: bool,
    /// 共享口令：双方使用相同口令即启用端到端加密
    #[arg(long)]
    pub key: Option<String>,
    /// 打洞端口散布范围（默认 ±2）
    #[arg(long, default_value_t = 2)]
    pub spread: u32,
    /// 虚拟网卡 IP（默认房主 10.66.0.1）
    #[arg(long)]
    pub ip: Option<String>,
    /// 虚拟网卡子网掩码
    #[arg(long, default_value = "255.255.255.0")]
    pub netmask: String,
    /// 虚拟网卡 MTU
    #[arg(long, default_value_t = 1400)]
    pub mtu: u16,
    /// 预留的访客虚拟 IP 池（逗号分隔，如 10.66.0.2,10.66.0.3）；
    /// 访客未指定 --ip 时按加入顺序分配，同一设备重连复用同一 IP
    #[arg(long, value_delimiter = ',')]
    pub guest_ips: Vec<String>,
}

#[derive(clap::Args, Debug)]
pub struct LanJoinArgs {
    /// 房间号，如 lan-a3f9c2
    pub room_id: String,
    /// 强制使用中继模式
    #[arg(short, long)]
    pub relay: bool,
    /// 共享口令：与房主一致即启用端到端加密
    #[arg(long)]
    pub key: Option<String>,
    /// 打洞端口散布范围（默认 ±2）
    #[arg(long, default_value_t = 2)]
    pub spread: u32,
    /// 虚拟网卡 IP（默认由设备 ID 稳定派生，如 10.66.0.42）
    #[arg(long)]
    pub ip: Option<String>,
    /// 虚拟网卡子网掩码
    #[arg(long, default_value = "255.255.255.0")]
    pub netmask: String,
    /// 虚拟网卡 MTU
    #[arg(long, default_value_t = 1400)]
    pub mtu: u16,
}

#[derive(Subcommand, Debug)]
pub enum LanCmd {
    /// 创建房间（房主）
    Create(LanCreateArgs),
    /// 加入房间（访客）
    Join(LanJoinArgs),
}
