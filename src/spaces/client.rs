use super::*;
use crate::device::key::DeviceKey;
use anyhow::ensure;
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    origin: String,
}
impl Client {
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn new(origin: &str) -> anyhow::Result<Self> {
        crate::invite_ticket::Ticket::new(origin, &"00".repeat(32))?;
        Ok(Self {
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(15))
                .build()?,
            origin: origin.into(),
        })
    }
    pub async fn execute(
        &self,
        key: &DeviceKey,
        operation: Operation,
        admin: Option<&str>,
    ) -> anyhow::Result<serde_json::Value> {
        let hash = operation.digest()?;
        let endpoint = format!("{}/spaces/v1", self.origin.trim_end_matches('/'));
        let challenge = self
            .http
            .post(format!("{endpoint}/challenge"))
            .json(&ChallengeRequest {
                device: key.public_key(),
                action_hash: hash.clone(),
            })
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("space server unavailable"))?;
        let challenge: crate::device::Challenge = Self::response(challenge).await?;
        let signature = key.sign(&challenge, &self.origin, &hash)?;
        let mut request = self
            .http
            .post(format!("{endpoint}/execute"))
            .json(&SignedRequest {
                nonce: challenge.nonce,
                signature,
                operation,
            });
        if let Some(password) = admin {
            request = request.header("X-Frp-Sh-Token", password);
        }
        Self::response(
            request
                .send()
                .await
                .map_err(|_| anyhow::anyhow!("space server unavailable"))?,
        )
        .await
    }
    async fn response<T: serde::de::DeserializeOwned>(
        mut response: reqwest::Response,
    ) -> anyhow::Result<T> {
        ensure!(
            response.status().is_success(),
            "space request rejected (HTTP {})",
            response.status().as_u16()
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow::anyhow!("invalid space response"))?
        {
            ensure!(
                bytes.len() + chunk.len() <= 512 * 1024,
                "space response too large"
            );
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid space response"))
    }
}
