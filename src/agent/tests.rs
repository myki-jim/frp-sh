use super::*;

async fn wait_phase(manager: &SessionManager, expected: Phase) {
    let mut updates = manager.subscribe();
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if updates.borrow_and_update().phase == expected {
                break;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .expect("supervisor did not reach expected phase");
}

#[tokio::test]
async fn supervisor_recovers_invalid_job_and_stops_without_worker() {
    let dir = std::env::temp_dir().join(format!("frpsh-supervisor-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("job.toml");
    std::fs::write(&path, "invalid").unwrap();
    let manager = SessionManager::default();
    let shutdown = CancellationToken::new();
    let worker = tokio::spawn({
        let path = path.clone();
        let manager = manager.clone();
        let shutdown = shutdown.clone();
        async move { supervise(&path, shutdown, manager).await }
    });
    wait_phase(&manager, Phase::Error).await;
    let status = crate::local_status::query("agent").await.unwrap();
    assert_eq!(status.lifecycle.unwrap().phase, Phase::Error);
    let mut desired = job::Job {
        schema_version: 1,
        enabled: false,
        config: dir.join("missing-config.toml"),
        profile: "friends".into(),
    };
    desired.save(&path).unwrap();
    wait_phase(&manager, Phase::Stopped).await;
    desired.enabled = true;
    desired.save(&path).unwrap();
    wait_phase(&manager, Phase::Error).await;
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(4), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(manager.snapshot().phase, Phase::Stopped);
    assert!(!manager.snapshot().starts_at_boot);
    assert!(manager.begin(false).is_ok());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
#[test]
fn jobs_store_references_only_and_roundtrip_atomically() {
    let dir = std::env::temp_dir().join(format!("frpsh-agent-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("job.toml");
    let mut value = job::Job {
        schema_version: 1,
        enabled: true,
        config: dir.join("config.toml"),
        profile: "friends".into(),
    };
    value.save(&path).unwrap();
    assert_eq!(job::Job::load(&path).unwrap(), value);
    value.enabled = false;
    value.save(&path).unwrap();
    assert!(!job::Job::load(&path).unwrap().enabled);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("password"));
    assert!(!text.contains("token"));
    std::fs::write(&path, format!("{text}password = 'unwanted'\n")).unwrap();
    assert!(job::Job::load(&path).is_err());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
