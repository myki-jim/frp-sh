#![cfg(target_os = "linux")]
use std::{
    os::unix::{fs::PermissionsExt, process::CommandExt},
    process::{Child, Command},
    time::Duration,
};

struct Service(Child);
impl Drop for Service {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.0.id() as i32, libc::SIGTERM);
        }
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires an isolated Linux container with NET_ADMIN and /dev/net/tun"]
fn helper_service_lifecycle() {
    assert_eq!(std::env::var("FRPSH_CONTAINER_TEST").as_deref(), Ok("1"));
    assert!(std::path::Path::new("/.dockerenv").exists());
    assert_eq!(unsafe { libc::geteuid() }, 0);
    std::fs::create_dir_all("/etc/frp-sh").unwrap();
    std::fs::write("/etc/frp-sh/helper.toml", "allowed_uid = 65534\n").unwrap();
    std::fs::set_permissions(
        "/etc/frp-sh/helper.toml",
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let service = Service(
        Command::new(env!("CARGO_BIN_EXE_frp-sh-net"))
            .spawn()
            .unwrap(),
    );
    for _ in 0..100 {
        if std::path::Path::new("/var/run/frp-sh/network.sock").exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "helper_service_child",
            "--nocapture",
        ])
        .env("FRPSH_HELPER_CHILD", "1")
        .uid(65534)
        .gid(65534)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for _ in 0..100 {
        let route = Command::new("ip")
            .args(["route", "show", "192.168.233.0/24"])
            .output()
            .unwrap();
        if route.stdout.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        Command::new("ip")
            .args(["route", "show", "192.168.233.0/24"])
            .output()
            .unwrap()
            .stdout
            .is_empty(),
        "session route leaked"
    );
    let outsider = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "helper_service_child"])
        .env("FRPSH_HELPER_CHILD", "unauthorized")
        .uid(65533)
        .gid(65533)
        .output()
        .unwrap();
    assert!(
        outsider.status.success(),
        "unauthorized access was not rejected"
    );
    drop(service);
    assert!(!std::path::Path::new("/var/run/frp-sh/network.sock").exists());
}

#[tokio::test]
#[ignore = "spawned by helper_service_lifecycle as an unprivileged account"]
async fn helper_service_child() {
    let mode = std::env::var("FRPSH_HELPER_CHILD").expect("parent-only test");
    if mode == "unauthorized" {
        assert!(frp_sh::helper::status().await.is_err());
        return;
    }
    assert_eq!(unsafe { libc::geteuid() }, 65534);
    frp_sh::helper::status().await.unwrap();
    let config = frp_sh::p2p::tun::TunConfig {
        name: "frp0".into(),
        ip: "10.66.0.1".into(),
        netmask: "255.255.255.0".into(),
        mtu: 1400,
    };
    let device = frp_sh::helper::open(&config).await.unwrap();
    assert!(
        frp_sh::helper::open(&config).await.is_err(),
        "duplicate role accepted"
    );
    frp_sh::helper::route(device.name(), "192.168.233.0/24")
        .await
        .unwrap();
    assert!(frp_sh::helper::route(device.name(), "0.0.0.0/0")
        .await
        .is_err());
    drop(device);
    tokio::time::sleep(Duration::from_millis(100)).await;
}
