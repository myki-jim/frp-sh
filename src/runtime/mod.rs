//! Session lifecycle shared by foreground and background frontends.
//! A generation prevents a cancelled task from publishing into its replacement.
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Stopped,
    Starting,
    WaitingNetwork,
    AuthRequired,
    Joining,
    Connected,
    Degraded,
    Reconnecting,
    Revoked,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema_version: u32,
    pub session_id: Option<String>,
    pub generation: u64,
    pub phase: Phase,
    pub starts_at_boot: bool,
    pub observed_at: u64,
    /// Stable error code only: never remote text, a credential or an invitation.
    pub error_code: Option<ErrorCode>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    NetworkUnavailable,
    AuthenticationRequired,
    MembershipRevoked,
    HelperUnavailable,
    SessionFailed,
}

struct Inner {
    leased: bool,
    state: Snapshot,
    cancel: CancellationToken,
}
#[derive(Clone)]
pub struct SessionManager {
    inner: Arc<Mutex<Inner>>,
    updates: watch::Sender<Snapshot>,
}
pub struct SessionLease {
    generation: u64,
    manager: SessionManager,
    pub cancelled: CancellationToken,
}
impl Default for SessionManager {
    fn default() -> Self {
        let state = Snapshot {
            schema_version: 1,
            session_id: None,
            generation: 0,
            phase: Phase::Stopped,
            starts_at_boot: false,
            observed_at: crate::utils::now_unix(),
            error_code: None,
        };
        let (updates, _) = watch::channel(state.clone());
        Self {
            inner: Arc::new(Mutex::new(Inner {
                leased: false,
                state,
                cancel: CancellationToken::new(),
            })),
            updates,
        }
    }
}
impl SessionManager {
    pub fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.updates.subscribe()
    }
    pub fn snapshot(&self) -> Snapshot {
        self.inner.lock().unwrap().state.clone()
    }
    /// The caller must await the cancelled task's cleanup before starting another task.
    /// begin rejects an active task instead of silently sharing its network resources.
    pub fn begin(&self, starts_at_boot: bool) -> anyhow::Result<SessionLease> {
        let mut inner = self.inner.lock().unwrap();
        anyhow::ensure!(!inner.leased, "a session is already active");
        inner.cancel.cancel();
        inner.cancel = CancellationToken::new();
        inner.state.generation = inner
            .state
            .generation
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("session generation exhausted"))?;
        inner.leased = true;
        inner.state.session_id = Some(uuid::Uuid::new_v4().to_string());
        inner.state.phase = Phase::Starting;
        inner.state.starts_at_boot = starts_at_boot;
        inner.state.error_code = None;
        inner.state.observed_at = crate::utils::now_unix();
        self.updates.send_replace(inner.state.clone());
        Ok(SessionLease {
            generation: inner.state.generation,
            manager: self.clone(),
            cancelled: inner.cancel.clone(),
        })
    }
    /// Cancels execution without claiming that cleanup is already finished.
    pub fn request_stop(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.state.starts_at_boot = false;
        inner.cancel.cancel();
        inner.state.observed_at = crate::utils::now_unix();
        self.updates.send_replace(inner.state.clone());
    }
}
impl SessionLease {
    pub fn transition(&self, phase: Phase, error: Option<ErrorCode>) -> bool {
        let mut inner = self.manager.inner.lock().unwrap();
        if inner.state.generation != self.generation || self.cancelled.is_cancelled() {
            return false;
        }
        inner.state.phase = phase;
        inner.state.error_code = error;
        inner.state.observed_at = crate::utils::now_unix();
        self.manager.updates.send_replace(inner.state.clone());
        true
    }
}
impl Drop for SessionLease {
    fn drop(&mut self) {
        self.cancelled.cancel();
        let mut inner = self.manager.inner.lock().unwrap();
        if inner.state.generation == self.generation {
            inner.leased = false;
            if !matches!(inner.state.phase, Phase::Error | Phase::Revoked) {
                inner.state.phase = Phase::Stopped;
            }
            inner.state.observed_at = crate::utils::now_unix();
            self.manager.updates.send_replace(inner.state.clone());
        }
    }
}

#[cfg(test)]
mod tests;
