//! Noninteractive profile supervisor. OS service installation is a separate boundary.
pub mod job;
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
        .arg("--agent-worker")
        .arg("--plain")
        .arg("--no-color")
        .arg("--config")
        .arg(&job.config)
        .arg("profile")
        .arg("run")
        .arg(&job.profile)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
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
    let _status = crate::local_status::publish_with_manager("agent", Some(manager.clone())).await?;
    let mut current: Option<job::Job> = None;
    let mut child = None;
    let mut lease = None;
    let mut failures = 0u32;
    let mut retry_at = tokio::time::Instant::now();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tokio::select! { _=shutdown.cancelled()=>break, _=tick.tick()=>{} }
        let loaded = job::Job::load(path);
        let desired = match loaded {
            Ok(job) => job,
            Err(_) => {
                stop(&mut child).await;
                if let Some(active) = lease.take() {
                    drop(active);
                }
                let active = manager.begin(false)?;
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
                Ok(None) => continue,
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
        let active = manager.begin(false)?;
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
    let result = supervise(path, shutdown, SessionManager::default()).await;
    watcher.abort();
    result
}
#[cfg(test)]
mod tests;
