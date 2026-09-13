//! Bounded short-lived access leases. Raw bearer tokens are never retained.
use crate::device::VerifiedDevice;
use anyhow::{ensure, Context};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, net::Ipv4Addr, sync::Arc};
use tokio_util::sync::CancellationToken;

pub const ACCESS_TTL: u64 = 600;
type Member = (String, String);
#[derive(Clone)]
pub struct Lease {
    pub space: String,
    pub device: Arc<VerifiedDevice>,
    pub session_id: String,
    pub address: Ipv4Addr,
    pub expires_at: u64,
    pub cancelled: CancellationToken,
}
struct Entry {
    hash: [u8; 32],
    lease: Lease,
}
pub use super::types::Admission;
pub struct Leases {
    root_key: [u8; 32],
    maximum: usize,
    members: HashMap<Member, Entry>,
    tokens: HashMap<[u8; 32], Member>,
}
impl Leases {
    pub fn new(maximum: usize) -> Self {
        use rand::RngCore;
        let mut root_key = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut root_key);
        Self {
            root_key,
            maximum,
            members: HashMap::new(),
            tokens: HashMap::new(),
        }
    }
    pub fn peers(
        &self,
        space: &str,
        device: &VerifiedDevice,
        session: &str,
        now: u64,
    ) -> anyhow::Result<Vec<super::types::Peer>> {
        use hmac::{Hmac, Mac};
        let local = self
            .members
            .get(&(space.to_owned(), device.public_key().to_owned()))
            .context("session unavailable")?;
        ensure!(
            local.lease.session_id == session
                && local.lease.expires_at > now
                && !local.lease.cancelled.is_cancelled(),
            "session expired or replaced"
        );
        self.members
            .values()
            .filter(|entry| {
                entry.lease.space == space
                    && entry.lease.session_id != session
                    && entry.lease.expires_at > now
                    && !entry.lease.cancelled.is_cancelled()
            })
            .map(|entry| {
                let remote = &entry.lease;
                let mut ids = [session, remote.session_id.as_str()];
                ids.sort_unstable();
                let context = serde_json::to_vec(&("frp-sh/peer-key/v1", space, ids))?;
                let mut mac = Hmac::<Sha256>::new_from_slice(&self.root_key).expect("HMAC key");
                mac.update(&context);
                Ok(super::types::Peer {
                    device: remote.device.public_key().into(),
                    session_id: remote.session_id.clone(),
                    address: remote.address,
                    key: mac.finalize().into_bytes().into(),
                })
            })
            .collect()
    }
    pub fn issue(
        &mut self,
        space: String,
        device: Arc<VerifiedDevice>,
        address: Ipv4Addr,
        now: u64,
    ) -> anyhow::Result<Admission> {
        self.sweep(now);
        let member = (space.clone(), device.public_key().to_owned());
        ensure!(
            self.members.contains_key(&member) || self.members.len() < self.maximum,
            "access lease capacity reached"
        );
        self.remove(&member);
        use rand::RngCore;
        let mut secret = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut secret);
        let token = hex::encode(secret);
        let hash = Sha256::digest(token.as_bytes()).into();
        let lease = Lease {
            space: space.clone(),
            device,
            address,
            session_id: uuid::Uuid::new_v4().to_string(),
            expires_at: now.checked_add(ACCESS_TTL).context("invalid lease clock")?,
            cancelled: CancellationToken::new(),
        };
        let admission = Admission {
            access_token: token,
            session_id: lease.session_id.clone(),
            space,
            device: lease.device.public_key().into(),
            address,
            expires_at: lease.expires_at,
        };
        self.tokens.insert(hash, member.clone());
        self.members.insert(member, Entry { hash, lease });
        Ok(admission)
    }
    pub fn lookup(&mut self, token: &str, space: &str, now: u64) -> anyhow::Result<Lease> {
        ensure!(
            token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid access token"
        );
        let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let member = self
            .tokens
            .get(&hash)
            .context("unknown access token")?
            .clone();
        let entry = self
            .members
            .get(&member)
            .context("access lease unavailable")?;
        if entry.lease.expires_at <= now {
            self.remove(&member);
            anyhow::bail!("access lease expired");
        }
        ensure!(
            entry.lease.space == space && !entry.lease.cancelled.is_cancelled(),
            "access scope mismatch"
        );
        Ok(entry.lease.clone())
    }
    pub fn refresh(
        &mut self,
        space: &str,
        device: &VerifiedDevice,
        session: &str,
        now: u64,
    ) -> anyhow::Result<u64> {
        let member = (space.to_owned(), device.public_key().to_owned());
        let entry = self
            .members
            .get_mut(&member)
            .context("access lease unavailable")?;
        ensure!(
            entry.lease.session_id == session
                && entry.lease.expires_at > now
                && !entry.lease.cancelled.is_cancelled(),
            "access lease expired or replaced"
        );
        entry.lease.expires_at = now.checked_add(ACCESS_TTL).context("invalid lease clock")?;
        Ok(entry.lease.expires_at)
    }
    pub fn close(
        &mut self,
        space: &str,
        device: &VerifiedDevice,
        session: &str,
    ) -> anyhow::Result<()> {
        let member = (space.to_owned(), device.public_key().to_owned());
        let entry = self
            .members
            .get(&member)
            .context("access lease unavailable")?;
        ensure!(entry.lease.session_id == session, "access session replaced");
        self.remove(&member);
        Ok(())
    }
    pub fn revoke_member(&mut self, space: &str, device: &str) {
        self.remove(&(space.to_owned(), device.to_owned()));
    }
    pub fn revoke_space(&mut self, space: &str) {
        let members: Vec<_> = self
            .members
            .keys()
            .filter(|(s, _)| s == space)
            .cloned()
            .collect();
        for member in members {
            self.remove(&member);
        }
    }
    fn remove(&mut self, member: &Member) {
        if let Some(entry) = self.members.remove(member) {
            entry.lease.cancelled.cancel();
            self.tokens.remove(&entry.hash);
        }
    }
    pub fn sweep(&mut self, now: u64) {
        let expired: Vec<_> = self
            .members
            .iter()
            .filter(|(_, e)| e.lease.expires_at <= now)
            .map(|(m, _)| m.clone())
            .collect();
        for member in expired {
            self.remove(&member);
        }
    }
}
impl Drop for Leases {
    fn drop(&mut self) {
        for entry in self.members.values() {
            entry.lease.cancelled.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn device(n: u8) -> Arc<VerifiedDevice> {
        use ed25519_dalek::{Signer, SigningKey};
        let key = SigningKey::from_bytes(&[n; 32]);
        let hash = "ab".repeat(32);
        let mut challenges = crate::device::Challenges::default();
        let challenge = challenges
            .issue(
                "test",
                &hex::encode(key.verifying_key().to_bytes()),
                &hash,
                0,
            )
            .unwrap();
        Arc::new(
            challenges
                .verify(
                    &challenge.nonce,
                    &hex::encode(key.sign(&challenge.message().unwrap()).to_bytes()),
                    &hash,
                    0,
                )
                .unwrap(),
        )
    }
    #[test]
    fn expiry_capacity_and_refresh_boundaries() {
        let mut pool = Leases::new(1);
        let first = device(1);
        let address = Ipv4Addr::new(10, 66, 0, 1);
        let issued = pool
            .issue("space".into(), first.clone(), address, 100)
            .unwrap();
        let lease = pool.lookup(&issued.access_token, "space", 100).unwrap();
        assert!(pool.issue("space".into(), device(2), address, 101).is_err());
        assert!(!lease.cancelled.is_cancelled());
        assert_eq!(
            pool.refresh("space", &first, &issued.session_id, 699)
                .unwrap(),
            1299
        );
        assert!(pool.lookup(&issued.access_token, "space", 1298).is_ok());
        assert!(pool
            .refresh("space", &first, &issued.session_id, 1299)
            .is_err());
        pool.sweep(1299);
        assert!(lease.cancelled.is_cancelled());
        assert!(pool.lookup(&issued.access_token, "space", 1299).is_err());
        assert!(pool.issue("other".into(), device(2), address, 1299).is_ok());
        assert_eq!(pool.members.len(), 1);
        assert_eq!(pool.tokens.len(), 1);
    }
}
