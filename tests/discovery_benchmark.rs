//! Repeatable discovery-stage measurements; deliberately excluded from routine tests.
//! Baseline reproduces e1037af's sequential STUN -> echo algorithm, using the same
//! unchanged STUN implementation. This does not measure complete Internet sessions.
use frp_sh::{p2p::stun, signaling::SignalingClient};
use tokio::net::UdpSocket;
#[tokio::test]
#[ignore = "30 samples per path, approximately 90 seconds"]
async fn address_discovery_benchmark() {
    let echo = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = echo.local_addr().unwrap();
    let task = tokio::spawn(async move {
        let mut buf = [0; 2048];
        loop {
            let (n, peer) = echo.recv_from(&mut buf).await.unwrap();
            let request = String::from_utf8_lossy(&buf[..n]);
            if let Some(token) = request.strip_prefix("ECHO ") {
                echo.send_to(format!("ADDR {token} {peer}").as_bytes(), peer)
                    .await
                    .unwrap();
            }
        }
    });
    let sink = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client = SignalingClient::new("http://127.0.0.1:1");
    for blocked_stun in [false, true] {
        for parallel in [false, true] {
            let mut samples = Vec::new();
            for _ in 0..30 {
                let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let start = std::time::Instant::now();
                let turn = if blocked_stun {
                    Some(sink.local_addr().unwrap())
                } else {
                    None
                };
                let discovered = if parallel {
                    client
                        .learn_public_addr_auto(&socket, addr, "bench", turn)
                        .await
                        .unwrap()
                } else {
                    if let Some(turn) = turn {
                        assert!(stun::binding_probe(&socket, turn).await.is_err());
                    }
                    client
                        .learn_public_addr(&socket, addr, "bench")
                        .await
                        .unwrap()
                };
                assert_eq!(discovered, socket.local_addr().unwrap());
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            samples.sort_by(f64::total_cmp);
            println!(
                "{}",
                serde_json::json!({"stage":"address_discovery","samples":30,"blocked_stun":blocked_stun,"parallel":parallel,"p50_ms":samples[14],"p95_ms":samples[28],"success":30})
            );
        }
    }
    task.abort();
}
