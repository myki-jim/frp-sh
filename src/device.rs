//! Device possession proofs. Challenges bind an action digest and a server identity.
use anyhow::{ensure, Context};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
pub mod key;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub nonce: String,
    pub server: String,
    pub device: String,
    pub action_hash: String,
    pub expires_at: u64,
}
impl Challenge {
    pub fn message(&self) -> anyhow::Result<Vec<u8>> {
        let mut message = b"frp-sh/device-proof/v1\0".to_vec();
        message.extend(serde_json::to_vec(self)?);
        Ok(message)
    }
}
#[derive(Default)]
pub struct Challenges {
    pending: HashMap<String, Challenge>,
}
/// Cannot be built from an unverified request identity.
pub struct VerifiedDevice(String);
impl VerifiedDevice {
    pub fn public_key(&self) -> &str {
        &self.0
    }
}

pub fn public_key(text: &str) -> anyhow::Result<VerifyingKey> {
    ensure!(
        text.len() == 64
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid device identity"
    );
    let bytes: [u8; 32] = hex::decode(text)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid device identity"))?;
    let key =
        VerifyingKey::from_bytes(&bytes).map_err(|_| anyhow::anyhow!("invalid device identity"))?;
    ensure!(!key.is_weak(), "weak device identity");
    Ok(key)
}
impl Challenges {
    pub fn issue(
        &mut self,
        server: &str,
        device: &str,
        action_hash: &str,
        now: u64,
    ) -> anyhow::Result<Challenge> {
        public_key(device)?;
        ensure!(
            server.len() <= 128 && !server.is_empty(),
            "invalid server identity"
        );
        ensure!(
            action_hash.len() == 64 && action_hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid action hash"
        );
        self.pending.retain(|_, v| v.expires_at > now);
        ensure!(self.pending.len() < 1024, "challenge capacity reached");
        let challenge = Challenge {
            nonce: crate::utils::random_hex(32),
            server: server.into(),
            device: device.into(),
            action_hash: action_hash.into(),
            expires_at: now.checked_add(60).context("invalid clock")?,
        };
        self.pending
            .insert(challenge.nonce.clone(), challenge.clone());
        Ok(challenge)
    }
    /// Invoke while holding the registry lock. A challenge is consumed on every attempt.
    pub fn verify(
        &mut self,
        nonce: &str,
        signature: &str,
        action_hash: &str,
        now: u64,
    ) -> anyhow::Result<VerifiedDevice> {
        ensure!(
            nonce.len() == 64 && signature.len() == 128,
            "invalid device proof"
        );
        let challenge = self
            .pending
            .remove(nonce)
            .context("device challenge unavailable")?;
        ensure!(
            now < challenge.expires_at && challenge.action_hash == action_hash,
            "expired or mismatched device proof"
        );
        let bytes = hex::decode(signature).context("invalid device proof")?;
        let signature =
            Signature::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid device proof"))?;
        public_key(&challenge.device)?
            .verify_strict(&challenge.message()?, &signature)
            .map_err(|_| anyhow::anyhow!("invalid device proof"))?;
        Ok(VerifiedDevice(challenge.device))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    #[test]
    fn proof_is_action_bound_and_cannot_replay() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let device = hex::encode(key.verifying_key().to_bytes());
        let mut challenges = Challenges::default();
        let action = "ab".repeat(32);
        let c = challenges.issue("server", &device, &action, 100).unwrap();
        let sig = hex::encode(key.sign(&c.message().unwrap()).to_bytes());
        assert_eq!(
            challenges
                .verify(&c.nonce, &sig, &action, 159)
                .unwrap()
                .public_key(),
            device
        );
        assert!(challenges.verify(&c.nonce, &sig, &action, 159).is_err());
        let c = challenges.issue("server", &device, &action, 100).unwrap();
        let sig = hex::encode(key.sign(&c.message().unwrap()).to_bytes());
        assert!(challenges
            .verify(&c.nonce, &sig, &"cd".repeat(32), 110)
            .is_err());
        let c = challenges.issue("server", &device, &action, 100).unwrap();
        let sig = hex::encode(key.sign(&c.message().unwrap()).to_bytes());
        assert!(challenges.verify(&c.nonce, &sig, &action, 160).is_err());
    }
}
