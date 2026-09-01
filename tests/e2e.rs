//! 端到端测试：进程内启动信令服务器，验证直连打洞与中继回退两条数据通路。

use frp_sh::commands;
use frp_sh::config::Config;
use frp_sh::p2p::hole_punch::{parse_ack, parse_punch, PunchEngine};
use frp_sh::signaling::{server, SignalingClient};
use frp_sh::utils;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};

struct TestServer {
    cfg: Config,
}

async fn start_server() -> TestServer {
    start_server_with_password(None).await
}

/// 启动带密码（认证 + 中继加密）的信令服务器。
async fn start_server_with_password(password: Option<&str>) -> TestServer {
    // 偶发端口冲突（强杀进程的端口未完全释放）时重试
    for _ in 0..10 {
        if let Some(s) = try_start_server(password).await {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("cannot start test signaling server (port conflicts)");
}

async fn try_start_server(password: Option<&str>) -> Option<TestServer> {
    let http_listener = TcpListener::bind("127.0.0.1:0").await.ok()?;
    let http_port = http_listener.local_addr().unwrap().port();
    // UDP 探测用独立端口（避免 Windows 上"同端口 TCP 刚释放"的绑定冲突，
    // 同时验证 config.signaling_udp 独立端口能力）
    let udp = UdpSocket::bind("127.0.0.1:0").await.ok()?;
    let udp_port = udp.local_addr().unwrap().port();
    let relay_listener = TcpListener::bind("127.0.0.1:0").await.ok()?;
    let relay_port = relay_listener.local_addr().unwrap().port();
    let state = server::new_state();
    let pw = password.map(str::to_string);
    tokio::spawn(server::run_http(
        http_listener,
        state.clone(),
        pw.clone(),
        None,
    ));
    tokio::spawn(server::run_udp_echo(udp));
    tokio::spawn(server::run_relay(relay_listener, state, pw.clone()));
    Some(TestServer {
        cfg: Config {
            signaling_addr: format!("http://127.0.0.1:{http_port}"),
            relay_addr: format!("127.0.0.1:{relay_port}"),
            signaling_udp: Some(format!("127.0.0.1:{udp_port}")),
            uuid: None,
            password: pw,
            stun_addr: None,
            turn_providers: Vec::new(),
            name: None,
            profiles: std::collections::BTreeMap::new(),
        },
    })
}

/// 创建房间并返回 room_id。
async fn create_room(cfg: &Config) -> String {
    let client = SignalingClient::new_with_password(&cfg.signaling_addr, cfg.password.as_deref());
    let engine = PunchEngine::bind().await.unwrap();
    let token = utils::rand_token();
    let my_ext = client
        .learn_public_addr(engine.socket(), cfg.signaling_udp_addr().unwrap(), &token)
        .await
        .unwrap();
    let lan = utils::lan_socket_addrs(engine.local_addr().unwrap().port());
    let resp = client
        .create_room(
            "test",
            120,
            my_ext,
            None,
            lan,
            Vec::new(),
            Vec::new(),
            "0.0.0".into(),
            None,
            None,
        )
        .await
        .unwrap();
    resp.room_id
}

/// 一个 echo TCP 服务（循环接受连接，逐个原样返回）。
async fn spawn_echo_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let (mut rd, mut wr) = s.split();
                let _ = tokio::io::copy(&mut rd, &mut wr).await;
            });
        }
    });
    addr
}

async fn free_local_port() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    l.local_addr().unwrap()
}

/// 完整跑一遍 host + guest 会话：依次建立 `max_conns` 个连接并各自 echo。
async fn run_session(
    cfg: Config,
    room_id: String,
    force_relay: bool,
    key: Option<String>,
    max_conns: u64,
) -> (anyhow::Result<()>, anyhow::Result<()>) {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .try_init();
    let echo = spawn_echo_server().await;
    let cfg_h = cfg.clone();
    let room_id_h = room_id.clone();
    let key_h = key.clone();
    let host = tokio::spawn(async move {
        commands::host_session(
            &cfg_h,
            &room_id_h,
            echo.to_string(),
            force_relay,
            key_h,
            max_conns,
            0,       // spread: 并行测试时禁用端口散布，避免 PUNCH 打到其他测试的 socket
            None,    // tun
            Some(1), // 测试：单轮即返回
            false,   // expose_lan
        )
        .await
    });
    let guest_listen = free_local_port().await;
    let cfg2 = cfg.clone();
    let room_id2 = room_id.clone();
    let listen = guest_listen.to_string();
    let guest = tokio::spawn(async move {
        commands::guest_session(
            &cfg2,
            &room_id2,
            listen,
            force_relay,
            key,
            max_conns,
            0, // spread: 同 host，避免并行测试互相干扰
            None,
            Some(1), // 测试：单轮即返回
            None,    // requested_ip
            false,   // expose_lan
        )
        .await
    });

    // 等待访客本地监听就绪后，依次建立连接
    for i in 1..=max_conns {
        let payload = format!("conn-{i}-hello").into_bytes();
        let mut client = None;
        for _ in 0..200 {
            if let Ok(s) = TcpStream::connect(guest_listen).await {
                client = Some(s);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut client = client.expect("guest listen port not ready");
        client.write_all(&payload).await.unwrap();
        let mut buf = vec![0u8; payload.len() + 64];
        let n = client.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], payload, "echo mismatch on conn {i}");
        drop(client);
    }

    let host_res = tokio::time::timeout(Duration::from_secs(60), host)
        .await
        .expect("host session timeout")
        .unwrap();
    let guest_res = tokio::time::timeout(Duration::from_secs(60), guest)
        .await
        .expect("guest session timeout")
        .unwrap();
    (host_res, guest_res)
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_direct_hole_punch() {
    let srv = start_server().await;
    let room_id = create_room(&srv.cfg).await;
    let (host, guest) = run_session(srv.cfg, room_id, false, None, 1).await;
    assert!(host.is_ok(), "host session failed: {host:?}");
    assert!(guest.is_ok(), "guest session failed: {guest:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_direct_multiconn() {
    let srv = start_server().await;
    let room_id = create_room(&srv.cfg).await;
    let (host, guest) = run_session(srv.cfg, room_id, false, None, 3).await;
    assert!(host.is_ok(), "host session failed: {host:?}");
    assert!(guest.is_ok(), "guest session failed: {guest:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_direct_encrypted() {
    let srv = start_server().await;
    let room_id = create_room(&srv.cfg).await;
    let (host, guest) = run_session(srv.cfg, room_id, false, Some("test-secret".into()), 1).await;
    assert!(host.is_ok(), "host session failed: {host:?}");
    assert!(guest.is_ok(), "guest session failed: {guest:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_relay_fallback() {
    let srv = start_server().await;
    let room_id = create_room(&srv.cfg).await;
    let (host, guest) = run_session(srv.cfg, room_id, true, None, 1).await;
    assert!(host.is_ok(), "host session failed: {host:?}");
    assert!(guest.is_ok(), "guest session failed: {guest:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_mesh_multi_guest_links() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .try_init();
    let srv = start_server().await;
    let room_id = create_room(&srv.cfg).await;
    let cfg_h = srv.cfg.clone();
    let room_h = room_id.clone();
    // host 网格会话（测试模式：无 TUN），目标 2 个访客建链后返回
    let host = tokio::spawn(async move {
        commands::host_mesh_session(&cfg_h, &room_h, false, None, 0, None, false, Some(2)).await
    });

    // 两个模拟访客：加入房间并打洞，验证 host 与每个访客建立链路
    let mut guests = Vec::new();
    for i in 0..2 {
        let cfg_g = srv.cfg.clone();
        let room_g = room_id.clone();
        guests.push(tokio::spawn(async move {
            let client = SignalingClient::new_with_password(
                &cfg_g.signaling_addr,
                cfg_g.password.as_deref(),
            );
            let engine = PunchEngine::bind().await.unwrap();
            let token = utils::rand_token();
            let my_ext = client
                .learn_public_addr(engine.socket(), cfg_g.signaling_udp_addr().unwrap(), &token)
                .await
                .unwrap();
            let lan = utils::lan_socket_addrs(engine.local_addr().unwrap().port());
            client
                .join_room(
                    &room_g,
                    my_ext,
                    lan,
                    Some(format!("mesh-guest-{i}")),
                    None,
                    Vec::new(),
                    None,
                    None,
                )
                .await
                .unwrap();
            // 打洞：PUNCH 房主并等待 ACK；同时应答房主的 PUNCH。
            // 与真实访客一致：每轮刷新房主地址（房间创建后房主会话会重新绑定 socket 并刷新地址）
            let mut info = client.get_room(&room_g).await.unwrap();
            let mut ok = false;
            for _ in 0..80 {
                let _ = engine.send_punch(info.host_addr, &token).await;
                if let Ok(Some((b, _))) = engine.recv(Duration::from_millis(120)).await {
                    if let Some(t) = parse_ack(&b) {
                        if t == token {
                            ok = true;
                            break;
                        }
                    }
                    if let Some(t) = parse_punch(&b) {
                        let _ = engine.send_ack(info.host_addr, &t).await;
                    }
                }
                if let Ok(i) = client.get_room(&room_g).await {
                    info = i;
                }
            }
            // 保持 socket 存活一小段时间，让 host 完成建链
            tokio::time::sleep(Duration::from_millis(800)).await;
            ok
        }));
    }
    for g in guests {
        let r = tokio::time::timeout(Duration::from_secs(60), g)
            .await
            .expect("guest punch timeout")
            .unwrap();
        assert!(r, "guest failed to establish direct link with host");
    }
    let host_res = tokio::time::timeout(Duration::from_secs(60), host)
        .await
        .expect("host mesh session timeout")
        .unwrap();
    assert!(host_res.is_ok(), "host mesh session failed: {host_res:?}");
}

/// 内置 TURN 服务端：两个客户端经 TURN 中继双向转发 + FRS1 流（本地，无外部依赖）。
async fn turn_builtin_test(password: Option<&str>) -> SocketAddr {
    let srv = frp_sh::p2p::turn_server::TurnServer::start(
        "127.0.0.1:0".parse().unwrap(),
        password,
        Some("127.0.0.1".parse().unwrap()),
    )
    .await
    .unwrap();
    let addr = srv.local_addr();
    tokio::spawn(async move {
        let _ = srv.run().await;
    });
    addr
}

async fn turn_pair_roundtrip(
    srv_addr: SocketAddr,
    username: &str,
    password: &str,
) -> (frp_sh::p2p::turn::TurnClient, frp_sh::p2p::turn::TurnClient) {
    use frp_sh::p2p::turn::{DatagramSocket, TurnClient, TurnCredentials};
    let cred = TurnCredentials {
        server: srv_addr,
        username: username.into(),
        password: password.into(),
    };
    let a = TurnClient::connect(UdpSocket::bind("0.0.0.0:0").await.unwrap(), &cred)
        .await
        .expect("client A allocate");
    let b = TurnClient::connect(UdpSocket::bind("0.0.0.0:0").await.unwrap(), &cred)
        .await
        .expect("client B allocate");
    a.create_permission(b.relay).await.expect("A permission");
    b.create_permission(a.relay).await.expect("B permission");
    // A → B
    a.send_to(b"hello-builtin", b.relay).await.unwrap();
    let mut buf = [0u8; 128];
    let (n, _) = tokio::time::timeout(Duration::from_secs(5), b.recv_from(&mut buf))
        .await
        .expect("B recv timeout")
        .unwrap();
    assert_eq!(&buf[..n], b"hello-builtin");
    // B → A
    b.send_to(b"reply", a.relay).await.unwrap();
    let (n, _) = tokio::time::timeout(Duration::from_secs(5), a.recv_from(&mut buf))
        .await
        .expect("A recv timeout")
        .unwrap();
    assert_eq!(&buf[..n], b"reply");
    (a, b)
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_turn_builtin_no_auth() {
    let srv = turn_builtin_test(None).await;
    let (a, b) = turn_pair_roundtrip(srv, "frp-sh", "").await;
    drop((a, b));
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_turn_builtin_password_auth() {
    let srv = turn_builtin_test(Some("secret")).await;
    // 正确密码 → 成功
    let (a, b) = turn_pair_roundtrip(srv, "frp-sh", "secret").await;
    drop((a, b));
    // 错误密码 → 认证失败
    let cred = frp_sh::p2p::turn::TurnCredentials {
        server: srv,
        username: "frp-sh".into(),
        password: "wrong".into(),
    };
    let r =
        frp_sh::p2p::turn::TurnClient::connect(UdpSocket::bind("0.0.0.0:0").await.unwrap(), &cred)
            .await;
    assert!(r.is_err(), "wrong password should fail auth");
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_turn_builtin_frs1_stream() {
    use frp_sh::p2p::stream::UdpStream;
    use frp_sh::p2p::turn::{TurnClient, TurnCredentials};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let srv = turn_builtin_test(None).await;
    let cred = TurnCredentials {
        server: srv,
        username: "frp-sh".into(),
        password: String::new(),
    };
    let a = TurnClient::connect(UdpSocket::bind("0.0.0.0:0").await.unwrap(), &cred)
        .await
        .unwrap();
    let b = TurnClient::connect(UdpSocket::bind("0.0.0.0:0").await.unwrap(), &cred)
        .await
        .unwrap();
    a.create_permission(b.relay).await.unwrap();
    b.create_permission(a.relay).await.unwrap();
    let relay_a = a.relay;
    let relay_b = b.relay;
    let mut sa = UdpStream::new(a, relay_b, None, None);
    let mut sb = UdpStream::new(b, relay_a, None, None);
    let payload: Vec<u8> = (0..8192u32).map(|i| (i % 253) as u8).collect();
    let send = payload.clone();
    let writer = tokio::spawn(async move {
        sa.write_all(&send).await.unwrap();
        sa.shutdown().await.unwrap();
    });
    let mut got = Vec::new();
    sb.read_to_end(&mut got).await.unwrap();
    writer.await.unwrap();
    assert_eq!(got, payload, "FRS1 over built-in TURN mismatch");
}

/// 会话级 TURN 回退链：force_relay（跳过打洞）+ 配置 turn_providers 指向内置 TURN，
/// host/guest 经 TURN 中继交换数据（而不是私有 TCP 中继）。
#[tokio::test(flavor = "multi_thread")]
async fn e2e_turn_chain_relay() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .try_init();
    let turn_addr = turn_builtin_test(None).await;
    let srv = start_server().await;
    let room_id = create_room(&srv.cfg).await;
    let tp = vec![format!("turn://127.0.0.1:{}", turn_addr.port())];
    let mut cfg_h = srv.cfg.clone();
    cfg_h.turn_providers = tp.clone();
    let mut cfg_g = srv.cfg.clone();
    cfg_g.turn_providers = tp;

    let echo = spawn_echo_server().await;
    let cfg_h2 = cfg_h.clone();
    let room_h = room_id.clone();
    let host = tokio::spawn(async move {
        commands::host_session(
            &cfg_h2,
            &room_h,
            echo.to_string(),
            true, // force_relay：跳过打洞，直接进入中继（TURN 优先）
            None,
            1, // max_conns
            0, // spread
            None,
            Some(1), // max_rounds
            false,
        )
        .await
    });
    let guest_listen = free_local_port().await;
    let cfg_g2 = cfg_g.clone();
    let room_g = room_id.clone();
    let guest = tokio::spawn(async move {
        commands::guest_session(
            &cfg_g2,
            &room_g,
            guest_listen.to_string(),
            true,
            None,
            1,
            0,
            None,
            Some(1),
            None,
            false,
        )
        .await
    });

    // 驱动一次 echo（经 TURN 中继的数据面）
    let payload = b"turn-chain-echo";
    let mut client = None;
    for _ in 0..200 {
        if let Ok(s) = TcpStream::connect(guest_listen).await {
            client = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut client = client.expect("guest listen not ready");
    client.write_all(payload).await.unwrap();
    let mut buf = [0u8; 64];
    let n = client.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], payload, "echo over TURN chain mismatch");
    drop(client);

    let host_res = tokio::time::timeout(Duration::from_secs(60), host)
        .await
        .expect("host timeout")
        .unwrap();
    let guest_res = tokio::time::timeout(Duration::from_secs(60), guest)
        .await
        .expect("guest timeout")
        .unwrap();
    assert!(host_res.is_ok(), "host session failed: {host_res:?}");
    assert!(guest_res.is_ok(), "guest session failed: {guest_res:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_mesh_relay_pairing() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .try_init();
    let srv = start_server().await;
    let room_id = create_room(&srv.cfg).await;
    let cfg_h = srv.cfg.clone();
    let room_h = room_id.clone();
    // host 网格会话：强制中继，验证按 UUID 配对
    let host = tokio::spawn(async move {
        commands::host_mesh_session(&cfg_h, &room_h, true, None, 0, None, false, Some(1)).await
    });
    // 模拟访客：先加入房间（host 据此知道要为它建中继），再带 UUID 连中继
    let cfg_g = srv.cfg.clone();
    let room_g = room_id.clone();
    let guest_uuid = "mesh-relay-guest".to_string();
    let guest = tokio::spawn(async move {
        let client =
            SignalingClient::new_with_password(&cfg_g.signaling_addr, cfg_g.password.as_deref());
        let engine = PunchEngine::bind().await.unwrap();
        let token = utils::rand_token();
        let my_ext = client
            .learn_public_addr(engine.socket(), cfg_g.signaling_udp_addr().unwrap(), &token)
            .await
            .unwrap();
        client
            .join_room(
                &room_g,
                my_ext,
                Vec::new(),
                Some(guest_uuid.clone()),
                None,
                Vec::new(),
                None,
                None,
            )
            .await
            .unwrap();
        let relay_addr: SocketAddr = cfg_g.relay_addr.parse().unwrap();
        let stream = frp_sh::p2p::relay::connect(
            relay_addr,
            &room_g,
            frp_sh::p2p::relay::RelayRole::Guest,
            None,
            false,
            Some(&guest_uuid),
        )
        .await;
        match stream {
            Ok((mut s, _st)) => {
                // 配对成功后双向拷贝生效：写入的数据应到达房主侧
                let _ = s.write_all(b"mesh-relay-ok").await;
                tokio::time::sleep(Duration::from_millis(600)).await;
                true
            }
            Err(_) => false,
        }
    });
    let g = tokio::time::timeout(Duration::from_secs(30), guest)
        .await
        .expect("guest relay timeout")
        .unwrap();
    assert!(g, "guest relay connect failed");
    let host_res = tokio::time::timeout(Duration::from_secs(60), host)
        .await
        .expect("host mesh session timeout")
        .unwrap();
    assert!(
        host_res.is_ok(),
        "host mesh relay session failed: {host_res:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn room_lifecycle_api() {
    let srv = start_server().await;
    let client =
        SignalingClient::new_with_password(&srv.cfg.signaling_addr, srv.cfg.password.as_deref());
    let engine = PunchEngine::bind().await.unwrap();
    let token = utils::rand_token();
    let my_ext = client
        .learn_public_addr(
            engine.socket(),
            srv.cfg.signaling_udp_addr().unwrap(),
            &token,
        )
        .await
        .unwrap();

    let created = client
        .create_room(
            "api",
            60,
            my_ext,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            "0.0.0".into(),
            None,
            None,
        )
        .await
        .unwrap();
    let info = client.get_room(&created.room_id).await.unwrap();
    assert_eq!(info.host_addr, my_ext);
    assert!(info.guest_addr.is_none());

    let joined = client
        .join_room(
            &created.room_id,
            my_ext,
            Vec::new(),
            None,
            None,
            Vec::new(),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(joined.host_addr, my_ext);
    let info = client.get_room(&created.room_id).await.unwrap();
    assert_eq!(info.guest_addr, Some(my_ext));

    client.delete_room(&created.room_id).await.unwrap();
    let err = client.get_room(&created.room_id).await.unwrap_err();
    assert!(matches!(err, frp_sh::error::FrpError::RoomNotFound(_)));
}

#[tokio::test(flavor = "multi_thread")]
async fn guest_ip_pool_assigns_stable_ips() {
    let srv = start_server().await;
    let client =
        SignalingClient::new_with_password(&srv.cfg.signaling_addr, srv.cfg.password.as_deref());
    let engine = PunchEngine::bind().await.unwrap();
    let token = utils::rand_token();
    let my_ext = client
        .learn_public_addr(
            engine.socket(),
            srv.cfg.signaling_udp_addr().unwrap(),
            &token,
        )
        .await
        .unwrap();

    let created = client
        .create_room(
            "ip",
            60,
            my_ext,
            Some("10.66.0.1".into()),
            Vec::new(),
            Vec::new(),
            vec!["10.66.0.2".into(), "10.66.0.3".into(), "10.66.0.4".into()],
            "0.0.0".into(),
            None,
            None,
        )
        .await
        .unwrap();

    // 同一访客（同 UUID）两次加入 → 复用同一 IP
    let uid = "123e4567-e89b-12d3-a456-426614174000";
    let j1 = client
        .join_room(
            &created.room_id,
            my_ext,
            Vec::new(),
            Some(uid.into()),
            None,
            Vec::new(),
            None,
            None,
        )
        .await
        .unwrap();
    let j2 = client
        .join_room(
            &created.room_id,
            my_ext,
            Vec::new(),
            Some(uid.into()),
            None,
            Vec::new(),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(j1.assigned_ip.as_deref(), Some("10.66.0.2"));
    assert_eq!(j1.assigned_ip, j2.assigned_ip, "同设备应复用同一 IP");

    // 第二个访客 → 下一个 IP
    let j3 = client
        .join_room(
            &created.room_id,
            my_ext,
            Vec::new(),
            Some("223e4567-e89b-12d3-a456-426614174000".into()),
            None,
            Vec::new(),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(j3.assigned_ip.as_deref(), Some("10.66.0.3"));

    // 未启用 IP 池时 assigned_ip 为 None
    let created2 = client
        .create_room(
            "ip2",
            60,
            my_ext,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            "0.0.0".into(),
            None,
            None,
        )
        .await
        .unwrap();
    let j4 = client
        .join_room(
            &created2.room_id,
            my_ext,
            Vec::new(),
            Some(uid.into()),
            None,
            Vec::new(),
            None,
            None,
        )
        .await
        .unwrap();
    assert!(j4.assigned_ip.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_relay_with_server_password_encrypted() {
    // 服务器设密码：请求认证 + 中继流量加密（force_relay 走中继）
    let srv = start_server_with_password(Some("test-secret-pw")).await;
    let room_id = create_room(&srv.cfg).await;
    let (host, guest) = run_session(srv.cfg, room_id, true, None, 1).await;
    assert!(host.is_ok(), "encrypted relay host failed: {host:?}");
    assert!(guest.is_ok(), "encrypted relay guest failed: {guest:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn server_auth_rejects_wrong_password() {
    let srv = start_server_with_password(Some("correct-pw")).await;
    // 未配置密码的客户端访问 → 401
    let bad = SignalingClient::new(&srv.cfg.signaling_addr);
    let engine = PunchEngine::bind().await.unwrap();
    let token = utils::rand_token();
    let my_ext = bad
        .learn_public_addr(
            engine.socket(),
            srv.cfg.signaling_udp_addr().unwrap(),
            &token,
        )
        .await
        .unwrap();
    let err = bad
        .create_room(
            "auth",
            60,
            my_ext,
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            "0.0.0".into(),
            None,
            None,
        )
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("password"),
        "expected password error, got: {err}"
    );
}
