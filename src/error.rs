//! 统一的错误类型。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum FrpError {
    #[error("signaling server error: {0}")]
    Signaling(String),

    #[error("room not found or expired: {0}")]
    RoomNotFound(String),

    #[error("UDP hole punch failed: {0}")]
    HolePunch(String),

    #[error("relay error: {0}")]
    Relay(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("config error: {0}")]
    Config(String),
}

pub type Result<T> = std::result::Result<T, FrpError>;
