//! 端到端测试：进程内启动信令服务器，验证直连打洞与中继回退两条数据通路。

use frp_sh::commands;
use frp_sh::config::Config;
use frp_sh::p2p::hole_punch::PunchEngine;
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
    // 偶发端口冲突（强杀进程的端口未完全释放）时重试
    for _ in 0..10 {
        if let Some(s) = try_start_server().await {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("cannot start test signaling server (port conflicts)");
}

async fn try_start_server() -> Option<TestServer> {
    let http_listener = TcpListener::bind("127.0.0.1:0").await.ok()?;
    let http_port = http_listener.local_addr().unwrap().port();
    // UDP 探测用独立端口（避免 Windows 上"同端口 TCP 刚释放"的绑定冲突，
    // 同时验证 config.signaling_udp 独立端口能力）
    let udp = UdpSocket::bind("127.0.0.1:0").await.ok()?;
    let udp_port = udp.local_addr().unwrap().port();
    let relay_listener = TcpListener::bind("127.0.0.1:0").await.ok()?;
    let relay_port = relay_listener.local_addr().unwrap().port();
    let state = server::new_state();
    tokio::spawn(server::run_http(http_listener, state.clone()));
    tokio::spawn(server::run_udp_echo(udp));
    tokio::spawn(server::run_relay(relay_listener, state));
    Some(TestServer {
        cfg: Config {
            signaling_addr: format!("http://127.0.0.1:{http_port}"),
            relay_addr: format!("127.0.0.1:{relay_port}"),
            signaling_udp: Some(format!("127.0.0.1:{udp_port}")),
        },
    })
}

/// 创建房间并返回 (room_id, engine, token)。
/// engine 与 token 必须原样传给 host_session，保证通告地址与打洞 socket 一致。
async fn create_room(cfg: &Config) -> (String, PunchEngine, String) {
    let client = SignalingClient::new(&cfg.signaling_addr);
    let engine = PunchEngine::bind().await.unwrap();
    let token = utils::rand_token();
    let my_ext = client
        .learn_public_addr(engine.socket(), cfg.signaling_udp_addr().unwrap(), &token)
        .await
        .unwrap();
    let resp = client.create_room("test", 120, my_ext).await.unwrap();
    (resp.room_id, engine, token)
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
    engine: PunchEngine,
    token: String,
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
            engine,
            token,
            key_h,
            max_conns,
            2,    // spread
            None, // tun
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
            2,
            None,
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

    let host_res = tokio::time::timeout(Duration::from_secs(30), host)
        .await
        .expect("host session timeout")
        .unwrap();
    let guest_res = tokio::time::timeout(Duration::from_secs(30), guest)
        .await
        .expect("guest session timeout")
        .unwrap();
    (host_res, guest_res)
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_direct_hole_punch() {
    let srv = start_server().await;
    let (room_id, engine, token) = create_room(&srv.cfg).await;
    let (host, guest) = run_session(srv.cfg, room_id, engine, token, false, None, 1).await;
    assert!(host.is_ok(), "host session failed: {host:?}");
    assert!(guest.is_ok(), "guest session failed: {guest:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_direct_multiconn() {
    let srv = start_server().await;
    let (room_id, engine, token) = create_room(&srv.cfg).await;
    let (host, guest) = run_session(srv.cfg, room_id, engine, token, false, None, 3).await;
    assert!(host.is_ok(), "host session failed: {host:?}");
    assert!(guest.is_ok(), "guest session failed: {guest:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_direct_encrypted() {
    let srv = start_server().await;
    let (room_id, engine, token) = create_room(&srv.cfg).await;
    let (host, guest) = run_session(
        srv.cfg,
        room_id,
        engine,
        token,
        false,
        Some("test-secret".into()),
        1,
    )
    .await;
    assert!(host.is_ok(), "host session failed: {host:?}");
    assert!(guest.is_ok(), "guest session failed: {guest:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn e2e_relay_fallback() {
    let srv = start_server().await;
    let (room_id, engine, token) = create_room(&srv.cfg).await;
    let (host, guest) = run_session(srv.cfg, room_id, engine, token, true, None, 1).await;
    assert!(host.is_ok(), "host session failed: {host:?}");
    assert!(guest.is_ok(), "guest session failed: {guest:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn room_lifecycle_api() {
    let srv = start_server().await;
    let client = SignalingClient::new(&srv.cfg.signaling_addr);
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

    let created = client.create_room("api", 60, my_ext).await.unwrap();
    let info = client.get_room(&created.room_id).await.unwrap();
    assert_eq!(info.host_addr, my_ext);
    assert!(info.guest_addr.is_none());

    let joined = client.join_room(&created.room_id, my_ext).await.unwrap();
    assert_eq!(joined.host_addr, my_ext);
    let info = client.get_room(&created.room_id).await.unwrap();
    assert_eq!(info.guest_addr, Some(my_ext));

    client.delete_room(&created.room_id).await.unwrap();
    let err = client.get_room(&created.room_id).await.unwrap_err();
    assert!(matches!(err, frp_sh::error::FrpError::RoomNotFound(_)));
}
