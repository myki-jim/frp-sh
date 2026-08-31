//! 真实网络互操作测试（默认忽略，手动运行）：
//! `cargo test --test turn_live -- --ignored --nocapture`
//!
//! - STUN：连 `stun.cloudflare.com:3478` 学习公网地址（无需凭据）
//! - TURN：连自建 coturn（VPS 或本地），验证 Allocate/CreatePermission/
//!   Send-Data 全链路与 FRS1 流经 TURN 中继传输
//!
//! 运行前确保 TURN 服务器可用（`TURN_SERVER` 环境变量，默认 101.43.41.195:3478，
//! 账号 frpshtest/testpass，realm frp.sh）。

use frp_sh::p2p::stun;
use frp_sh::p2p::turn::{DatagramSocket, TurnClient, TurnCredentials};
use tokio::net::UdpSocket;

/// 初始化日志（RUST_LOG 控制级别）。
fn init_log() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug"))
        .try_init();
}

fn turn_cred() -> TurnCredentials {
    TurnCredentials {
        server: std::env::var("TURN_SERVER")
            .unwrap_or_else(|_| "101.43.41.195:3478".into())
            .parse()
            .unwrap(),
        username: "frpshtest".into(),
        password: "testpass".into(),
    }
}

#[tokio::test]
#[ignore]
async fn stun_binding() {
    // 用 TURN 服务器自带的 STUN 端点验证 Binding 探测（coturn 默认提供 STUN；
    // 也可改连 stun.cloudflare.com:3478 实测公共 STUN）
    let s = UdpSocket::bind("0.0.0.0:0").await.unwrap();
    let server: std::net::SocketAddr = std::env::var("TURN_SERVER")
        .unwrap_or_else(|_| "101.43.41.195:3478".into())
        .parse()
        .unwrap();
    let addr = stun::binding_probe(&s, server).await;
    println!("STUN mapped address: {addr:?}");
    assert!(addr.is_ok(), "STUN binding failed");
}

#[tokio::test]
#[ignore]
async fn dbg_raw_allocate() {
    use std::io::Write;
    let s = UdpSocket::bind("0.0.0.0:0").await.unwrap();
    let txid = stun::new_txid();
    let rtp = stun::encode_requested_transport();
    let req = stun::build(
        stun::METHOD_ALLOCATE,
        0,
        &txid,
        &[(stun::ATTR_REQUESTED_TRANSPORT, &rtp)],
    );
    println!("sending {} bytes, head: {:02x?}", req.len(), &req[..20]);
    std::io::stdout().flush().unwrap();
    let server: std::net::SocketAddr = std::env::var("TURN_SERVER")
        .unwrap_or_else(|_| "101.43.41.195:3478".into())
        .parse()
        .unwrap();
    s.send_to(&req, server).await.unwrap();
    let mut buf = [0u8; 2048];
    match tokio::time::timeout(std::time::Duration::from_secs(3), s.recv_from(&mut buf)).await {
        Ok(Ok((n, src))) => {
            println!("recv {n} bytes from {src}: {:02x?}", &buf[..n.min(64)])
        }
        Ok(Err(e)) => println!("recv err: {e}"),
        Err(_) => println!("TIMEOUT"),
    }
    std::io::stdout().flush().unwrap();
}

#[tokio::test]
#[ignore]
async fn turn_allocate_and_permission() {
    init_log();
    let cred = turn_cred();
    let a = TurnClient::connect(UdpSocket::bind("0.0.0.0:0").await.unwrap(), &cred)
        .await
        .expect("allocate A failed");
    println!("relay A: {}", a.relay);
    assert!(a.relay.ip().is_ipv4());
    // 对端地址必须被授权，否则服务器拒绝转发
    let fake_peer: std::net::SocketAddr = "203.0.113.9:9999".parse().unwrap();
    a.create_permission(fake_peer)
        .await
        .expect("create-permission failed");
}

#[tokio::test]
#[ignore]
async fn turn_relay_roundtrip() {
    let cred = turn_cred();
    let a = TurnClient::connect(UdpSocket::bind("0.0.0.0:0").await.unwrap(), &cred)
        .await
        .expect("allocate A failed");
    let b = TurnClient::connect(UdpSocket::bind("0.0.0.0:0").await.unwrap(), &cred)
        .await
        .expect("allocate B failed");
    println!("relay A: {}  relay B: {}", a.relay, b.relay);
    // 互相授权（同一 TURN 服务器上，授权对方 relay 地址）
    a.create_permission(b.relay).await.unwrap();
    b.create_permission(a.relay).await.unwrap();

    // A → B（经 TURN 转发）
    a.send_to(b"hello-via-turn", b.relay).await.unwrap();
    let mut buf = [0u8; 128];
    let (n, peer) = tokio::time::timeout(std::time::Duration::from_secs(5), b.recv_from(&mut buf))
        .await
        .expect("B recv timeout")
        .unwrap();
    assert_eq!(&buf[..n], b"hello-via-turn", "payload mismatch");
    println!("A→B via TURN ok, peer seen as {peer}");

    // B → A
    b.send_to(b"reply-from-b", a.relay).await.unwrap();
    let (n, _) = tokio::time::timeout(std::time::Duration::from_secs(5), a.recv_from(&mut buf))
        .await
        .expect("A recv timeout")
        .unwrap();
    assert_eq!(&buf[..n], b"reply-from-b", "reply mismatch");
    println!("B→A via TURN ok");
}

/// FRS1 可靠流经 TURN 中继传输（两段 UdpStream 跑在 TurnClient 之上）。
#[tokio::test]
#[ignore]
async fn frs1_stream_over_turn() {
    use frp_sh::p2p::stream::UdpStream;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let cred = turn_cred();
    let a = TurnClient::connect(UdpSocket::bind("0.0.0.0:0").await.unwrap(), &cred)
        .await
        .unwrap();
    let b = TurnClient::connect(UdpSocket::bind("0.0.0.0:0").await.unwrap(), &cred)
        .await
        .unwrap();
    a.create_permission(b.relay).await.unwrap();
    b.create_permission(a.relay).await.unwrap();

    // 两段 FRS1 流：各自指向对端的 TURN relay 地址
    let relay_a = a.relay;
    let relay_b = b.relay;
    let mut sa = UdpStream::new(a, relay_b, None, None);
    let mut sb = UdpStream::new(b, relay_a, None, None);

    let payload: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
    let send = payload.clone();
    let writer = tokio::spawn(async move {
        sa.write_all(&send).await.unwrap();
        sa.shutdown().await.unwrap();
    });
    let mut got = Vec::new();
    sb.read_to_end(&mut got).await.unwrap();
    writer.await.unwrap();
    assert_eq!(got, payload, "FRS1 payload over TURN mismatch");
    println!("FRS1 stream over TURN ok ({} bytes)", got.len());
}
