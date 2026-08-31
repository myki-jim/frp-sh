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
    /// Path to the config file (TOML, see config/default.toml)
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the signaling server (standalone deployment; HTTP + UDP public probing share the same port)
    Serve {
        /// HTTP listen address
        #[arg(short, long, default_value = "0.0.0.0:8080")]
        addr: String,
        /// TCP relay listen address
        #[arg(long, default_value = "0.0.0.0:8081")]
        relay_addr: String,
        /// Separate UDP probe listen address (optional; only needed when your
        /// cloud firewall can't open TCP+UDP on the same port)
        #[arg(long)]
        udp_addr: Option<String>,
        /// Server password (optional): clients must configure the same password;
        /// enables request authentication and relay traffic encryption
        #[arg(long)]
        password: Option<String>,
        /// Built-in TURN relay listen address (optional, e.g. 0.0.0.0:3478);
        /// provides RFC 5766 TURN relaying using the same --password
        #[arg(long)]
        turn: Option<String>,
        /// Public IP advertised in TURN relay addresses (optional; needed when
        /// the server is behind NAT and clients reach it via a public IP)
        #[arg(long)]
        external_ip: Option<String>,
    },
    /// Configure the signaling server interactively (first-run wizard)
    Config,
    /// game multiplayer: application-layer port forwarding (e.g. Minecraft), pure forwarding without meshing
    Game {
        #[command(subcommand)]
        cmd: GameCmd,
    },
    /// development: application-layer port forwarding (any TCP service)
    Dev {
        #[command(subcommand)]
        cmd: DevCmd,
    },
    /// mesh (Tailscale-like): virtual NIC puts the whole machine on the network, can reach the peer's entire LAN
    Lan {
        #[command(subcommand)]
        cmd: LanCmd,
    },
}

/// Common parameters for port-forwarding modes (shared by game / dev).
#[derive(clap::Args, Debug)]
pub struct ForwardCreateArgs {
    /// Room prefix (optional; default: none — plain 4-digit code like 4832)
    #[arg(short, long)]
    pub prefix: Option<String>,
    /// Room lifetime in seconds (default 12 hours)
    #[arg(short, long, default_value_t = 12 * 3600)]
    pub ttl: u64,
    /// Local service address (traffic is forwarded to it once the tunnel is up)
    #[arg(long, default_value = "127.0.0.1:25565")]
    pub service: String,
    /// Skip hole punching and use the relay directly
    #[arg(long)]
    pub relay: bool,
    /// Shared passphrase: using the same passphrase on both sides enables end-to-end encryption (ChaCha20-Poly1305)
    #[arg(long)]
    pub key: Option<String>,
    /// Maximum accepted connections (0 = unlimited, default unlimited)
    #[arg(long, default_value_t = 0)]
    pub max_conns: u64,
    /// Hole-punching port spread range (lightweight port prediction, default ±2)
    #[arg(long, default_value_t = 2)]
    pub spread: u32,
}

#[derive(clap::Args, Debug)]
pub struct ForwardJoinArgs {
    /// Room ID, e.g. game-a3f9c2
    pub room_id: String,
    /// Force relay mode
    #[arg(short, long)]
    pub relay: bool,
    /// Local listen address (players/programs connect to this port)
    #[arg(long, default_value = "127.0.0.1:25565")]
    pub listen: String,
    /// Shared passphrase: must match the host to enable end-to-end encryption
    #[arg(long)]
    pub key: Option<String>,
    /// Maximum connections to establish (0 = unlimited, default unlimited)
    #[arg(long, default_value_t = 0)]
    pub max_conns: u64,
    /// Hole-punching port spread range (default ±2)
    #[arg(long, default_value_t = 2)]
    pub spread: u32,
}

#[derive(Subcommand, Debug)]
pub enum GameCmd {
    /// Create a room (host)
    Create(ForwardCreateArgs),
    /// Join a room (guest)
    Join(ForwardJoinArgs),
}

#[derive(Subcommand, Debug)]
pub enum DevCmd {
    /// Create a room (host)
    Create(ForwardCreateArgs),
    /// Join a room (guest)
    Join(ForwardJoinArgs),
}

/// Common parameters for mesh mode (lan).
#[derive(clap::Args, Debug)]
pub struct LanCreateArgs {
    /// Room prefix (optional; default: none — plain 4-digit code like 4832)
    #[arg(short, long)]
    pub prefix: Option<String>,
    /// Room lifetime in seconds (default 12 hours)
    #[arg(short, long, default_value_t = 12 * 3600)]
    pub ttl: u64,
    /// Skip hole punching and use the relay directly
    #[arg(long)]
    pub relay: bool,
    /// Shared passphrase: using the same passphrase on both sides enables end-to-end encryption
    #[arg(long)]
    pub key: Option<String>,
    /// Hole-punching port spread range (default ±2)
    #[arg(long, default_value_t = 2)]
    pub spread: u32,
    /// Virtual NIC IP (default: host 10.66.0.1)
    #[arg(long)]
    pub ip: Option<String>,
    /// Virtual NIC netmask
    #[arg(long, default_value = "255.255.255.0")]
    pub netmask: String,
    /// Virtual NIC MTU
    #[arg(long, default_value_t = 1400)]
    pub mtu: u16,
    /// Reserved guest virtual IP pool (comma-separated, e.g. 10.66.0.2,10.66.0.3);
    /// guests without --ip are assigned in join order, and reconnecting devices reuse the same IP
    #[arg(long, value_delimiter = ',')]
    pub guest_ips: Vec<String>,
    /// Expose your LAN into the tunnel (the peer can reach your LAN devices).
    /// Off by default: only the virtual subnet is shared, your local network stays hidden
    #[arg(long)]
    pub expose_lan: bool,
}

#[derive(clap::Args, Debug)]
pub struct LanJoinArgs {
    /// Room ID, e.g. lan-a3f9c2
    pub room_id: String,
    /// Force relay mode
    #[arg(short, long)]
    pub relay: bool,
    /// Shared passphrase: must match the host to enable end-to-end encryption
    #[arg(long)]
    pub key: Option<String>,
    /// Hole-punching port spread range (default ±2)
    #[arg(long, default_value_t = 2)]
    pub spread: u32,
    /// Virtual NIC IP (default: derived stably from the device ID, e.g. 10.66.0.42)
    #[arg(long)]
    pub ip: Option<String>,
    /// Virtual NIC netmask
    #[arg(long, default_value = "255.255.255.0")]
    pub netmask: String,
    /// Virtual NIC MTU
    #[arg(long, default_value_t = 1400)]
    pub mtu: u16,
    /// Expose your LAN into the tunnel (the peer can reach your LAN devices).
    /// Off by default: only the virtual subnet is shared, your local network stays hidden
    #[arg(long)]
    pub expose_lan: bool,
}

#[derive(Subcommand, Debug)]
pub enum LanCmd {
    /// Create a room (host)
    Create(LanCreateArgs),
    /// Join a room (guest)
    Join(LanJoinArgs),
}
