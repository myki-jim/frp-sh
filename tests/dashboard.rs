use frp_sh::{
    dashboard,
    stats::{RoomView, SessionInfo},
};
use unicode_width::UnicodeWidthStr;

fn fixture() -> (SessionInfo, RoomView) {
    let room = serde_json::from_value(serde_json::json!({
        "room_id":"3856", "host_addr":"192.0.2.1:8080", "guest_addr":null,
        "created_at":0, "expires_at":999999, "tun_ip":"10.66.0.1", "host_name":"Studio PC",
        "guests": (0..12).map(|i| serde_json::json!({
            "uuid":format!("device-{i}"), "name":format!("设计工作站 / Device {i}"),
            "addr":"192.0.2.2:54321", "vnet_ip":format!("10.66.0.{}",i+2)
        })).collect::<Vec<_>>()
    }))
    .unwrap();
    (
        SessionInfo {
            my_id: "device-0".into(),
            mode: "guest".into(),
            room: "3856".into(),
            vnet_ip: "10.66.0.2".into(),
            ..Default::default()
        },
        RoomView {
            room: Some(room),
            ..Default::default()
        },
    )
}

#[test]
fn every_member_is_visible_and_frames_fit_multilingual_terminals() {
    let (info, view) = fixture();
    let cards = dashboard::cards(&info, &view, &[]);
    assert_eq!(cards.len(), 13);
    assert_eq!(cards.iter().filter(|c| c.local).count(), 1);
    assert_eq!(cards[0].ip, "10.66.0.2");
    assert_eq!(cards[1].name, "Studio PC");
    for language in ["en", "zh-CN"] {
        frp_sh::i18n::choose(language);
        for (width, height) in [(20, 10), (40, 17), (80, 24), (120, 42)] {
            let frame = dashboard::render(width, height, &info, &view, &[], 0, 0);
            let mut seen = String::new();
            for page in 0..frame.pages {
                let frame = dashboard::render(width, height, &info, &view, &[], page, 0);
                for span in frame.spans {
                    assert!((span.x as usize + span.text.width()) <= width as usize);
                    assert!(span.y < height);
                    assert!(!span.text.contains('\x1b'));
                    seen.push_str(&span.text);
                }
            }
            assert!(
                !seen.contains("192.0.2."),
                "public candidates leaked into UI"
            );
            if width >= 40 && height >= 18 {
                for card in &cards {
                    assert!(seen.contains(&card.ip), "device missing from pagination");
                }
            }
        }
    }
}

#[test]
fn host_sees_each_guest_once_and_unknown_peers_have_no_fake_latency() {
    let (mut info, view) = fixture();
    info.mode = "lan-host".into();
    info.my_id = "host".into();
    let cards = dashboard::cards(&info, &view, &[]);
    assert_eq!(cards.len(), 13);
    for index in 0..12 {
        let ip = format!("10.66.0.{}", index + 2);
        assert_eq!(cards.iter().skip(1).filter(|c| c.ip == ip).count(), 1);
    }
    assert!(cards.iter().all(|c| !c.detail.contains("0 ms")));
}

#[test]
fn device_names_cannot_inject_terminal_controls() {
    let safe = dashboard::clean("Mac\x1b]52;c;payload\x07\r\n名字", 18);
    assert!(!safe.chars().any(char::is_control));
    assert!(safe.width() <= 18);
}
