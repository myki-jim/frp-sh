use anyhow::{ensure, Context};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};

type Task = Box<dyn FnOnce(&mut Connection) + Send>;

#[derive(Clone)]
pub struct Worker(mpsc::Sender<Task>);

#[derive(Clone, Debug, Serialize)]
pub struct Binding {
    pub domain: String,
    #[serde(skip_serializing)]
    pub owner: String,
    pub verified: bool,
    pub verification_name: String,
    pub created_at: u64,
    pub updated_at: u64,
}

fn digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Binding> {
    Ok(Binding {
        domain: row.get(0)?,
        owner: row.get(1)?,
        verified: row.get::<_, Option<i64>>(2)?.is_some(),
        verification_name: row.get(3)?,
        created_at: row.get::<_, i64>(4)? as u64,
        updated_at: row.get::<_, i64>(5)? as u64,
    })
}

impl Worker {
    pub async fn open(path: PathBuf) -> anyhow::Result<Self> {
        let (tx, mut rx) = mpsc::channel::<Task>(64);
        let (ready_tx, ready_rx) = oneshot::channel();
        std::thread::Builder::new()
            .name("frpsh-domains-db".into())
            .spawn(move || {
                let opened = (|| -> anyhow::Result<Connection> {
                    let db = Connection::open(path)?;
                    db.busy_timeout(std::time::Duration::from_secs(5))?;
                    db.execute_batch(
                        "PRAGMA journal_mode=WAL;
                         CREATE TABLE IF NOT EXISTS domain_bindings(
                           domain TEXT PRIMARY KEY,
                           owner TEXT NOT NULL,
                           token_hash TEXT NOT NULL,
                           token_expires INTEGER NOT NULL,
                           verified_at INTEGER,
                           verification_name TEXT NOT NULL,
                           created_at INTEGER NOT NULL,
                           updated_at INTEGER NOT NULL
                         );
                         CREATE INDEX IF NOT EXISTS domain_owner ON domain_bindings(owner);",
                    )?;
                    let has_verification_name = {
                        let mut statement = db.prepare("PRAGMA table_info(domain_bindings)")?;
                        let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
                        let names = columns.collect::<Result<Vec<_>, _>>()?;
                        names.iter().any(|name| name == "verification_name")
                    };
                    if !has_verification_name {
                        db.execute(
                            "ALTER TABLE domain_bindings ADD COLUMN verification_name TEXT NOT NULL DEFAULT ''",
                            [],
                        )?;
                    }
                    Ok(db)
                })();
                let mut db = match opened {
                    Ok(db) => {
                        let _ = ready_tx.send(Ok(()));
                        db
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                while let Some(task) = rx.blocking_recv() {
                    task(&mut db);
                }
            })?;
        ready_rx.await??;
        Ok(Self(tx))
    }

    async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> anyhow::Result<T> + Send + 'static,
    ) -> anyhow::Result<T> {
        let (tx, rx) = oneshot::channel();
        self.0
            .try_send(Box::new(move |db| {
                let _ = tx.send(f(db));
            }))
            .map_err(|_| anyhow::anyhow!("domain database busy or unavailable"))?;
        rx.await?
    }

    pub async fn begin(
        &self,
        domain: String,
        owner: String,
        token: String,
        verification_name: String,
        now: u64,
    ) -> anyhow::Result<()> {
        self.call(move |db| {
            ensure!(now <= i64::MAX as u64, "invalid clock");
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing: Option<String> = tx
                .query_row(
                    "SELECT owner FROM domain_bindings WHERE domain=?",
                    [&domain],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(existing) = existing {
                ensure!(existing == owner, "domain is already owned by another device");
            } else {
                let count: u32 = tx.query_row(
                    "SELECT count(*) FROM domain_bindings WHERE owner=?",
                    [&owner],
                    |r| r.get(0),
                )?;
                ensure!(count < 20, "device domain limit reached");
            }
            let expires = now.checked_add(900).context("invalid clock")?;
            tx.execute(
                "INSERT INTO domain_bindings(domain,owner,token_hash,token_expires,verified_at,verification_name,created_at,updated_at)
                 VALUES(?,?,?,?,NULL,?,?,?)
                 ON CONFLICT(domain) DO UPDATE SET token_hash=excluded.token_hash,
                 token_expires=excluded.token_expires,verified_at=NULL,
                 verification_name=excluded.verification_name,updated_at=excluded.updated_at",
                params![domain, owner, digest(&token), expires as i64, verification_name, now as i64, now as i64],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    pub async fn verify(
        &self,
        domain: String,
        owner: String,
        candidates: Vec<String>,
        now: u64,
    ) -> anyhow::Result<Binding> {
        self.call(move |db| {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let value: Option<(String, String, i64)> = tx
                .query_row(
                    "SELECT owner,token_hash,token_expires FROM domain_bindings WHERE domain=?",
                    [&domain],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let (bound_owner, expected, expires) = value.context("domain is not bound")?;
            ensure!(bound_owner == owner, "domain owner required");
            ensure!(expires > now as i64, "domain verification expired; bind again");
            ensure!(
                candidates.iter().any(|candidate| digest(candidate) == expected),
                "verification TXT record not found"
            );
            tx.execute(
                "UPDATE domain_bindings SET verified_at=?,updated_at=? WHERE domain=?",
                params![now as i64, now as i64, domain],
            )?;
            let binding = tx.query_row(
                "SELECT domain,owner,verified_at,verification_name,created_at,updated_at FROM domain_bindings WHERE domain=?",
                [&domain],
                row,
            )?;
            tx.commit()?;
            Ok(binding)
        })
        .await
    }

    pub async fn owned(&self, domain: String, owner: String) -> anyhow::Result<Binding> {
        self.call(move |db| {
            let value = db
                .query_row(
                    "SELECT domain,owner,verified_at,verification_name,created_at,updated_at FROM domain_bindings WHERE domain=?",
                    [&domain],
                    row,
                )
                .optional()?
                .context("domain is not bound")?;
            ensure!(value.owner == owner, "domain owner required");
            Ok(value)
        })
        .await
    }

    pub async fn public(&self, domain: String) -> anyhow::Result<Option<Binding>> {
        self.call(move |db| {
            db.query_row(
                "SELECT domain,owner,verified_at,verification_name,created_at,updated_at FROM domain_bindings WHERE domain=? AND verified_at IS NOT NULL",
                [&domain],
                row,
            )
            .optional()
            .map_err(Into::into)
        })
        .await
    }

    pub async fn remove(&self, domain: String, owner: String) -> anyhow::Result<()> {
        self.call(move |db| {
            let changed = db.execute(
                "DELETE FROM domain_bindings WHERE domain=? AND owner=?",
                params![domain, owner],
            )?;
            ensure!(changed == 1, "domain owner required");
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn binding_requires_owner_and_matching_txt() {
        let path =
            std::env::temp_dir().join(format!("frpsh-domains-{}.sqlite", uuid::Uuid::new_v4()));
        let db = Worker::open(path).await.unwrap();
        let domain = "demo.example.com".to_owned();
        let owner = "ab".repeat(32);
        db.begin(
            domain.clone(),
            owner.clone(),
            "frpsh-verify=secret".into(),
            "_frpsh-verify-deadbeef0000.demo.example.com".into(),
            10,
        )
        .await
        .unwrap();
        assert!(db
            .verify(
                domain.clone(),
                owner.clone(),
                vec!["frpsh-verify=wrong".into()],
                20
            )
            .await
            .is_err());
        let binding = db
            .verify(
                domain.clone(),
                owner.clone(),
                vec!["frpsh-verify=secret".into()],
                20,
            )
            .await
            .unwrap();
        assert!(binding.verified);
        assert!(db.owned(domain.clone(), "cd".repeat(32)).await.is_err());
        assert!(db.public(domain.clone()).await.unwrap().is_some());
        db.remove(domain.clone(), owner).await.unwrap();
        assert!(db.public(domain).await.unwrap().is_none());
    }
}
