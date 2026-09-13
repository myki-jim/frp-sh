use super::*;
use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
const ORIGIN: &str = "https://example.test";

#[tokio::test]
async fn member_sessions_replace_refresh_and_revoke_without_owner_presence() {
    let (url, task, service) = start_with_service(":memory:".into()).await;
    let client = Client::new();
    let owner = SigningKey::from_bytes(&[51; 32]);
    let guest = SigningKey::from_bytes(&[52; 32]);
    let (_, created) = execute(
        &client,
        &url,
        &owner,
        Operation::Create {
            name: "leases".into(),
            expires_at: None,
            request_id: uuid::Uuid::new_v4().to_string(),
        },
        true,
    )
    .await;
    let space = created["id"].as_str().unwrap().to_owned();
    assert_eq!(
        execute(
            &client,
            &url,
            &guest,
            Operation::OpenSession {
                space: space.clone()
            },
            false
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, invitation) = execute(
        &client,
        &url,
        &owner,
        Operation::Invite {
            space: space.clone(),
            ttl: None,
            uses: 1,
        },
        false,
    )
    .await;
    let ticket =
        crate::invite_ticket::Ticket::parse(invitation["invitation"].as_str().unwrap()).unwrap();
    assert_eq!(
        execute(
            &client,
            &url,
            &guest,
            Operation::Redeem {
                token: ticket.token().into(),
                request_id: uuid::Uuid::new_v4().to_string()
            },
            false
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, admitted) = execute(
        &client,
        &url,
        &guest,
        Operation::OpenSession {
            space: space.clone(),
        },
        false,
    )
    .await;
    let admission: leases::Admission = serde_json::from_value(admitted).unwrap();
    assert_eq!(admission.address.to_string(), "10.66.0.2");
    assert_eq!(admission.access_token.len(), 64);
    let old = service
        .authenticate_session(space.clone(), admission.access_token.clone())
        .await
        .unwrap();
    assert!(service
        .authenticate_session(
            uuid::Uuid::new_v4().to_string(),
            admission.access_token.clone()
        )
        .await
        .is_err());
    assert_eq!(
        execute(
            &client,
            &url,
            &owner,
            Operation::RefreshSession {
                space: space.clone(),
                session: admission.session_id.clone()
            },
            false
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        execute(
            &client,
            &url,
            &guest,
            Operation::RefreshSession {
                space: space.clone(),
                session: admission.session_id.clone()
            },
            false
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, replacement) = execute(
        &client,
        &url,
        &guest,
        Operation::OpenSession {
            space: space.clone(),
        },
        false,
    )
    .await;
    let replacement: leases::Admission = serde_json::from_value(replacement).unwrap();
    assert!(old.cancelled.is_cancelled());
    assert!(service
        .authenticate_session(space.clone(), admission.access_token)
        .await
        .is_err());
    let active = service
        .authenticate_session(space.clone(), replacement.access_token.clone())
        .await
        .unwrap();
    assert_eq!(
        execute(
            &client,
            &url,
            &guest,
            Operation::CloseSession {
                space: space.clone(),
                session: admission.session_id
            },
            false
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert!(!active.cancelled.is_cancelled());
    assert_eq!(
        execute(
            &client,
            &url,
            &owner,
            Operation::RemoveMember {
                space: space.clone(),
                device: hex::encode(guest.verifying_key().to_bytes())
            },
            false
        )
        .await
        .0,
        StatusCode::OK
    );
    assert!(active.cancelled.is_cancelled());
    assert!(service
        .authenticate_session(space.clone(), replacement.access_token)
        .await
        .is_err());
    assert_eq!(
        execute(
            &client,
            &url,
            &guest,
            Operation::RefreshSession {
                space,
                session: replacement.session_id
            },
            false
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn relay_routes_only_authorized_members_and_rewrites_the_source_address() {
    let (url, task, _) = start_with_service(":memory:".into()).await;
    let client = Client::new();
    let owner = SigningKey::from_bytes(&[61; 32]);
    let guest = SigningKey::from_bytes(&[62; 32]);
    let (_, created) = execute(
        &client,
        &url,
        &owner,
        Operation::Create {
            name: "relay".into(),
            expires_at: None,
            request_id: uuid::Uuid::new_v4().to_string(),
        },
        true,
    )
    .await;
    let space = created["id"].as_str().unwrap().to_owned();
    let (_, invite) = execute(
        &client,
        &url,
        &owner,
        Operation::Invite {
            space: space.clone(),
            ttl: None,
            uses: 1,
        },
        false,
    )
    .await;
    let ticket =
        crate::invite_ticket::Ticket::parse(invite["invitation"].as_str().unwrap()).unwrap();
    assert_eq!(
        execute(
            &client,
            &url,
            &guest,
            Operation::Redeem {
                token: ticket.token().into(),
                request_id: uuid::Uuid::new_v4().to_string()
            },
            false
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, owner_admission) = execute(
        &client,
        &url,
        &owner,
        Operation::OpenSession {
            space: space.clone(),
        },
        false,
    )
    .await;
    let (_, guest_admission) = execute(
        &client,
        &url,
        &guest,
        Operation::OpenSession {
            space: space.clone(),
        },
        false,
    )
    .await;
    let owner_admission: leases::Admission = serde_json::from_value(owner_admission).unwrap();
    let guest_admission: leases::Admission = serde_json::from_value(guest_admission).unwrap();
    let base = url
        .replacen("http://", "ws://", 1)
        .trim_end_matches("/spaces/v1")
        .to_owned();
    let connect = |token: &str| {
        let mut request = format!("{base}/spaces/v1/{space}/relay")
            .into_client_request()
            .unwrap();
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
        request
    };
    let (mut owner_socket, _) =
        tokio_tungstenite::connect_async(connect(&owner_admission.access_token))
            .await
            .unwrap();
    let (mut guest_socket, _) =
        tokio_tungstenite::connect_async(connect(&guest_admission.access_token))
            .await
            .unwrap();
    // The guest claims its own source byte, yet relay replaces it with the address assigned to the lease.
    guest_socket
        .send(tokio_tungstenite::tungstenite::Message::Binary(
            vec![1, 9, 8, 7].into(),
        ))
        .await
        .unwrap();
    let received = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let message = owner_socket.next().await.unwrap().unwrap();
            if matches!(message, tokio_tungstenite::tungstenite::Message::Binary(_)) {
                return message;
            }
        }
    })
    .await
    .unwrap();
    assert!(
        matches!(received, tokio_tungstenite::tungstenite::Message::Binary(frame) if frame.as_ref()==[2,9,8,7])
    );
    // Invalid target and unavailable address are ignored rather than reflected or broadcast.
    guest_socket
        .send(tokio_tungstenite::tungstenite::Message::Binary(
            vec![255, 1].into(),
        ))
        .await
        .unwrap();
    // Server pings can arrive at any time; only a binary relay frame is a violation.
    let unexpected = tokio::time::timeout(std::time::Duration::from_millis(250), async {
        loop {
            match owner_socket.next().await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(frame))) => {
                    return Some(frame)
                }
                Some(Ok(_)) => continue,
                _ => return None,
            }
        }
    })
    .await;
    assert!(matches!(unexpected, Err(_) | Ok(None)));
    drop(guest_socket);
    drop(owner_socket);
    task.abort();
    let _ = task.await;
}
async fn start(path: std::path::PathBuf) -> (String, tokio::task::JoinHandle<()>) {
    let (url, task, _) = start_with_service(path).await;
    (url, task)
}
async fn start_with_service(
    path: std::path::PathBuf,
) -> (String, tokio::task::JoinHandle<()>, server::Service) {
    let service = server::Service::open(
        server::Options {
            spaces_db: Some(path),
            spaces_origin: Some(ORIGIN.into()),
            invite_ttl: 900,
            invite_max_ttl: 1800,
        },
        "test-administrator".into(),
        crate::storage::Quota::default(),
    )
    .await
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = service.clone().router();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (format!("http://{addr}/spaces/v1"), task, service)
}
async fn signed(
    client: &Client,
    url: &str,
    key: &SigningKey,
    operation: Operation,
) -> SignedRequest {
    let challenge: crate::device::Challenge = client
        .post(format!("{url}/challenge"))
        .json(&ChallengeRequest {
            device: hex::encode(key.verifying_key().to_bytes()),
            action_hash: operation.digest().unwrap(),
        })
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(challenge.server, ORIGIN);
    SignedRequest {
        signature: hex::encode(key.sign(&challenge.message().unwrap()).to_bytes()),
        nonce: challenge.nonce,
        operation,
    }
}
async fn execute(
    client: &Client,
    url: &str,
    key: &SigningKey,
    operation: Operation,
    admin: bool,
) -> (StatusCode, Value) {
    let req = signed(client, url, key, operation).await;
    let mut send = client.post(format!("{url}/execute")).json(&req);
    if admin {
        send = send.header("X-Frp-Sh-Token", "test-administrator");
    }
    let response = send.send().await.unwrap();
    let status = response.status();
    (status, response.json().await.unwrap())
}
#[tokio::test]
async fn durable_spaces_and_device_bound_invites_through_http() {
    let dir = std::env::temp_dir().join(format!("frpsh-space-api-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("spaces.db");
    let (url, task) = start(path.clone()).await;
    let client = Client::new();
    let owner = SigningKey::from_bytes(&[41; 32]);
    let guest = SigningKey::from_bytes(&[42; 32]);
    let stranger = SigningKey::from_bytes(&[43; 32]);
    let create = Operation::Create {
        name: "friends".into(),
        expires_at: None,
        request_id: uuid::Uuid::new_v4().to_string(),
    };
    assert_eq!(
        execute(&client, &url, &owner, create.clone(), false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, space) = execute(&client, &url, &owner, create.clone(), true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(execute(&client, &url, &owner, create, true).await.1, space);
    assert!(space["expires_at"].is_null());
    let id = space["id"].as_str().unwrap().to_owned();
    assert_eq!(
        execute(
            &client,
            &url,
            &guest,
            Operation::Members { space: id.clone() },
            false
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        execute(
            &client,
            &url,
            &owner,
            Operation::Invite {
                space: id.clone(),
                ttl: Some(1801),
                uses: 1
            },
            false
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let (_, invitation) = execute(
        &client,
        &url,
        &owner,
        Operation::Invite {
            space: id.clone(),
            ttl: None,
            uses: 1,
        },
        false,
    )
    .await;
    let ticket =
        crate::invite_ticket::Ticket::parse(invitation["invitation"].as_str().unwrap()).unwrap();
    assert_eq!(ticket.server(), ORIGIN);
    let remaining = invitation["expires_at"].as_u64().unwrap() - crate::utils::now_unix();
    assert!((895..=900).contains(&remaining));
    let redeem = Operation::Redeem {
        token: ticket.token().into(),
        request_id: uuid::Uuid::new_v4().to_string(),
    };
    let req = signed(&client, &url, &guest, redeem.clone()).await;
    assert_eq!(
        client
            .post(format!("{url}/execute"))
            .json(&req)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .post(format!("{url}/execute"))
            .json(&req)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        execute(&client, &url, &guest, redeem.clone(), false)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        execute(&client, &url, &stranger, redeem.clone(), false)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut tampered = signed(&client, &url, &owner, Operation::List).await;
    tampered.operation = Operation::Delete { space: id.clone() };
    assert_eq!(
        client
            .post(format!("{url}/execute"))
            .json(&tampered)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    task.abort();
    let _ = task.await;
    let (url, task) = start(path).await;
    let (_, list) = execute(&client, &url, &guest, Operation::List, false).await;
    assert_eq!(list[0]["id"], id);
    assert_eq!(
        execute(
            &client,
            &url,
            &guest,
            Operation::Delete { space: id.clone() },
            false
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        execute(
            &client,
            &url,
            &owner,
            Operation::RemoveMember {
                space: id.clone(),
                device: hex::encode(guest.verifying_key().to_bytes())
            },
            false
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        execute(&client, &url, &guest, redeem, false).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        execute(
            &client,
            &url,
            &owner,
            Operation::Delete { space: id },
            false
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        execute(&client, &url, &owner, Operation::List, false)
            .await
            .1,
        json!([])
    );
    task.abort();
    let _ = task.await;
    // The dedicated SQLite worker shuts down after the last HTTP state drops.
    for _ in 0..20 {
        let files = std::fs::read_dir(&dir).unwrap();
        for file in files {
            let _ = std::fs::remove_file(file.unwrap().path());
        }
        if std::fs::remove_dir(&dir).is_ok() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("database worker retained handles after shutdown");
}
