//! 命令行定义（clap derive）。

use clap::builder::styling::{AnsiColor, Effects};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// 彩色 help 配色（clap Styles）：标题黄、命令/参数绿、占位符青、错误红。
/// `--help` / `--version` 输出自动套用，无需逐处修改。
fn cli_styles() -> clap::builder::Styles {
    clap::builder::Styles::styled()
        .header(AnsiColor::Yellow.on_default().effects(Effects::BOLD))
        .usage(AnsiColor::Yellow.on_default().effects(Effects::BOLD))
        .literal(AnsiColor::Green.on_default())
        .placeholder(AnsiColor::Cyan.on_default())
        .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
        .valid(AnsiColor::Green.on_default())
        .invalid(AnsiColor::Red.on_default())
}

#[derive(Parser, Debug)]
#[command(
    name = "frp-sh",
    version,
    about = "Social P2P tunnel: room-based UDP hole punching with relay fallback",
    styles = cli_styles()
)]
pub struct Cli {
    /// Path to the config file (TOML, see config/default.toml)
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Local web panel address (client side; binds 127.0.0.1:6793 by default)
    #[arg(long, global = true, default_value = "127.0.0.1:6793")]
    pub panel_addr: String,

    /// Disable the local web panel
    #[arg(long, global = true)]
    pub no_panel: bool,

    /// Device display name (panel/topology; default hostname, config `name` also works)
    #[arg(long, global = true)]
    pub name: Option<String>,

    /// UDP hole-punch attempts before falling back to the TCP relay
    /// (default 1: one failed punch goes straight to relay; 0 disables punching)
    #[arg(long, global = true, default_value_t = 1)]
    pub punch_retries: u32,

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
    /// Manage saved connection profiles (server panel "one-click join" writes these too)
    Profile {
        #[command(subcommand)]
        cmd: ProfileCmd,
    },
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

/// Saved connection profile management.
#[derive(Subcommand, Debug)]
pub enum ProfileCmd {
    /// List saved profiles
    List,
    /// Show one profile (password masked)
    Show { name: String },
    /// Add a profile (name defaults to profile1, profile2, ... in order)
    Add {
        /// Profile name (defaults to profileN)
        #[arg(long)]
        name: Option<String>,
        /// Signaling server URL, e.g. http://101.43.41.195:8080
        #[arg(long)]
        server: String,
        /// Room ID to join (optional: omit for a server-only profile saved by
        /// the panel's "one-click client setup"; the room is filled in later
        /// when a room join command runs)
        #[arg(long, default_value = "")]
        room: String,
        /// Server password
        #[arg(long)]
        password: Option<String>,
        /// Connection mode: lan | dev | game (default lan)
        #[arg(long, default_value = "lan")]
        mode: String,
        /// Device name shown to peers (default hostname)
        #[arg(long)]
        device: Option<String>,
        /// Relay address (default derived from the server host, :8081)
        #[arg(long)]
        relay: Option<String>,
        /// Local listen address (dev/game modes; default 127.0.0.1:25565)
        #[arg(long)]
        listen: Option<String>,
        /// lan mode: expose your LAN into the tunnel
        #[arg(long)]
        expose_lan: bool,
        /// Set as the default profile
        #[arg(long, default_value_t = true)]
        set_default: bool,
    },
    /// Edit a profile (rename / change fields)
    Edit {
        /// Profile name to edit
        name: String,
        /// New profile name
        #[arg(long)]
        rename: Option<String>,
        /// Signaling server URL
        #[arg(long)]
        server: Option<String>,
        /// Room ID
        #[arg(long)]
        room: Option<String>,
        /// Server password
        #[arg(long)]
        password: Option<String>,
        /// Device name shown to peers
        #[arg(long)]
        device: Option<String>,
        /// Relay address
        #[arg(long)]
        relay: Option<String>,
        /// Local listen address (dev/game modes)
        #[arg(long)]
        listen: Option<String>,
        /// Connection mode: lan | dev | game
        #[arg(long)]
        mode: Option<String>,
        /// lan mode: expose your LAN into the tunnel
        #[arg(long)]
        expose_lan: Option<bool>,
        /// Set as the default profile
        #[arg(long)]
        set_default: bool,
    },
    /// Remove a profile
    Remove { name: String },
    /// Start a session from a saved profile (uses the default profile if name omitted)
    Run {
        /// Profile name (optional; default profile otherwise)
        name: Option<String>,
    },
}
