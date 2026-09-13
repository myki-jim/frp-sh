use super::*;
fn device(n: u8) -> crate::device::VerifiedDevice {
    use ed25519_dalek::{Signer, SigningKey};
    let key = SigningKey::from_bytes(&[n; 32]);
    let mut challenges = crate::device::Challenges::default();
    let action = "aa".repeat(32);
    let challenge = challenges
        .issue(
            "test-server",
            &hex::encode(key.verifying_key().to_bytes()),
            &action,
            0,
        )
        .unwrap();
    let signature = hex::encode(key.sign(&challenge.message().unwrap()).to_bytes());
    challenges
        .verify(&challenge.nonce, &signature, &action, 0)
        .unwrap()
}
fn memory() -> Store {
    Store::open(Path::new(":memory:")).unwrap()
}
#[test]
fn permanent_space_invite_expiry_and_idempotency() {
    let mut db = memory();
    let quota = Quota::default();
    let space = db.create(&device(1), "friends", None, 100, quota).unwrap();
    let token = db.invite(&space.id, &device(1), 100, 900, 1).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let admitted = db.redeem(&token, &device(2), &id, 999, quota).unwrap();
    assert_eq!(
        admitted,
        db.redeem(&token, &device(2), &id, 1001, quota).unwrap()
    );
    assert!(db
        .redeem(
            &token,
            &device(3),
            &uuid::Uuid::new_v4().to_string(),
            999,
            quota
        )
        .is_err());
    db.remove_member(&space.id, &device(1), device(2).public_key())
        .unwrap();
    assert!(db.redeem(&token, &device(2), &id, 1001, quota).is_err());
}
#[test]
fn full_room_does_not_consume_invite() {
    let mut db = memory();
    let quota = Quota::default();
    let space = db.create(&device(1), "friends", None, 100, quota).unwrap();
    let token = db.invite(&space.id, &device(1), 100, 900, 1).unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    assert!(db
        .redeem(
            &token,
            &device(2),
            &id,
            200,
            Quota {
                members_per_space: 1,
                ..quota
            }
        )
        .is_err());
    assert!(db.redeem(&token, &device(2), &id, 200, quota).is_ok());
}
#[test]
fn exact_expiry_and_owner_enforced() {
    let mut db = memory();
    let quota = Quota::default();
    let space = db
        .create(&device(1), "friends", Some(2000), 100, quota)
        .unwrap();
    assert!(db.invite(&space.id, &device(2), 100, 900, 1).is_err());
    let token = db.invite(&space.id, &device(1), 100, 900, 1).unwrap();
    assert!(db
        .redeem(
            &token,
            &device(2),
            &uuid::Uuid::new_v4().to_string(),
            1000,
            quota
        )
        .is_err());
    let raw: String = db
        .db
        .query_row("SELECT hash FROM invites", [], |r| r.get(0))
        .unwrap();
    assert_ne!(raw, token);
}

#[test]
fn restart_preserves_space_and_only_one_parallel_redemption_wins() {
    let dir = std::env::temp_dir().join(format!("frpsh-spaces-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("spaces.sqlite");
    let quota = Quota::default();
    let mut store = Store::open(&path).unwrap();
    let space = store
        .create(&device(1), "persistent", None, 100, quota)
        .unwrap();
    let token = store.invite(&space.id, &device(1), 100, 900, 1).unwrap();
    drop(store);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let jobs: Vec<_> = (2..=3)
        .map(|n| {
            let mut store = Store::open(&path).unwrap();
            let token = token.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let device = device(n);
                barrier.wait();
                store
                    .redeem(
                        &token,
                        &device,
                        &uuid::Uuid::new_v4().to_string(),
                        200,
                        quota,
                    )
                    .is_ok()
            })
        })
        .collect();
    assert_eq!(
        jobs.into_iter()
            .filter_map(|job| job.join().ok())
            .filter(|v| *v)
            .count(),
        1
    );
    let store = Store::open(&path).unwrap();
    let expires: Option<i64> = store
        .db
        .query_row(
            "SELECT expires_at FROM spaces WHERE id=?",
            [&space.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(expires, None);
    let count: u32 = store
        .db
        .query_row("SELECT count(*) FROM members", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
    assert_eq!(
        store
            .virtual_address(&space.id, &device(1), 201)
            .unwrap()
            .to_string(),
        "10.66.0.1"
    );
    drop(store);
    // Only this test's freshly created UUID directory is removed.
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn virtual_addresses_are_unique_stable_and_require_membership() {
    let mut db = memory();
    let quota = Quota::default();
    let owner = device(1);
    let space = db.create(&owner, "addresses", None, 100, quota).unwrap();
    let token = db.invite(&space.id, &owner, 100, 900, 4).unwrap();
    for n in 2..=4 {
        db.redeem(
            &token,
            &device(n),
            &uuid::Uuid::new_v4().to_string(),
            101,
            quota,
        )
        .unwrap();
    }
    let addresses: std::collections::HashSet<_> = (1..=4)
        .map(|n| db.virtual_address(&space.id, &device(n), 102).unwrap())
        .collect();
    assert_eq!(addresses.len(), 4);
    let third = db.virtual_address(&space.id, &device(3), 102).unwrap();
    db.remove_member(&space.id, &owner, device(2).public_key())
        .unwrap();
    assert!(db.virtual_address(&space.id, &device(2), 103).is_err());
    db.redeem(
        &token,
        &device(5),
        &uuid::Uuid::new_v4().to_string(),
        103,
        quota,
    )
    .unwrap();
    assert_eq!(
        db.virtual_address(&space.id, &device(3), 104).unwrap(),
        third
    );
    assert_eq!(
        db.virtual_address(&space.id, &device(5), 104)
            .unwrap()
            .to_string(),
        "10.66.0.2"
    );
}

#[test]
fn schema_two_migration_assigns_owner_first_and_preserves_existing_members() {
    let dir = std::env::temp_dir().join(format!("frpsh-migration-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("spaces.sqlite");
    let owner = device(1);
    let guest = device(2);
    let space = uuid::Uuid::new_v4().to_string();
    let db = Connection::open(&path).unwrap();
    db.execute_batch("CREATE TABLE spaces(id TEXT PRIMARY KEY,name TEXT NOT NULL,owner TEXT NOT NULL,expires_at INTEGER);
        CREATE TABLE members(space TEXT NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,device TEXT NOT NULL,PRIMARY KEY(space,device));
        PRAGMA user_version=2;").unwrap();
    db.execute(
        "INSERT INTO spaces VALUES(?,'legacy',?,NULL)",
        params![space, owner.public_key()],
    )
    .unwrap();
    // Reverse insertion order must not take the owner's address.
    db.execute(
        "INSERT INTO members VALUES(?,?)",
        params![space, guest.public_key()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO members VALUES(?,?)",
        params![space, owner.public_key()],
    )
    .unwrap();
    drop(db);
    for _ in 0..2 {
        let store = Store::open(&path).unwrap();
        assert_eq!(
            store
                .virtual_address(&space, &owner, 1)
                .unwrap()
                .to_string(),
            "10.66.0.1"
        );
        assert_eq!(
            store
                .virtual_address(&space, &guest, 1)
                .unwrap()
                .to_string(),
            "10.66.0.2"
        );
    }
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
