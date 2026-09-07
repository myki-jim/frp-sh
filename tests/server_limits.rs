#![cfg(feature = "server")]
use frp_sh::signaling::{limits::ServerLimits, server};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use tokio::{net::TcpListener, task::JoinHandle};

struct Server {
    base: String,
    client: Client,
    state: server::SharedState,
    task: JoinHandle<anyhow::Result<()>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn start(limits: ServerLimits) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let state = server::new_state();
        let task = tokio::spawn(server::run_http_with_limits(
            listener,
            state.clone(),
            None,
            None,
            limits,
        ));
        Self {
            base,
            client: Client::new(),
            state,
            task,
        }
    }
    async fn create(&self) -> reqwest::Response {
        self.client
            .post(format!("{}/room/create", self.base))
            .json(&json!({"prefix":"", "ttl":3600, "addr":"127.0.0.1:1234"}))
            .send()
            .await
            .unwrap()
    }
    async fn join(&self, room: &str, visitor: Option<&str>) -> reqwest::Response {
        self.client
            .post(format!("{}/room/{room}/join", self.base))
            .json(&json!({"addr":"127.0.0.1:5678", "visitor_id":visitor}))
            .send()
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn concurrent_creates_respect_limit_and_delete_frees_capacity() {
    let s = Server::start(ServerLimits {
        max_rooms: 1,
        ..Default::default()
    })
    .await;
    let (a, b) = tokio::join!(s.create(), s.create());
    let (created, rejected) = if a.status().is_success() {
        (a, b)
    } else {
        (b, a)
    };
    assert_eq!(rejected.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(rejected.text().await.unwrap(), "server room limit reached");
    let room: Value = created.json().await.unwrap();
    let id = room["room_id"].as_str().unwrap();
    let response = s
        .client
        .delete(format!("{}/room/{id}", s.base))
        .header("X-Frp-Sh-Room-Token", room["owner_token"].as_str().unwrap())
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert!(s.create().await.status().is_success());
}

#[tokio::test]
async fn members_include_owner_and_reconnect_reuses_slot() {
    let s = Server::start(ServerLimits {
        max_members: 2,
        ..Default::default()
    })
    .await;
    let room: Value = s.create().await.json().await.unwrap();
    let id = room["room_id"].as_str().unwrap();
    let (a, b) = tokio::join!(s.join(id, Some("device-a")), s.join(id, Some("device-b")));
    let winner = if a.status().is_success() {
        "device-a"
    } else {
        "device-b"
    };
    assert_eq!(
        [a.status(), b.status()]
            .iter()
            .filter(|s| s.is_success())
            .count(),
        1
    );
    assert!([a.status(), b.status()].contains(&StatusCode::TOO_MANY_REQUESTS));
    assert!(s.join(id, Some(winner)).await.status().is_success());
    assert_eq!(s.state.lock().await[id].guests.len(), 1);
}

#[tokio::test]
async fn total_limit_applies_to_creates_and_joins_and_expiry_releases_it() {
    let s = Server::start(ServerLimits {
        max_total_members: 3,
        ..Default::default()
    })
    .await;
    let first: Value = s.create().await.json().await.unwrap();
    let second: Value = s.create().await.json().await.unwrap();
    let first = first["room_id"].as_str().unwrap();
    let second = second["room_id"].as_str().unwrap();
    assert!(s.join(first, Some("a")).await.status().is_success());
    assert_eq!(
        s.join(second, Some("b")).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(s.create().await.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(s.join(first, Some("a")).await.status().is_success());
    s.state.lock().await.get_mut(first).unwrap().expires_at = 0;
    assert!(s.join(second, Some("b")).await.status().is_success());
    assert!(s.create().await.status().is_success());
}

#[tokio::test]
async fn anonymous_reconnect_and_advertised_limits() {
    let s = Server::start(ServerLimits {
        max_members: 2,
        ..Default::default()
    })
    .await;
    let room: Value = s.create().await.json().await.unwrap();
    let id = room["room_id"].as_str().unwrap();
    for _ in 0..2 {
        assert!(s.join(id, None).await.status().is_success());
    }
    assert_eq!(
        s.join(id, Some("new")).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    let version: Value = s
        .client
        .get(format!("{}/version", s.base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(version["limits"]["max_members"], 2);
}

#[test]
fn cli_rejects_invalid_limits() {
    use clap::Parser;
    use frp_sh::cli::Cli;
    for (flag, value) in [
        ("--max-rooms", "0"),
        ("--max-rooms", "1025"),
        ("--max-members", "1"),
        ("--max-members", "34"),
        ("--max-total-members", "0"),
    ] {
        assert!(Cli::try_parse_from(["frp-sh", "serve", flag, value]).is_err());
    }
    assert!(Cli::try_parse_from([
        "frp-sh",
        "serve",
        "--max-rooms",
        "20",
        "--max-members",
        "8",
        "--max-total-members",
        "100"
    ])
    .is_ok());
}

#[tokio::test]
async fn concurrent_cross_room_admission_shares_total_budget() {
    let s = Server::start(ServerLimits {
        max_total_members: 3,
        ..Default::default()
    })
    .await;
    let a: Value = s.create().await.json().await.unwrap();
    let b: Value = s.create().await.json().await.unwrap();
    let (a, b) = tokio::join!(
        s.join(a["room_id"].as_str().unwrap(), Some("a")),
        s.join(b["room_id"].as_str().unwrap(), Some("b"))
    );
    assert_eq!(
        [a.status(), b.status()]
            .iter()
            .filter(|s| s.is_success())
            .count(),
        1
    );
    assert!([a.status(), b.status()].contains(&StatusCode::TOO_MANY_REQUESTS));
    assert_eq!(
        s.state
            .lock()
            .await
            .values()
            .map(|r| 1 + r.guests.len())
            .sum::<usize>(),
        3
    );
}

#[tokio::test]
async fn client_reports_capacity_instead_of_an_opaque_status() {
    let s = Server::start(ServerLimits {
        max_total_members: 1,
        ..Default::default()
    })
    .await;
    let room: Value = s.create().await.json().await.unwrap();
    let api = frp_sh::signaling::SignalingClient::new_with_password(&s.base, None);
    let error = api
        .join_room(
            room["room_id"].as_str().unwrap(),
            "127.0.0.1:5555".parse().unwrap(),
            vec![],
            Some("guest".into()),
            None,
            vec![],
            None,
            None,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("member limit reached"));
}
