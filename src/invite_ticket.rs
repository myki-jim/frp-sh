//! v3 invitation envelope: endpoint + short-lived opaque capability, never configuration.
use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
const PREFIX: &str = "https://frp.sh/join#v3.";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    server: String,
    token: String,
}
impl Ticket {
    pub fn new(server: &str, token: &str) -> anyhow::Result<Self> {
        let value = Self {
            server: server.into(),
            token: token.into(),
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> anyhow::Result<()> {
        ensure!(self.server.len() <= 2048, "invalid invitation endpoint");
        let url = reqwest::Url::parse(&self.server)
            .map_err(|_| anyhow::anyhow!("invalid invitation endpoint"))?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "invitation requires a credential-free HTTPS endpoint"
        );
        ensure!(
            self.token.len() == 64
                && self
                    .token
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid invitation token"
        );
        Ok(())
    }
    pub fn link(&self) -> anyhow::Result<String> {
        self.validate()?;
        Ok(format!(
            "{PREFIX}{}",
            hex::encode(serde_json::to_vec(self)?)
        ))
    }
    pub fn parse(link: &str) -> anyhow::Result<Self> {
        ensure!(link.len() <= 6000, "invalid invitation");
        let encoded = link
            .strip_prefix(PREFIX)
            .context("expected a v3 invitation")?;
        let bytes = hex::decode(encoded).map_err(|_| anyhow::anyhow!("invalid invitation"))?;
        let ticket: Self =
            serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid invitation"))?;
        ticket.validate()?;
        crate::debuglog::protect(link);
        crate::debuglog::protect(&ticket.token);
        Ok(ticket)
    }
    pub fn server(&self) -> &str {
        &self.server
    }
    /// Only send this token in an HTTPS request body; never log or put it into a query string.
    pub fn token(&self) -> &str {
        &self.token
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ticket_is_only_an_endpoint_and_temporary_token() {
        let token = "ab".repeat(32);
        let ticket = Ticket::new("https://example.com", &token).unwrap();
        let link = ticket.link().unwrap();
        assert_eq!(Ticket::parse(&link).unwrap().token(), token);
        let value = serde_json::to_value(&ticket).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2);
        let malicious = serde_json::json!({"server":"https://example.com","token":token,"password":"must-not-be-shared"});
        let link = format!(
            "{PREFIX}{}",
            hex::encode(serde_json::to_vec(&malicious).unwrap())
        );
        assert!(Ticket::parse(&link).is_err());
    }
    #[test]
    fn rejects_insecure_or_credential_bearing_sources() {
        for server in [
            "http://example.com",
            "https://user:secret@example.com",
            "https://example.com?token=x",
            "https://example.com#x",
            "file:///tmp/source",
        ] {
            assert!(Ticket::new(server, &"ab".repeat(32)).is_err());
        }
    }
}
