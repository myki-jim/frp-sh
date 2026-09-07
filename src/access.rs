//! Local forwarding policy. Remote frames never contain a dial target.
use crate::error::{FrpError, Result};
use std::net::SocketAddr;

/// Pin an explicit numeric loopback endpoint; no DNS rebinding or remote targets.
pub fn local_endpoint(value: &str) -> Result<SocketAddr> {
    let value = value.trim();
    let normalized = if let Some(port) = value.strip_prefix("localhost:") {
        format!("127.0.0.1:{port}")
    } else if value.bytes().all(|b| b.is_ascii_digit()) && !value.is_empty() {
        format!("127.0.0.1:{value}")
    } else {
        value.to_owned()
    };
    let addr: SocketAddr = normalized.parse().map_err(|_| {
        FrpError::Config(
            "specify a local port or numeric loopback address, e.g. 3000 or 127.0.0.1:3000".into(),
        )
    })?;
    validate_local_endpoint(addr)?;
    Ok(addr)
}

pub fn validate_local_endpoint(addr: SocketAddr) -> Result<()> {
    if !addr.ip().is_loopback() || addr.port() == 0 {
        return Err(FrpError::Config(
            "service forwarding requires a nonzero loopback port; public, LAN and wildcard addresses are not allowed".into(),
        ));
    }
    Ok(())
}

pub fn require_room_mode(local_mesh: bool, remote_mesh: bool) -> Result<()> {
    if local_mesh != remote_mesh {
        return Err(FrpError::Config(if remote_mesh {
            "this is a device-network room; use lan join (this grants device-network access)".into()
        } else {
            "this is a service-only room; use dev join with an explicit --listen port".into()
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_fixed_local_and_nonzero() {
        for value in ["3000", "localhost:3000", "127.0.0.1:3000", "[::1]:3000"] {
            assert!(local_endpoint(value).is_ok(), "{value}");
        }
        for value in [
            "",
            "0",
            "0.0.0.0:3000",
            "[::]:3000",
            "192.168.1.1:80",
            "169.254.169.254:80",
            "8.8.8.8:53",
            "example.com:80",
            "[::ffff:8.8.8.8]:53",
            "127.0.0.1:65536",
        ] {
            assert!(local_endpoint(value).is_err(), "{value}");
        }
    }

    #[test]
    fn a_service_entry_never_implicitly_grants_device_access() {
        assert!(require_room_mode(false, true).is_err());
        assert!(require_room_mode(true, false).is_err());
        assert!(require_room_mode(true, true).is_ok());
        assert!(require_room_mode(false, false).is_ok());
    }
}
