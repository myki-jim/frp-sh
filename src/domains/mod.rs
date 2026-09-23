//! Device-owned public HTTP domains and a constrained reverse HTTP tunnel.

use anyhow::ensure;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub mod cli;
pub mod client;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
mod storage;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Bind { domain: String },
    Verify { domain: String },
    Status { domain: String },
    Unbind { domain: String },
    OpenPublisher { domain: String },
}

impl Operation {
    pub fn digest(&self) -> anyhow::Result<String> {
        let mut hash = Sha256::new();
        hash.update(b"frp-sh/domain-operation/v1\0");
        hash.update(serde_json::to_vec(self)?);
        Ok(hex::encode(hash.finalize()))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChallengeRequest {
    pub device: String,
    pub action_hash: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedRequest {
    pub nonce: String,
    pub signature: String,
    pub operation: Operation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TunnelRequest {
    pub id: String,
    pub host: String,
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TunnelResponse {
    pub id: String,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

pub fn normalize_domain(input: &str) -> anyhow::Result<String> {
    let value = input.trim().trim_end_matches('.');
    ensure!(
        !value.is_empty() && value.len() <= 253 && value.is_ascii(),
        "domain must be an ASCII hostname"
    );
    ensure!(!value.contains('*'), "wildcard domains are not accepted");
    let url = reqwest::Url::parse(&format!("http://{value}/"))?;
    ensure!(
        url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url.path() == "/"
            && url.query().is_none()
            && url.fragment().is_none(),
        "invalid domain"
    );
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("invalid domain"))?
        .to_ascii_lowercase();
    ensure!(
        host.parse::<std::net::IpAddr>().is_err(),
        "IP addresses are not domains"
    );
    ensure!(
        host.contains('.')
            && !host.ends_with(".local")
            && !host.ends_with(".localhost")
            && !host.ends_with(".internal")
            && !host.ends_with(".invalid"),
        "a public domain is required"
    );
    for label in host.split('.') {
        ensure!(
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "invalid domain label"
        );
    }
    Ok(host)
}

pub fn verification_name(domain: &str, token: &str) -> anyhow::Result<String> {
    ensure!(
        token.len() >= 12 && token.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid verification token"
    );
    Ok(format!("_frpsh-verify-{}.{domain}", &token[..12]))
}

pub fn action_token(token: &str) -> String {
    format!("frpsh-verify={token}")
}

pub fn loopback_target(input: &str) -> anyhow::Result<String> {
    let value = if input.contains("://") {
        input.to_owned()
    } else {
        format!("http://{input}")
    };
    let url = reqwest::Url::parse(&value)?;
    ensure!(url.scheme() == "http", "local target must use HTTP");
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "invalid local target"
    );
    let host = url.host_str().unwrap_or_default();
    ensure!(
        matches!(host, "127.0.0.1" | "localhost" | "::1"),
        "local target must be loopback"
    );
    ensure!(url.port().is_some(), "local target requires a port");
    let mut normalized = url;
    normalized.set_path("");
    Ok(normalized.to_string().trim_end_matches('/').to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains_are_canonical_and_public() {
        assert_eq!(
            normalize_domain("App.Example.COM.").unwrap(),
            "app.example.com"
        );
        for invalid in ["localhost", "127.0.0.1", "*.example.com", "a..example.com"] {
            assert!(normalize_domain(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn targets_are_loopback_http_only() {
        assert_eq!(
            loopback_target("127.0.0.1:3000").unwrap(),
            "http://127.0.0.1:3000"
        );
        assert!(loopback_target("http://192.168.1.2:80").is_err());
        assert!(loopback_target("https://127.0.0.1:3000").is_err());
    }
}
