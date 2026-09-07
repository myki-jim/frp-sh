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
    /// Display language (auto follows the system locale)
    #[arg(long, global = true, value_parser = ["auto", "zh-CN", "en"])]
    pub lang: Option<String>,
    /// Disable animated terminal output
    #[arg(long, global = true)]
    pub plain: bool,
    /// Disable colors
    #[arg(long, global = true)]
    pub no_color: bool,
    /// Emit machine-readable JSON events
    #[arg(long, global = true)]
    pub json: bool,
    /// Path to the config file (TOML, see config/default.toml)
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Device display name (connection status; default hostname, config `name` also works)
    #[arg(long, global = true)]
    pub name: Option<String>,

    /// UDP hole-punch attempts before relay fallback (TURN, then TCP)
    /// (default 1; does not disable TURN; use --relay to force TCP)
    #[arg(long, global = true, default_value_t = 1)]
    pub punch_retries: u32,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Open the keyboard-operated terminal app
    App,
    /// Join according to the room capabilities; --network explicitly permits device access
    Join {
        room_id: String,
        #[arg(long)]
        network: bool,
        #[arg(long)]
        listen: Option<String>,
        #[arg(long)]
        service: Option<u16>,
    },
    /// Install a saved invitation and join its room
    Connect {
        invitation: String,
        /// Save the connection without joining
        #[arg(long)]
        save_only: bool,
    },
    /// Check the installed network helper
    Doctor {
        /// Create and close a temporary virtual adapter (requires no active LAN session)
        #[arg(long)]
        network_test: bool,
    },
    /// Read diagnostic logs (separate from connection status)
    Logs {
        #[command(subcommand)]
        cmd: LogCmd,
    },
    /// Check for a newer release explicitly
    Update,
    /// Start the signaling server (standalone deployment; HTTP + UDP public probing share the same port)
    #[cfg(feature = "server")]
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
        /// Server password (required for public listeners): clients configure the same password;
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
    /// Manage saved connection profiles
    Profile {
        #[command(subcommand)]
        cmd: Option<ProfileCmd>,
    },
    /// Game multiplayer: virtual LAN by default; --service shares a single TCP server
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

#[derive(Subcommand, Debug)]
pub enum LogCmd {
    /// Print the log directory
    Path,
    /// Read the most recent log file
    Tail {
        #[arg(short, long, default_value_t = 50)]
        lines: usize,
        #[arg(short, long)]
        follow: bool,
        #[arg(long, value_parser = ["error", "warn", "info", "debug", "trace"])]
        level: Option<String>,
    },
}

/// Common parameters for port-forwarding modes (shared by game / dev).
#[derive(clap::Args, Debug)]
pub struct ForwardCreateArgs {
    /// Game room kind; LAN needs no game port
    #[arg(long, value_parser=["lan", "server"])]
    pub kind: Option<String>,
    /// Publish additional local TCP ports (repeatable)
    #[arg(long)]
    pub tcp: Vec<String>,
    /// Publish local UDP ports (repeatable)
    #[arg(long)]
    pub udp: Vec<String>,
    #[arg(long)]
    pub label: Option<String>,
    /// Room prefix (optional; default: none — plain 4-digit code like 4832)
    #[arg(short, long)]
    pub prefix: Option<String>,
    /// Room lifetime in seconds (default 12 hours)
    #[arg(short, long, default_value_t = 12 * 3600)]
    pub ttl: u64,
    /// Explicit local loopback TCP service or port (e.g. 3000); game defaults to LAN when omitted
    #[arg(long, default_value = "")]
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
    #[arg(long)]
    pub service: Option<u16>,
    /// Room ID, e.g. game-a3f9c2
    pub room_id: String,
    /// Force relay mode
    #[arg(short, long)]
    pub relay: bool,
    /// Local loopback TCP listen address or port; game defaults to LAN when omitted
    #[arg(long, default_value = "")]
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
    /// Add local services to the active hosted room
    Add {
        #[arg(long)]
        service: Option<String>,
        #[arg(long)]
        tcp: Vec<String>,
        #[arg(long)]
        udp: Vec<String>,
        #[arg(long)]
        label: Option<String>,
    },
    /// Remove a published service (terminates existing streams)
    Remove { service_id: u16 },
    /// Revoke every current invitation and disconnect members
    Revoke,
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
        /// a saved server profile; the room is filled in later
        /// when a room join command runs)
        #[arg(long, default_value = "")]
        room: String,
        /// Server password
        #[arg(long)]
        password: Option<String>,
        /// End-to-end key (shared by room members)
        #[arg(long)]
        key: Option<String>,
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
        /// End-to-end key (shared by room members)
        #[arg(long)]
        key: Option<String>,
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

impl ForwardCreateArgs {
    pub fn network(&self) -> bool {
        self.kind.as_deref() != Some("server")
            && self.service.is_empty()
            && self.tcp.is_empty()
            && self.udp.is_empty()
    }
    pub fn published(&self) -> anyhow::Result<Vec<crate::services::Published>> {
        anyhow::ensure!(
            self.kind.as_deref() != Some("lan"),
            "--kind lan does not accept service ports"
        );
        anyhow::ensure!(self.max_conns==0,"--max-conns belongs to legacy forwarding; service rooms enforce per-member and room-wide concurrency limits");
        let mut tcp = self.tcp.clone();
        if !self.service.is_empty() {
            tcp.insert(0, self.service.clone());
        }
        crate::services::published(&tcp, &self.udp, self.label.as_deref())
    }
}
