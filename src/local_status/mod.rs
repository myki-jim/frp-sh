//! Read-only per-account status IPC. Never serializes configuration or invitation objects.
mod transport;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
const MAX_FRAME: usize = 65536;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub schema_version: u32,
    pub role: String,
    pub state: String,
    pub observed_at: u64,
    pub room: Option<String>,
    pub peers: Vec<Peer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle: Option<crate::runtime::Snapshot>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Peer {
    pub name: String,
    pub transport: String,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub rtt_ms: Option<f64>,
}
fn safe(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(80).collect()
}
fn snapshot(role: &str) -> Status {
    let info = crate::stats::info_snapshot();
    let peers = crate::stats::links_snapshot()
        .into_iter()
        .take(33)
        .map(|(name, kind, _, _, tx, rx, _, rtt)| Peer {
            name: safe(&name),
            transport: safe(&kind),
            tx_bytes: tx,
            rx_bytes: rx,
            rtt_ms: (rtt > 0).then_some(f64::from(rtt) / 1000.0),
        })
        .collect::<Vec<_>>();
    Status {
        schema_version: 1,
        role: role.into(),
        state: if role == "serve" {
            "running"
        } else {
            "running_connection_unverified"
        }
        .into(),
        observed_at: crate::utils::now_unix(),
        room: (!info.room.is_empty()).then(|| safe(&info.room)),
        peers,
        lifecycle: None,
    }
}
pub struct Guard(tokio::task::JoinHandle<()>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.abort();
    }
}
pub async fn publish(role: &'static str) -> anyhow::Result<Guard> {
    publish_with_manager(role, None).await
}
pub async fn publish_with_manager(
    role: &'static str,
    manager: Option<crate::runtime::SessionManager>,
) -> anyhow::Result<Guard> {
    let mut listener = transport::Listener::bind(role).await?;
    Ok(Guard(tokio::spawn(async move {
        let mut jobs = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _=jobs.join_next(),if !jobs.is_empty()=>{},
                incoming=listener.accept(),if jobs.len()<6=>{
                    let Ok(mut stream)=incoming else {break};
                    let manager=manager.clone();
                    jobs.spawn(async move {
                        let mut value=snapshot(role);
                        value.lifecycle=manager.map(|m|m.snapshot());
                        if value.lifecycle.is_some() { value.state="running".into(); }
                        let Ok(bytes)=serde_json::to_vec(&value) else{return};
                        if bytes.len()>MAX_FRAME{return;}
                        let _=tokio::time::timeout(Duration::from_secs(2),async {
                            stream.write_u32(bytes.len() as u32).await?;
                            stream.write_all(&bytes).await?;
                            // Keep Windows pipe buffers alive until the client consumes them.
                            stream.read_u8().await.map(|_| ())
                        }).await;
                    });
                }
            }
        }
    })))
}
pub async fn query(role: &str) -> anyhow::Result<Status> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut stream = transport::connect(role).await?;
        let length = stream.read_u32().await? as usize;
        anyhow::ensure!(length <= MAX_FRAME, "status response too large");
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes).await?;
        let result: Status = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(
            result.schema_version == 1 && result.role == role,
            "incompatible status response"
        );
        stream.write_u8(1).await?;
        Ok(result)
    })
    .await?
}
pub async fn print(json: bool) -> anyhow::Result<i32> {
    let mut values = Vec::new();
    for role in ["agent", "serve", "host", "guest"] {
        match query(role).await {
            Ok(value) => values.push(serde_json::to_value(value)?),
            Err(error) => {
                let state = match error.downcast_ref::<std::io::Error>().map(|e| e.kind()) {
                    Some(std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused) => {
                        "not_running_for_current_account"
                    }
                    Some(std::io::ErrorKind::PermissionDenied) => "permission_denied",
                    _ => "unavailable",
                };
                values.push(serde_json::json!({"role":role,"state":state}));
            }
        }
    }
    let code = exit_code(&values);
    if json {
        println!(
            "{}",
            serde_json::json!({"schema_version":1,"scope":"current_account","processes":values})
        );
    } else {
        println!("frp-sh status (current account)");
        for value in values {
            println!(
                "{}: {}",
                value["role"].as_str().unwrap_or("?"),
                value["state"].as_str().unwrap_or("?")
            );
            if let Some(room) = value["room"].as_str() {
                println!("  room: {}", safe(room));
            }
            if let Some(phase) = value["lifecycle"]["phase"].as_str() {
                println!("  session: {}", safe(phase));
            }
        }
    }
    Ok(code)
}
fn exit_code(values: &[serde_json::Value]) -> i32 {
    if values.iter().any(|v| v["state"] == "permission_denied") {
        4
    } else if values.iter().any(|v| {
        v["state"] == "unavailable"
            || matches!(
                v["lifecycle"]["phase"].as_str(),
                Some("error" | "revoked" | "auth_required")
            )
    }) {
        1
    } else if values.iter().any(|v| {
        v["state"] == "running_connection_unverified"
            || matches!(
                v["lifecycle"]["phase"].as_str(),
                Some("starting" | "joining" | "reconnecting" | "waiting_network" | "degraded")
            )
    }) {
        2
    } else if values.iter().any(|v| v["state"] == "running") {
        0
    } else {
        3
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supervisor_process_does_not_hide_failed_or_pending_session() {
        for (phase, expected) in [
            ("error", 1),
            ("auth_required", 1),
            ("revoked", 1),
            ("joining", 2),
            ("reconnecting", 2),
            ("stopped", 0),
        ] {
            let values = [serde_json::json!({"state":"running","lifecycle":{"phase":phase}})];
            assert_eq!(exit_code(&values), expected);
        }
    }
    #[tokio::test]
    async fn process_status_roundtrip_and_exclusive_endpoint() {
        let guard = publish("guest").await.unwrap();
        assert!(publish("guest").await.is_err());
        let result = query("guest").await.unwrap();
        assert_eq!(result.role, "guest");
        assert_ne!(result.state, "connected");
        assert!(query("guest").await.is_ok());
        let value = serde_json::to_value(result).unwrap();
        assert!(value.get("password").is_none());
        drop(guard);
    }
}
