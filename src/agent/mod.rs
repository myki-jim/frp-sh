//! Noninteractive profile supervisor. OS service installation is a separate boundary.
pub mod install;
#[cfg(unix)]
mod install_unix;
pub mod job;
pub mod server_config;
pub mod service;
use crate::runtime::{ErrorCode, Phase, SessionManager};
use std::{path::Path, process::Stdio, time::Duration};
use tokio_util::sync::CancellationToken;

/// A closed parent pipe terminates this worker, including when the supervisor crashes.
/// No command bytes are accepted; the pipe is solely a lifetime lease.
pub fn watch_parent() {
    std::thread::spawn(|| {
        use std::io::Read;
        let mut byte = [0u8; 1];
        let mut input = std::io::stdin().lock();
        while matches!(input.read(&mut byte), Ok(1)) {}
        std::process::exit(0);
    });
}
fn spawn(job: &job::Job) -> anyhow::Result<tokio::process::Child> {
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .env("FRPSH_LOG_DIR", crate::debuglog::directory())
        .arg("--agent-worker")
        .arg("--plain")
        .arg("--no-color")
        .arg("--config")
        .arg(&job.config)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if job.server {
        command.arg("agent").arg("server-worker");
    } else {
        command.arg("profile").arg("run").arg(&job.profile);
    }
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    Ok(command.spawn()?)
}
async fn stop(child: &mut Option<tokio::process::Child>) {
    if let Some(mut process) = child.take() {
        drop(process.stdin.take());
        if tokio::time::timeout(Duration::from_secs(3), process.wait())
            .await
            .is_err()
        {
            let _ = process.kill().await;
        }
    }
}
/// Reload desired state without opening a privileged management socket.
/// All worker resources belong to a subprocess, so cancellation also closes helper IPC.
pub async fn supervise(
    path: &Path,
    shutdown: CancellationToken,
    manager: SessionManager,
) -> anyhow::Result<()> {
    supervise_with_startup(path, shutdown, manager, false).await
}
pub async fn supervise_with_startup(
    path: &Path,
    shutdown: CancellationToken,
    manager: SessionManager,
    starts_at_boot: bool,
) -> anyhow::Result<()> {
    let initial = job::Job::load(path).ok();
    let server = initial.as_ref().is_some_and(|j| j.server);
    let owner = initial.and_then(|j| j.owner_sid);
    let role = if server { "server_agent" } else { "agent" };
    let _status =
        crate::local_status::publish_as(role, Some(manager.clone()), owner.as_deref()).await?;
    let mut current: Option<job::Job> = None;
    let mut child = None;
    let initial_lease = manager.begin(starts_at_boot)?;
    initial_lease.transition(Phase::Stopped, None);
    let mut lease = Some(initial_lease);
    let mut failures = 0u32;
    let mut retry_at = tokio::time::Instant::now();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! { _=shutdown.cancelled()=>break, _=tick.tick()=>{} }
        let loaded = job::Job::load(path).and_then(|job| {
            anyhow::ensure!(
                job.server == server && job.owner_sid == owner,
                "job role cannot change while supervisor is running"
            );
            Ok(job)
        });
        let desired = match loaded {
            Ok(job) => job,
            Err(_) => {
                stop(&mut child).await;
                if let Some(active) = lease.take() {
                    drop(active);
                }
                let active = manager.begin(starts_at_boot)?;
                active.transition(Phase::Error, Some(ErrorCode::SessionFailed));
                lease = Some(active);
                current = None;
                continue;
            }
        };
        if current.as_ref() != Some(&desired) {
            stop(&mut child).await;
            if let Some(active) = &lease {
                active.transition(Phase::Stopped, None);
            }
            drop(lease.take());
            failures = 0;
            retry_at = tokio::time::Instant::now();
            current = Some(desired.clone());
        }
        if !desired.enabled {
            continue;
        }
        if let Some(process) = &mut child {
            match process.try_wait() {
                Ok(None) => {
                    if server {
                        let ready = crate::local_status::query("serve").await.is_ok_and(|s| {
                            Some(s.process_id) == process.id() && s.state == "running"
                        });
                        if let Some(active) = &lease {
                            active.transition(
                                if ready {
                                    Phase::Serving
                                } else {
                                    Phase::Starting
                                },
                                None,
                            );
                        }
                    }
                    continue;
                }
                _ => {
                    stop(&mut child).await;
                    if let Some(active) = &lease {
                        active.transition(Phase::Reconnecting, Some(ErrorCode::NetworkUnavailable));
                    }
                    failures = failures.saturating_add(1);
                    let delay = (1u64 << failures.min(5)).min(30);
                    retry_at = tokio::time::Instant::now() + Duration::from_secs(delay);
                }
            }
        }
        if tokio::time::Instant::now() < retry_at {
            continue;
        }
        drop(lease.take());
        let active = manager.begin(starts_at_boot)?;
        if desired.check_profile().is_err() {
            active.transition(Phase::Error, Some(ErrorCode::SessionFailed));
            lease = Some(active);
            retry_at = tokio::time::Instant::now() + Duration::from_secs(5);
            continue;
        }
        match spawn(&desired) {
            Ok(process) => {
                child = Some(process);
                active.transition(Phase::Joining, None);
            }
            Err(_) => {
                active.transition(Phase::Error, Some(ErrorCode::SessionFailed));
                retry_at = tokio::time::Instant::now() + Duration::from_secs(5);
            }
        }
        lease = Some(active);
    }
    stop(&mut child).await;
    if let Some(active) = &lease {
        active.transition(Phase::Stopped, None);
    }
    manager.request_stop();
    drop(lease);
    Ok(())
}
pub async fn run(path: &Path) -> anyhow::Result<()> {
    run_with_startup(path, false).await
}
pub async fn run_with_startup(path: &Path, starts_at_boot: bool) -> anyhow::Result<()> {
    job::Job::load(path)?;
    let shutdown = CancellationToken::new();
    let signal = shutdown.clone();
    let watcher = tokio::spawn(async move {
        #[cfg(unix)]
        {
            let mut terminate =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
            tokio::select! {_=terminate.recv()=>{},r=tokio::signal::ctrl_c()=>{r?;}}
        }
        #[cfg(windows)]
        tokio::signal::ctrl_c().await?;
        signal.cancel();
        Ok::<(), std::io::Error>(())
    });
    let result =
        supervise_with_startup(path, shutdown, SessionManager::default(), starts_at_boot).await;
    watcher.abort();
    result
}
#[cfg(test)]
mod tests;
