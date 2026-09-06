//! frp-sh：社交化 P2P 打洞工具。
//!
//! 房间制 UDP 打洞（PUNCH/ACK 同时握手）+ TCP 中继回退，
//! 全部逻辑自包含，不依赖 frp / libp2p / webrtc 等第三方穿透库。

extern crate self as frp_sh;
pub use frpsh_ui_macros::{ui_format, ui_print, ui_println};
pub mod cli;
pub mod commands;
pub mod config;
pub mod debuglog;
pub mod error;
pub mod helper;
pub mod i18n;
pub mod p2p;
pub mod room;
pub mod signaling;
pub mod stats;
pub mod terminal;
pub mod tunnel;
pub mod update;
pub mod utils;
pub mod version;
