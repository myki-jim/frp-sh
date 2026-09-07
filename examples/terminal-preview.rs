//! Export the actual layout renderer for visual review without starting a room.
use frp_sh::{
    dashboard,
    stats::{RoomView, SessionInfo},
};
fn main() {
    frp_sh::i18n::choose(if std::env::args().any(|a| a == "--zh") {
        "zh-CN"
    } else {
        "en"
    });
    let room = serde_json::from_value(serde_json::json!({
        "room_id":"3856", "host_addr":"192.0.2.1:8080", "guest_addr":null,
        "created_at":0,"expires_at":0,"host_name":"Studio PC", "tun_ip":"10.66.0.1",
        "guests":[
            {"uuid":"mac","name":"Jimmy's MacBook Pro","addr":"192.0.2.2:1000","vnet_ip":"10.66.0.40"},
            {"uuid":"alice","name":"Alice / 工作站","addr":"192.0.2.3:1000","vnet_ip":"10.66.0.12"},
            {"uuid":"deck","name":"Steam Deck","addr":"192.0.2.4:1000","vnet_ip":"10.66.0.23"},
            {"uuid":"nas","name":"Home server","addr":"192.0.2.5:1000","vnet_ip":"10.66.0.64"}
        ]
    })).unwrap();
    let info = SessionInfo {
        mode: "lan-host".into(),
        room: "3856".into(),
        my_id: "host".into(),
        device_name: "Studio PC".into(),
        vnet_ip: "10.66.0.1".into(),
        ..Default::default()
    };
    let view = RoomView {
        room: Some(room),
        ..Default::default()
    };
    let links = vec![
        (
            "mac".into(),
            "turn".into(),
            String::new(),
            0,
            84224,
            64213,
            198000,
            198000,
        ),
        (
            "alice".into(),
            "direct".into(),
            String::new(),
            0,
            452224,
            96213,
            4000,
            4000,
        ),
    ];
    let frame = dashboard::render(112, 36, &info, &view, &links, 0, 2);
    if std::env::args().any(|a| a == "--live") {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            frp_sh::terminal::configure(false, false, false);
            frp_sh::stats::update_info(info);
            frp_sh::stats::set_room_view(view);
            for link in links {
                let stats = frp_sh::stats::StreamStats::new(frp_sh::stats::KIND_DIRECT);
                stats.on_sent(link.4 as usize);
                stats.on_recv(link.5 as usize);
                stats.on_rtt(link.7);
                frp_sh::stats::push_link(frp_sh::stats::LinkEntry {
                    peer: link.0,
                    kind: if link.1 == "turn" { "turn" } else { "direct" },
                    detail: String::new(),
                    stats,
                });
            }
            let _screen = frp_sh::terminal::monitor(true);
            frp_sh::terminal::ctrl_c().await;
        });
        return;
    }
    println!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="1120" height="760" viewBox="0 0 1120 760"><rect width="1120" height="760" rx="16" fill="#0e141c"/><g font-family="Cascadia Mono,Consolas,monospace" font-size="16">"##
    );
    for span in frame.spans {
        let text = span
            .text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let color = match span.tone {
            1 => "#7ee0c0",
            2 => "#738494",
            3 => "#ebbc71",
            _ => "#e2eaf2",
        };
        println!(
            r#"<text x="{}" y="{}" fill="{}" textLength="{}" lengthAdjust="spacingAndGlyphs" xml:space="preserve">{}</text>"#,
            span.x as u32 * 10,
            span.y as u32 * 20 + 32,
            color,
            unicode_width::UnicodeWidthStr::width(span.text.as_str()) * 10,
            text
        );
    }
    println!("</g></svg>");
}
