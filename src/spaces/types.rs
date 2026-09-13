//! Sensitive connection responses intentionally have no Debug implementation.
use std::net::Ipv4Addr;
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub access_token: String,
    pub session_id: String,
    pub space: String,
    pub device: String,
    pub address: Ipv4Addr,
    pub expires_at: u64,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    pub device: String,
    pub session_id: String,
    pub address: Ipv4Addr,
    pub key: [u8; 32],
}
