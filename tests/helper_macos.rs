#![cfg(target_os = "macos")]
use std::{process::Command, time::Duration};
use tokio::io::AsyncReadExt;

fn route(ip: &str) -> String {
    let output = Command::new("/sbin/route")
        .args(["-n", "get", ip])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[tokio::test]
#[ignore = "requires the installed helper in an isolated macOS CI runner"]
async fn self_ping_uses_loopback_without_tunnel_traffic() {
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    let ip = "10.66.0.40";
    let mut device = frp_sh::helper::open(&frp_sh::p2p::tun::TunConfig {
        allow_lan: false,
        name: "frp1".into(),
        ip: ip.into(),
        netmask: "255.255.255.0".into(),
        mtu: 1400,
    })
    .await
    .unwrap();
    assert!(
        route(ip).contains("interface: lo0"),
        "self route: {}",
        route(ip)
    );
    assert!(route("10.66.0.1").contains(&format!("interface: {}", device.name())));
    println!("self route: {}", route(ip));
    let result = Command::new("/sbin/ping")
        .args(["-n", "-c", "3", "-W", "1000", ip])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "self ping failed: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let mut packet = [0; 2048];
    assert!(
        tokio::time::timeout(Duration::from_millis(300), device.read(&mut packet))
            .await
            .is_err(),
        "self ping entered the tunnel even without a remote peer"
    );
    drop(device);
    for _ in 0..50 {
        if !route(ip).contains("interface: lo0") {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("self route leaked after device lease ended");
}
