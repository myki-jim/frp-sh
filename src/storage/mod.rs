//! Durable spaces and atomic admission. No sockets or plaintext invite tokens are stored.
use anyhow::{ensure, Context};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use std::path::Path;
mod membership;
pub mod worker;

pub struct Store {
    db: Connection,
}
#[derive(Clone, Copy)]
pub struct Quota {
    pub spaces: u32,
    pub members_per_space: u32,
    pub total_members: u32,
}
impl Default for Quota {
    fn default() -> Self {
        Self {
            spaces: 1024,
            members_per_space: 33,
            total_members: 33792,
        }
    }
}
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Space {
    pub id: String,
    pub name: String,
    pub owner: String,
    pub expires_at: Option<u64>,
}
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct Admission {
    pub space_id: String,
    pub device: String,
}

fn digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}
fn identity(device: &str) -> anyhow::Result<()> {
    ensure!(
        device.len() == 64 && device.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid device public key"
    );
    Ok(())
}
impl Store {
    /// The caller supplies an administrator-owned directory with restrictive ACLs.
    /// Run database operations on a dedicated blocking worker, never inside Tokio polling.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        let version: u32 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        ensure!(version <= 2, "database schema is newer than this binary");
        db.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS spaces(id TEXT PRIMARY KEY, name TEXT NOT NULL, owner TEXT NOT NULL, expires_at INTEGER);
            CREATE TABLE IF NOT EXISTS members(space TEXT NOT NULL REFERENCES spaces(id) ON DELETE CASCADE, device TEXT NOT NULL, PRIMARY KEY(space,device));
            CREATE TABLE IF NOT EXISTS invites(hash TEXT PRIMARY KEY, space TEXT NOT NULL REFERENCES spaces(id) ON DELETE CASCADE, expires_at INTEGER NOT NULL, remaining INTEGER NOT NULL CHECK(remaining>=0), revoked INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS redemptions(hash TEXT NOT NULL REFERENCES invites(hash) ON DELETE CASCADE, device TEXT NOT NULL, request_id TEXT NOT NULL, PRIMARY KEY(hash,device,request_id));
            CREATE TABLE IF NOT EXISTS creation_requests(owner TEXT NOT NULL, request_id TEXT NOT NULL, space TEXT NOT NULL REFERENCES spaces(id) ON DELETE CASCADE, PRIMARY KEY(owner,request_id));
            PRAGMA user_version=2; COMMIT;")?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        Ok(Self { db })
    }
    pub fn create(
        &mut self,
        owner: &crate::device::VerifiedDevice,
        name: &str,
        expires_at: Option<u64>,
        now: u64,
        quota: Quota,
    ) -> anyhow::Result<Space> {
        self.create_idempotent(
            owner,
            name,
            expires_at,
            now,
            quota,
            &uuid::Uuid::new_v4().to_string(),
        )
    }
    pub fn create_idempotent(
        &mut self,
        owner: &crate::device::VerifiedDevice,
        name: &str,
        expires_at: Option<u64>,
        now: u64,
        quota: Quota,
        request_id: &str,
    ) -> anyhow::Result<Space> {
        uuid::Uuid::parse_str(request_id).context("invalid request ID")?;
        ensure!(now <= i64::MAX as u64, "invalid clock");
        let owner = owner.public_key();
        identity(owner)?;
        ensure!(
            !name.trim().is_empty() && name.len() <= 80 && !name.chars().any(char::is_control),
            "invalid space name"
        );
        ensure!(
            expires_at.is_none_or(|t| t > now && t <= i64::MAX as u64),
            "invalid expiration"
        );
        ensure!(
            quota.spaces > 0 && quota.members_per_space > 0 && quota.total_members > 0,
            "invalid quota"
        );
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM spaces WHERE expires_at IS NOT NULL AND expires_at<=?",
            [now as i64],
        )?;
        let existing:Option<Space>=tx.query_row("SELECT s.id,s.name,s.owner,s.expires_at FROM creation_requests c JOIN spaces s ON s.id=c.space WHERE c.owner=? AND c.request_id=?",params![owner,request_id],|r|Ok(Space{id:r.get(0)?,name:r.get(1)?,owner:r.get(2)?,expires_at:r.get::<_,Option<i64>>(3)?.map(|t|t as u64)})).optional()?;
        if let Some(space) = existing {
            ensure!(
                space.name == name && space.expires_at == expires_at,
                "request ID reused with different parameters"
            );
            return Ok(space);
        }
        let count: u32 = tx.query_row("SELECT count(*) FROM spaces", [], |r| r.get(0))?;
        let members: u32 = tx.query_row("SELECT count(*) FROM members", [], |r| r.get(0))?;
        ensure!(
            count < quota.spaces && members < quota.total_members,
            "capacity reached"
        );
        let space = Space {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            owner: owner.into(),
            expires_at,
        };
        tx.execute(
            "INSERT INTO spaces VALUES(?,?,?,?)",
            params![
                space.id,
                space.name,
                space.owner,
                space.expires_at.map(|t| t as i64)
            ],
        )?;
        tx.execute("INSERT INTO members VALUES(?,?)", params![space.id, owner])?;
        tx.execute(
            "INSERT INTO creation_requests VALUES(?,?,?)",
            params![owner, request_id, space.id],
        )?;
        tx.commit()?;
        Ok(space)
    }
    /// Call only after authenticating the owner; owner is a verified device key, not request text.
    pub fn invite(
        &mut self,
        space: &str,
        owner: &crate::device::VerifiedDevice,
        now: u64,
        ttl: u64,
        uses: u32,
    ) -> anyhow::Result<String> {
        let owner = owner.public_key();
        ensure!(
            (1..=86400).contains(&ttl) && (1..=32).contains(&uses),
            "invalid invite policy"
        );
        let deadline = now
            .checked_add(ttl)
            .filter(|t| *t <= i64::MAX as u64)
            .context("invalid clock")?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let valid: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM spaces WHERE id=? AND owner=? AND (expires_at IS NULL OR expires_at>?))",params![space,owner,now as i64],|r|r.get(0))?;
        ensure!(valid, "space owner required");
        let outstanding:u32=tx.query_row("SELECT count(*) FROM invites WHERE space=? AND expires_at>? AND remaining>0 AND revoked=0",params![space,now as i64],|r|r.get(0))?;
        ensure!(outstanding < 20, "outstanding invite limit reached");
        let token = crate::utils::random_hex(32);
        tx.execute(
            "INSERT INTO invites(hash,space,expires_at,remaining) VALUES(?,?,?,?)",
            params![digest(&token), space, deadline as i64, uses],
        )?;
        tx.commit()?;
        Ok(token)
    }
    /// Device possession MUST be verified before calling this transaction.
    pub fn redeem(
        &mut self,
        token: &str,
        device: &crate::device::VerifiedDevice,
        request_id: &str,
        now: u64,
        quota: Quota,
    ) -> anyhow::Result<Admission> {
        ensure!(now <= i64::MAX as u64, "invalid clock");
        let device = device.public_key();
        identity(device)?;
        ensure!(
            token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid invitation"
        );
        uuid::Uuid::parse_str(request_id).context("invalid request ID")?;
        let hash = digest(token);
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value: Option<(String, i64, u32, bool)> = tx
            .query_row(
                "SELECT space,expires_at,remaining,revoked FROM invites WHERE hash=?",
                [&hash],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let (space, deadline, remaining, revoked) = value.context("invalid invitation")?;
        let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM spaces WHERE id=? AND (expires_at IS NULL OR expires_at>?))",params![space,now as i64],|r|r.get(0))?;
        ensure!(active && !revoked, "invitation unavailable");
        let member: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM members WHERE space=? AND device=?)",
            params![space, device],
            |r| r.get(0),
        )?;
        let retry: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM redemptions WHERE hash=? AND device=? AND request_id=?)",
            params![hash, device, request_id],
            |r| r.get(0),
        )?;
        if member && retry {
            return Ok(Admission {
                space_id: space,
                device: device.into(),
            });
        }
        ensure!(
            deadline > now as i64 && remaining > 0,
            "invitation expired or exhausted"
        );
        // Repeated requests by an existing member must not create unbounded
        // receipts. Only admission of a new member needs an idempotency record.
        if member {
            return Ok(Admission {
                space_id: space,
                device: device.into(),
            });
        }
        if !member {
            let count: u32 = tx.query_row(
                "SELECT count(*) FROM members WHERE space=?",
                [&space],
                |r| r.get(0),
            )?;
            let total:u32=tx.query_row("SELECT count(*) FROM members m JOIN spaces s ON s.id=m.space WHERE s.expires_at IS NULL OR s.expires_at>?",[now as i64],|r|r.get(0))?;
            ensure!(
                count < quota.members_per_space && total < quota.total_members,
                "capacity reached"
            );
            tx.execute("INSERT INTO members VALUES(?,?)", params![space, device])?;
            tx.execute(
                "UPDATE invites SET remaining=remaining-1 WHERE hash=?",
                [&hash],
            )?;
        }
        tx.execute(
            "INSERT INTO redemptions VALUES(?,?,?)",
            params![hash, device, request_id],
        )?;
        tx.commit()?;
        Ok(Admission {
            space_id: space,
            device: device.into(),
        })
    }
    pub fn remove_member(
        &mut self,
        space: &str,
        owner: &crate::device::VerifiedDevice,
        device: &str,
    ) -> anyhow::Result<()> {
        let owner = owner.public_key();
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let valid: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM spaces WHERE id=? AND owner=?)",
            params![space, owner],
            |r| r.get(0),
        )?;
        ensure!(valid && owner != device, "owner cannot be removed");
        tx.execute(
            "DELETE FROM members WHERE space=? AND device=?",
            params![space, device],
        )?;
        // Prevent an old idempotency receipt from resurrecting a removed member.
        tx.execute("DELETE FROM redemptions WHERE device=? AND hash IN (SELECT hash FROM invites WHERE space=?)",params![device,space])?;
        tx.commit()?;
        Ok(())
    }
}
#[cfg(test)]
mod tests;
