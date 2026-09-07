//! Full-screen room view. Rendering is pure; no addresses beyond virtual IPs enter it.
use crate::{
    i18n::text as tr,
    stats::{LinkRow, RoomView, SessionInfo},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Debug)]
pub struct DeviceCard {
    pub name: String,
    pub ip: String,
    pub role: String,
    pub state: String,
    pub detail: String,
    pub local: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub x: u16,
    pub y: u16,
    pub text: String,
    pub tone: u8,
}
pub struct Frame {
    pub spans: Vec<Span>,
    pub pages: usize,
}

pub fn clean(value: &str, width: usize) -> String {
    let value: String = crate::debuglog::redact(value)
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if value.width() <= width {
        return value;
    }
    let mut result = String::new();
    let mut used = 0;
    for c in value.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        result.push(c);
        used += w;
    }
    if width > 0 {
        result.push('…');
    }
    result
}

pub fn cards(info: &SessionInfo, view: &RoomView, links: &[LinkRow]) -> Vec<DeviceCard> {
    let host = info.mode.ends_with("host");
    let own_name = view
        .room
        .as_ref()
        .and_then(|r| r.guests.iter().find(|g| g.uuid == info.my_id))
        .and_then(|g| g.name.clone())
        .unwrap_or_else(|| info.device_name.clone());
    let mut result = vec![DeviceCard {
        name: if own_name.is_empty() {
            tr("This device", "本机").into()
        } else {
            own_name
        },
        ip: if info.vnet_ip.is_empty() {
            tr("Preparing network", "准备网络").into()
        } else {
            info.vnet_ip.clone()
        },
        role: if host {
            tr("YOU / HOST", "本机 / 房主")
        } else {
            tr("YOU", "本机")
        }
        .into(),
        state: tr("LOCAL", "本地").into(),
        detail: tr("Private device network", "独立虚拟网络").into(),
        local: true,
    }];
    let Some(room) = &view.room else {
        return result;
    };
    let mut add = |name: String, ip: String, role: &str, link: Option<&LinkRow>| {
        let (state, detail) = match link {
            Some(link) => (
                match link.1.as_str() {
                    "direct" => tr("DIRECT", "直连"),
                    "turn" => "TURN",
                    _ => tr("RELAY", "中继"),
                }
                .into(),
                if link.7 > 0 {
                    format!(
                        "{:.0} ms  ·  {}",
                        link.7 as f64 / 1000.0,
                        tr("latency", "延迟")
                    )
                } else if std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64
                    - link.3
                    >= 12
                {
                    tr("Latency unavailable", "延迟暂不可用").into()
                } else {
                    tr("Measuring latency", "正在测量延迟").into()
                },
            ),
            None => (
                tr("IN ROOM", "房间成员").into(),
                if host {
                    tr("Waiting for link", "等待连接")
                } else {
                    tr("Reachable through host when connected", "连接后经房主互通")
                }
                .into(),
            ),
        };
        result.push(DeviceCard {
            name,
            ip,
            role: role.into(),
            state,
            detail,
            local: false,
        });
    };
    if !host {
        add(
            room.host_name
                .clone()
                .unwrap_or_else(|| tr("Host", "房主").into()),
            room.tun_ip.clone().unwrap_or_default(),
            tr("HOST", "房主"),
            links.iter().find(|l| l.0 == "host"),
        );
    }
    let mut guests: Vec<_> = room
        .guests
        .iter()
        .filter(|g| g.uuid != info.my_id)
        .collect();
    guests.sort_by(|a, b| a.uuid.cmp(&b.uuid));
    for guest in guests {
        let name = guest
            .name
            .clone()
            .unwrap_or_else(|| tr("Device", "设备").into());
        let link = if host {
            links.iter().find(|l| l.0 == guest.uuid || l.0 == name)
        } else {
            None
        };
        add(
            name,
            guest
                .vnet_ip
                .clone()
                .unwrap_or_else(|| crate::utils::derive_vnet_ip(&guest.uuid)),
            tr("MEMBER", "成员"),
            link,
        );
    }
    result
}

pub fn render(
    width: u16,
    height: u16,
    info: &SessionInfo,
    view: &RoomView,
    links: &[LinkRow],
    page: usize,
    tick: usize,
) -> Frame {
    let mut frame = Frame {
        spans: Vec::new(),
        pages: 1,
    };
    let mut put = |x: usize, y: usize, s: String, tone: u8| {
        if x < width as usize && y < height as usize {
            frame.spans.push(Span {
                x: x as u16,
                y: y as u16,
                text: clean(&s, width as usize - x),
                tone,
            });
        }
    };
    if width < 40 || height < 18 {
        put(1, 1, "frp.sh".into(), 1);
        put(
            1,
            3,
            tr(
                "Enlarge terminal to view devices",
                "请放大终端以查看设备卡片",
            )
            .into(),
            0,
        );
        put(1, 5, format!("{} {}", tr("Room", "房间"), info.room), 1);
        put(1, 7, tr("Q / Ctrl+C  Leave", "Q / Ctrl+C  退出").into(), 2);
        return frame;
    }
    let w = (width as usize - 4).min(116);
    let x = (width as usize - w) / 2;
    let devices = cards(info, view, links);
    put(x, 1, "frp.sh".into(), 1);
    put(x + 9, 1, tr("/  SHARED SPACE", "/  共享空间").into(), 2);
    put(x + w - 8, 1, format!("v{}", crate::version::VERSION), 2);
    put(
        x,
        3,
        format!(
            "{}  {}",
            tr("ROOM", "房间"),
            if info.room.is_empty() {
                "····"
            } else {
                &info.room
            }
        ),
        1,
    );
    let status = if view.stale {
        tr("Reconnecting room directory", "正在恢复成员列表")
    } else if info.room.is_empty() {
        tr("Preparing your room", "正在准备房间")
    } else if links.is_empty() {
        tr("Waiting for a connection", "等待设备连接")
    } else {
        tr("Your private space is connected", "设备已连接到共享空间")
    };
    let pulse = ["·", "•", "●", "•"][tick % 4];
    put(
        x,
        4,
        format!("{pulse}  {status}"),
        if view.stale { 3 } else { 0 },
    );
    put(x, 6, "─".repeat(w), 2);
    let sent: u64 = links.iter().map(|l| l.4).sum();
    let recv: u64 = links.iter().map(|l| l.5).sum();
    put(
        x,
        7,
        format!("{:02} {}", devices.len(), tr("DEVICES", "台设备")),
        1,
    );
    put(
        x + w / 2,
        7,
        format!("↑ {}   ↓ {}", bytes(sent), bytes(recv)),
        2,
    );
    let cols = if w >= 104 {
        3
    } else if w >= 70 {
        2
    } else {
        1
    };
    let rows = ((height as usize - 14) / 7).max(1);
    let per_page = cols * rows;
    let pages = devices.len().div_ceil(per_page).max(1);
    frame.pages = pages;
    let current = page % pages;
    let cw = (w - (cols - 1) * 2) / cols;
    for (index, device) in devices
        .iter()
        .skip(current * per_page)
        .take(per_page)
        .enumerate()
    {
        let cx = x + (index % cols) * (cw + 2);
        let cy = 9 + (index / cols) * 7;
        let border = if device.local { 1 } else { 2 };
        put(cx, cy, format!("╭{}╮", "─".repeat(cw - 2)), border);
        for dy in 1..5 {
            put(cx, cy + dy, "│".into(), border);
            put(cx + cw - 1, cy + dy, "│".into(), border);
        }
        put(cx, cy + 5, format!("╰{}╯", "─".repeat(cw - 2)), border);
        put(cx + 2, cy + 1, clean(&device.name, cw - 4), 0);
        put(
            cx + 2,
            cy + 2,
            clean(&format!("{}  ·  {}", device.role, device.state), cw - 4),
            border,
        );
        put(cx + 2, cy + 3, clean(&device.ip, cw - 4), 1);
        put(cx + 2, cy + 4, clean(&device.detail, cw - 4), 2);
    }
    let footer = height as usize - 3;
    put(
        x,
        footer,
        format!(
            "{}  ·  {} / {}",
            if view.expose_lan {
                tr("LAN sharing enabled", "已开启局域网共享")
            } else {
                tr("LAN sharing off", "未共享本地局域网")
            },
            current + 1,
            pages
        ),
        if view.expose_lan { 3 } else { 2 },
    );
    put(
        x,
        footer + 1,
        tr(
            "← → Devices   I Invite   G Logs   L Language   Q Leave",
            "← → 翻页   I 邀请   G 日志   L 中英文   Q 退出",
        )
        .into(),
        2,
    );
    frame
}
fn bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MiB", n as f64 / 1048576.0)
    } else {
        format!("{:.1} KiB", n as f64 / 1024.0)
    }
}
