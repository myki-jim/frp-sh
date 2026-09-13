use super::*;
#[test]
fn one_session_and_explicit_cleanup() {
    let manager = SessionManager::default();
    let lease = manager.begin(true).unwrap();
    assert!(manager.begin(false).is_err());
    assert!(lease.transition(Phase::Joining, None));
    manager.request_stop();
    assert!(lease.cancelled.is_cancelled());
    assert!(!lease.transition(Phase::Connected, None));
    assert!(manager.begin(false).is_err());
    drop(lease);
    assert_eq!(manager.snapshot().phase, Phase::Stopped);
    assert!(!manager.snapshot().starts_at_boot);
    assert!(manager.begin(false).is_ok());
}
#[test]
fn failed_task_must_release_resources_before_restart() {
    let manager = SessionManager::default();
    let first = manager.begin(true).unwrap();
    first.transition(Phase::Error, Some(ErrorCode::SessionFailed));
    assert!(manager.begin(true).is_err());
    let generation = manager.snapshot().generation;
    drop(first);
    let next = manager.begin(true).unwrap();
    assert!(manager.snapshot().generation > generation);
    assert_eq!(manager.snapshot().phase, Phase::Starting);
    assert!(!next.cancelled.is_cancelled());
}
#[test]
fn status_schema_has_no_credentials_and_unobserved_is_not_connected() {
    let manager = SessionManager::default();
    let value = serde_json::to_value(manager.snapshot()).unwrap();
    assert_eq!(value["phase"], "stopped");
    assert!(value.get("password").is_none());
    assert!(value.get("key").is_none());
    assert!(value.get("invitation").is_none());
}
