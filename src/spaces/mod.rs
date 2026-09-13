//! Device-authenticated space control protocol, separate from legacy room passwords.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
pub mod cli;
pub mod client;
#[cfg(feature = "server")]
pub mod server;
#[cfg(all(test, feature = "server"))]
mod tests;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Create {
        name: String,
        expires_at: Option<u64>,
        request_id: String,
    },
    List,
    Members {
        space: String,
    },
    Invite {
        space: String,
        ttl: Option<u64>,
        uses: u32,
    },
    Redeem {
        token: String,
        request_id: String,
    },
    RevokeInvites {
        space: String,
    },
    RemoveMember {
        space: String,
        device: String,
    },
    Leave {
        space: String,
    },
    Delete {
        space: String,
    },
}
impl Operation {
    pub fn digest(&self) -> anyhow::Result<String> {
        let mut hash = Sha256::new();
        hash.update(b"frp-sh/space-operation/v1\0");
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
