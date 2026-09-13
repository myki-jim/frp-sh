use super::*;
use ed25519_dalek::{Signer, SigningKey};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
const ORIGIN: &str = "https://example.test";
async fn start(path: std::path::PathBuf) -> (String, tokio::task::JoinHandle<()>) {
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
    let task = tokio::spawn(async move {
        axum::serve(listener, service.router()).await.unwrap();
    });
    (format!("http://{addr}/spaces/v1"), task)
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
