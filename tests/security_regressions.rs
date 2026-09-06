#![cfg(feature = "server")]
use frp_sh::{
    p2p::{enc::EncStream, stream::UdpStream, stun, turn_server::TurnServer},
    signaling::{server, SignalingClient},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    time::{timeout, Duration},
};

#[tokio::test]
async fn removed_panel_routes_are_not_exposed() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let task = tokio::spawn(server::run_http(listener, server::new_state(), None, None));
    let client = reqwest::Client::new();
    for path in ["/panel", "/api/traffic", "/api/debug", "/api/info", "/ws"] {
        assert_eq!(
            client
                .get(format!("http://{addr}{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    assert_eq!(
        client
            .get(format!("http://{addr}/health"))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    task.abort();
}

#[tokio::test]
async fn encrypted_stream_is_bounded_and_flush_waits() {
    let (wire, _unresponsive) = tokio::io::duplex(1);
    let mut stream = EncStream::new(wire, &[7; 32]);
    stream.write_all(b"payload").await.unwrap();
    assert!(timeout(Duration::from_millis(80), stream.flush())
        .await
        .is_err());
    assert!(timeout(
        Duration::from_millis(80),
        stream.write_all(&vec![0; 256 * 1024])
    )
    .await
    .is_err());
}
#[tokio::test]
async fn encrypted_stream_large_bidirectional_transfer_and_close() {
    timeout(Duration::from_secs(10), async {
        let (a, b) = tokio::io::duplex(128);
        let mut a = EncStream::new(a, &[7; 32]);
        let mut b = EncStream::new(b, &[7; 32]);
        let left = async {
            a.write_all(&vec![42; 512 * 1024]).await.unwrap();
            a.flush().await.unwrap();
            a.shutdown().await.unwrap();
            let mut out = Vec::new();
            a.read_to_end(&mut out).await.unwrap();
            assert_eq!(out, vec![21; 12345]);
        };
        let right = async {
            let mut out = Vec::new();
            b.read_to_end(&mut out).await.unwrap();
            assert_eq!(out, vec![42; 512 * 1024]);
            b.write_all(&vec![21; 12345]).await.unwrap();
            b.shutdown().await.unwrap();
        };
        tokio::join!(left, right);
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn different_keys_never_deliver_plaintext() {
    let (a, b) = tokio::io::duplex(1024);
    let mut a = EncStream::new(a, &[7; 32]);
    let mut b = EncStream::new(b, &[8; 32]);
    a.write_all(b"protected").await.unwrap();
    let mut out = [0; 9];
    assert!(timeout(Duration::from_secs(2), b.read_exact(&mut out))
        .await
        .unwrap()
        .is_err());
}
#[tokio::test]
async fn session_challenges_are_fresh() {
    async fn challenge() -> [u8; 32] {
        let (a, mut b) = tokio::io::duplex(128);
        let _a = EncStream::new(a, &[7; 32]);
        let mut h = [0; 32];
        b.read_exact(&mut h).await.unwrap();
        h
    }
    assert_ne!(challenge().await, challenge().await);
}
#[tokio::test]
async fn slow_udp_consumer_loses_no_bytes() {
    timeout(Duration::from_secs(30), async {
        let a = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let b = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let aa = a.local_addr().unwrap();
        let ba = b.local_addr().unwrap();
        let mut a = UdpStream::new(a, ba, None, None);
        let mut b = UdpStream::new(b, aa, None, None);
        let send = async {
            a.write_all(&vec![42; 1200 * 1000]).await.unwrap();
            a.shutdown().await.unwrap();
        };
        let recv = async {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let mut out = Vec::new();
            b.read_to_end(&mut out).await.unwrap();
            assert_eq!(out, vec![42; 1200 * 1000]);
        };
        tokio::join!(send, recv);
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn unknown_udp_sender_cannot_change_peer() {
    let p = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let bad = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let s = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = s.local_addr().unwrap();
    let expected = p.local_addr().unwrap();
    let stream = UdpStream::new(s, expected, None, Some([7; 32]));
    let mut frame = b"FRS2".to_vec();
    frame.push(0);
    frame.extend([0; 8]);
    frame.extend([0, 1]);
    frame.push(b'p');
    bad.send_to(&frame, addr).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(stream.peer(), expected);
}
#[tokio::test]
async fn owner_only_refresh_delete_and_percent_encoded_token() {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let task = tokio::spawn(server::run_http(
        l,
        server::new_state(),
        Some("test+password".into()),
        None,
    ));
    let owner = SignalingClient::new_with_password(&url, Some("test+password"));
    let created = owner
        .create_room(
            "security",
            60,
            "127.0.0.1:9000".parse().unwrap(),
            None,
            vec![],
            vec![],
            vec![],
            "0.4.0".into(),
            None,
            None,
        )
        .await
        .unwrap();
    let raw = reqwest::Client::new();
    let r = raw
        .get(format!(
            "{url}/room/{}?token=test%2Bpassword",
            created.room_id
        ))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let outsider = SignalingClient::new_with_password(&url, Some("test+password"));
    assert!(outsider.delete_room(&created.room_id).await.is_err());
    assert!(outsider
        .refresh_room(
            &created.room_id,
            "127.0.0.1:9001".parse().unwrap(),
            vec![],
            vec![],
            None
        )
        .await
        .is_err());
    owner
        .refresh_room(
            &created.room_id,
            "127.0.0.1:9002".parse().unwrap(),
            vec![],
            vec![],
            None,
        )
        .await
        .unwrap();
    owner.delete_room(&created.room_id).await.unwrap();
    task.abort();
}
#[tokio::test]
async fn builtin_stun_has_matching_transaction() {
    let server = TurnServer::start("127.0.0.1:0".parse().unwrap(), None, None)
        .await
        .unwrap();
    let addr = server.local_addr();
    let task = tokio::spawn(server.run());
    let s = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    assert_eq!(
        stun::binding_probe(&s, addr).await.unwrap(),
        s.local_addr().unwrap()
    );
    task.abort();
}
#[tokio::test(flavor = "multi_thread")]
async fn tcp_relay_rejects_mismatched_end_to_end_keys() {
    use frp_sh::{commands, config::Config, signaling::server};
    let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let cfg = Config {
        signaling_addr: format!("http://{}", http.local_addr().unwrap()),
        relay_addr: relay.local_addr().unwrap().to_string(),
        signaling_udp: Some(udp.local_addr().unwrap().to_string()),
        ..Config::default()
    };
    let state = server::new_state();
    tokio::spawn(server::run_http(http, state.clone(), None, None));
    tokio::spawn(server::run_relay(relay, state, None));
    tokio::spawn(server::run_udp_echo(udp));
    let created: serde_json::Value = reqwest::Client::new()
        .post(format!("{}/room/create", cfg.signaling_addr))
        .json(&serde_json::json!({"prefix":"audit","ttl":120,"addr":"127.0.0.1:9000"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let room = created["room_id"].as_str().unwrap().to_string();
    cfg.room_tokens.lock().unwrap().insert(
        room.clone(),
        created["owner_token"].as_str().unwrap().into(),
    );
    let echo = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let service = echo.local_addr().unwrap().to_string();
    tokio::spawn(async move {
        let (mut s, _) = echo.accept().await.unwrap();
        let mut b = [0; 128];
        let n = s.read(&mut b).await.unwrap();
        s.write_all(&b[..n]).await.unwrap();
    });
    let local = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = local.local_addr().unwrap();
    drop(local);
    let ch = cfg.clone();
    let rh = room.clone();
    let host = tokio::spawn(async move {
        commands::host_session(
            &ch,
            &rh,
            service,
            true,
            Some("host-secret".into()),
            1,
            0,
            None,
            Some(1),
            false,
        )
        .await
    });
    let guest = tokio::spawn(async move {
        commands::guest_session(
            &cfg,
            &room,
            addr.to_string(),
            true,
            Some("different-secret".into()),
            1,
            0,
            None,
            Some(1),
            None,
            false,
        )
        .await
    });
    let mut client = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Ok(s) = TcpStream::connect(addr).await {
                break s;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    client.write_all(b"secret payload").await.unwrap();
    let mut b = [0; 14];
    let outcome = tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut b)).await;
    assert!(
        !matches!(outcome, Ok(Ok(_))),
        "different keys must never deliver application data"
    );
    host.abort();
    guest.abort();
}
