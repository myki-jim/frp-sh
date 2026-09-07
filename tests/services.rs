use frp_sh::services::{self, Binding, Protocol, Published, ServiceInfo};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::Semaphore,
    task::JoinSet,
};
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn eight_members_have_concurrent_tcp_and_isolated_udp_flows() {
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp_addr = tcp.local_addr().unwrap();
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let udp_addr = udp.local_addr().unwrap();
    let mut jobs = JoinSet::new();
    jobs.spawn(async move {
        loop {
            let (s, _) = tcp.accept().await.unwrap();
            tokio::spawn(async move {
                let (mut r, mut w) = s.into_split();
                let _ = tokio::io::copy(&mut r, &mut w).await;
            });
        }
    });
    jobs.spawn(async move {
        let mut b = [0; 16384];
        loop {
            let (n, p) = udp.recv_from(&mut b).await.unwrap();
            udp.send_to(&b[..n], p).await.unwrap();
        }
    });
    let targets = vec![
        Published {
            info: ServiceInfo {
                id: 1,
                label: "Web".into(),
                protocol: Protocol::Tcp,
                port: tcp_addr.port(),
            },
            target: tcp_addr,
        },
        Published {
            info: ServiceInfo {
                id: 2,
                label: "Game".into(),
                protocol: Protocol::Udp,
                port: udp_addr.port(),
            },
            target: udp_addr,
        },
    ];
    let budget = Arc::new(Semaphore::new(256));
    let mut checks = JoinSet::new();
    for member in 0..8u8 {
        let (a, b) = tokio::io::duplex(65536);
        let publisher = targets.clone();
        let budget = budget.clone();
        jobs.spawn(async move {
            let _ = services::session(Box::new(a), publisher, vec![], budget).await;
        });
        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local = tcp.local_addr().unwrap();
        let udp = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let local_udp = udp.local_addr().unwrap();
        let bindings = vec![
            Binding::Tcp(targets[0].info.clone(), tcp),
            Binding::Udp(targets[1].info.clone(), udp),
        ];
        jobs.spawn(async move {
            let _ = services::session(Box::new(b), vec![], bindings, Arc::new(Semaphore::new(64)))
                .await;
        });
        for connection in 0..8u8 {
            checks.spawn(async move {
                let mut s = TcpStream::connect(local).await.unwrap();
                let payload = vec![member * 8 + connection; 32768];
                s.write_all(&payload).await.unwrap();
                s.shutdown().await.unwrap();
                let mut answer = Vec::new();
                s.read_to_end(&mut answer).await.unwrap();
                assert_eq!(answer, payload);
            });
        }
        for source in 0..2u8 {
            checks.spawn(async move {
                let s = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                s.connect(local_udp).await.unwrap();
                let payload = vec![member, source, 42];
                s.send(&payload).await.unwrap();
                let mut buf = [0; 64];
                let n = s.recv(&mut buf).await.unwrap();
                assert_eq!(&buf[..n], payload);
            });
        }
    }
    tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(result) = checks.join_next().await {
            result.unwrap();
        }
    })
    .await
    .unwrap();
    jobs.abort_all();
}
#[tokio::test]
async fn oversized_service_frame_is_rejected_before_allocation() {
    let (a, mut b) = tokio::io::duplex(64);
    let job = tokio::spawn(services::session(
        Box::new(a),
        vec![],
        vec![],
        Arc::new(Semaphore::new(64)),
    ));
    b.write_all(&[2, 0, 0, 0, 1, 0xff, 0xff, 0xff, 0xff])
        .await
        .unwrap();
    assert!(tokio::time::timeout(Duration::from_secs(1), job)
        .await
        .unwrap()
        .unwrap()
        .is_err());
}
#[cfg(feature = "server")]
#[tokio::test]
async fn room_invitation_cannot_create_rooms_or_access_other_rooms_and_is_revocable() {
    use frp_sh::signaling::{server, SignalingClient};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let state = server::new_state();
    let task = tokio::spawn(server::run_http(
        listener,
        state,
        Some("test-server-admin".into()),
        None,
    ));
    let admin = SignalingClient::new_with_password(&url, Some("test-server-admin"));
    let a = admin
        .create_room(
            "",
            60,
            "127.0.0.1:1".parse().unwrap(),
            None,
            vec![],
            vec![],
            vec![],
            "0.5.0".into(),
            None,
            None,
        )
        .await
        .unwrap();
    let b = admin
        .create_room(
            "",
            60,
            "127.0.0.1:2".parse().unwrap(),
            None,
            vec![],
            vec![],
            vec![],
            "0.5.0".into(),
            None,
            None,
        )
        .await
        .unwrap();
    let token = admin.secure_room(&a.room_id).await.unwrap();
    let guest = SignalingClient::new_with_password(&url, Some(&token));
    assert!(guest.get_room(&a.room_id).await.is_ok());
    assert!(guest.get_room(&b.room_id).await.is_err());
    assert!(guest
        .create_room(
            "",
            60,
            "127.0.0.1:3".parse().unwrap(),
            None,
            vec![],
            vec![],
            vec![],
            "0.5.0".into(),
            None,
            None
        )
        .await
        .is_err());
    assert!(guest.secure_room(&a.room_id).await.is_err());
    assert!(guest.delete_room(&a.room_id).await.is_err());
    admin.secure_room(&a.room_id).await.unwrap();
    assert!(guest.get_room(&a.room_id).await.is_err());
    task.abort();
}

#[cfg(feature = "server")]
#[tokio::test]
async fn revocation_closes_an_established_encrypted_relay() {
    use frp_sh::{
        p2p::relay::{self, RelayRole},
        signaling::{server, SignalingClient},
    };
    let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = tcp.local_addr().unwrap();
    let url = format!("http://{}", http.local_addr().unwrap());
    let state = server::new_state();
    let http_job = tokio::spawn(server::run_http(
        http,
        state.clone(),
        Some("relay-admin-fixture".into()),
        None,
    ));
    let relay_job = tokio::spawn(server::run_relay(
        tcp,
        state,
        Some("relay-admin-fixture".into()),
    ));
    let api = SignalingClient::new_with_password(&url, Some("relay-admin-fixture"));
    let room = api
        .create_room(
            "",
            60,
            "127.0.0.1:1".parse().unwrap(),
            None,
            vec![],
            vec![],
            vec![],
            "0.5.0".into(),
            None,
            None,
        )
        .await
        .unwrap();
    let token = api.secure_room(&room.room_id).await.unwrap();
    let (mut host, _) = relay::connect(
        addr,
        &room.room_id,
        RelayRole::Host,
        Some(&token),
        true,
        Some("member"),
        Some(room.owner_token),
    )
    .await
    .unwrap();
    let (mut guest, _) = relay::connect(
        addr,
        &room.room_id,
        RelayRole::Guest,
        Some(&token),
        true,
        Some("member"),
        None,
    )
    .await
    .unwrap();
    host.write_all(b"before-revoke").await.unwrap();
    host.flush().await.unwrap();
    let mut data = [0; 13];
    tokio::time::timeout(Duration::from_secs(2), guest.read_exact(&mut data))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&data, b"before-revoke");
    api.secure_room(&room.room_id).await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), guest.read(&mut data))
        .await
        .unwrap();
    assert!(matches!(result, Ok(0) | Err(_)));
    http_job.abort();
    relay_job.abort();
}
