//! Local-only companion. No timers, sockets or terminal escape sequences in artwork.
mod art;
use crate::{
    dashboard::{clean, Frame, Span},
    i18n::text as tr,
    stats::{LinkRow, RoomView, SessionInfo},
};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub look: usize,
    pub visible: bool,
    pub reduced_motion: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            look: 0,
            visible: true,
            reduced_motion: false,
        }
    }
}
static SETTINGS: OnceLock<Mutex<Settings>> = OnceLock::new();
pub fn configure(mut s: Settings) {
    s.look = s.look.min(19);
    *SETTINGS
        .get_or_init(|| Mutex::new(Settings::default()))
        .lock()
        .unwrap() = s;
}
pub fn settings() -> Settings {
    SETTINGS
        .get_or_init(|| Mutex::new(Settings::default()))
        .lock()
        .unwrap()
        .clone()
}
pub fn name(id: usize) -> String {
    let id = id.min(19);
    format!(
        "{} / {}",
        tr(art::SPECIES[id / 4].0, art::SPECIES[id / 4].1),
        tr(art::RIGS[id % 4].0, art::RIGS[id % 4].1)
    )
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mood {
    #[default]
    Rest,
    Serving,
    Partial,
    Waiting,
    Connecting,
    Direct,
    Relay,
    Unstable,
    Reconnecting,
}
impl Mood {
    fn label(self) -> &'static str {
        match self {
            Self::Serving => tr("Services ready", "服务已就绪"),
            Self::Partial => tr("Some links are not ready", "部分成员链路未就绪"),
            Self::Rest => tr("Resting", "休息中"),
            Self::Waiting => tr("Waiting for friends", "等待朋友"),
            Self::Connecting => tr("Establishing connection", "建立连接中"),
            Self::Direct => tr("Direct connection", "直连正常"),
            Self::Relay => tr("Relay connection", "中继正常"),
            Self::Unstable => tr("Network unstable", "网络波动"),
            Self::Reconnecting => tr("Reconnecting", "重新连接中"),
        }
    }
}
pub fn observe(info: &SessionInfo, view: &RoomView, links: &[LinkRow]) -> Mood {
    if info.room.is_empty() {
        return Mood::Rest;
    }
    if view.stale {
        return Mood::Unstable;
    }
    if links.is_empty() {
        if info.reconnects > 0 {
            return Mood::Reconnecting;
        }
        return if info.mode == "host" && view.room.as_ref().is_some_and(|r| r.guests.is_empty()) {
            Mood::Waiting
        } else {
            Mood::Connecting
        };
    }
    if !links.iter().any(|l| l.5 > 0 || l.7 > 0) {
        return Mood::Connecting;
    }
    if info.mode == "host"
        && view
            .room
            .as_ref()
            .is_some_and(|r| r.guests.len() > links.len())
    {
        return Mood::Partial;
    }
    if links.iter().any(|l| l.7 > 200_000) {
        Mood::Unstable
    } else if links.iter().any(|l| l.1 != "direct") {
        Mood::Relay
    } else {
        Mood::Direct
    }
}
#[derive(Default)]
pub struct Tracker {
    pending: Option<(Mood, usize)>,
    current: Option<Mood>,
}
impl Tracker {
    pub fn update(&mut self, raw: Mood, tick: usize) -> Mood {
        // Immediate disconnect feedback; quality fluctuations require five seconds.
        if self.current.is_none()
            || (raw != Mood::Unstable && self.current != Some(Mood::Unstable))
            || matches!(
                raw,
                Mood::Reconnecting
                    | Mood::Connecting
                    | Mood::Rest
                    | Mood::Waiting
                    | Mood::Serving
                    | Mood::Partial
            )
        {
            self.current = Some(raw);
            self.pending = None;
        } else if self.current != Some(raw) {
            match self.pending {
                Some((p, start)) if p == raw && tick.saturating_sub(start) >= 50 => {
                    self.current = Some(raw);
                    self.pending = None;
                }
                Some((p, _)) if p == raw => {}
                _ => self.pending = Some((raw, tick)),
            }
        } else {
            self.pending = None;
        }
        self.current.unwrap_or(raw)
    }
}
pub fn artwork(id: usize, mood: Mood, tick: usize, reduced: bool) -> Vec<String> {
    let id = id.min(19);
    let mut grid = vec![vec![b' '; 44]; 13];
    for (y, line) in art::BODIES[id / 4].lines().enumerate() {
        for (x, b) in line.bytes().take(44).enumerate() {
            grid[y + 1][x] = b;
        }
    }
    for (y, line) in art::EQUIPMENT[id % 4].iter().enumerate() {
        for (x, b) in line.bytes().enumerate() {
            if b != b' ' {
                grid[y + 5][18 + x] = b;
            }
        }
    }
    let phase = if reduced { 0 } else { (tick / 5) % 4 };
    let signal = match mood {
        Mood::Rest | Mood::Waiting | Mood::Serving => " . ",
        Mood::Partial => ": :",
        Mood::Direct => "===",
        Mood::Relay => "=+=",
        Mood::Unstable => "! !",
        Mood::Connecting | Mood::Reconnecting => [".  ", ".. ", "...", " .."][phase],
    };
    grid[0][20..23].copy_from_slice(signal.as_bytes());
    // Separate antenna rises while negotiating; body remains anchored to its pad.
    if matches!(mood, Mood::Connecting | Mood::Reconnecting | Mood::Unstable) {
        grid[1][21] = b'|';
        grid[2][21] = b'|';
    }
    if !reduced && matches!(mood, Mood::Rest | Mood::Waiting) && tick % 80 >= 70 {
        for row in &mut grid {
            if let Some(x) = row.iter().position(|b| *b == b'o') {
                row[x] = b'-';
                break;
            }
        }
    }
    grid.into_iter()
        .map(|r| String::from_utf8(r).unwrap())
        .collect()
}
fn put(frame: &mut Frame, x: u16, y: u16, w: u16, h: u16, text: String, tone: u8) {
    if x < w && y < h {
        frame.spans.push(Span {
            x,
            y,
            text: clean(&text, (w - x) as usize),
            tone,
        });
    }
}
pub fn space(w: u16, h: u16, s: &Settings) -> u16 {
    if s.visible && w >= 104 && h >= 22 {
        w - 46
    } else {
        w
    }
}
pub fn content_height(w: u16, h: u16, s: &Settings) -> u16 {
    if s.visible && (64..104).contains(&w) && h >= 24 {
        h - 8
    } else {
        h
    }
}
pub fn append(frame: &mut Frame, w: u16, h: u16, s: &Settings, mood: Mood, tick: usize) {
    if space(w, h, s) == w {
        if content_height(w, h, s) == h {
            return;
        }
        let x = (w - 44) / 2;
        let y = h - 7;
        put(frame, x, y, w, h, name(s.look), 1);
        for (i, line) in art::COMPACT[s.look.min(19) / 4].lines().enumerate() {
            put(frame, x, y + 1 + i as u16, w, h, line.into(), 2);
        }
        put(
            frame,
            x + 15,
            y + 2,
            w,
            h,
            ["[:::]", "[=+=]", "[/*/]", "[#:#]"][s.look.min(19) % 4].into(),
            1,
        );
        let phase = if s.reduced_motion { 0 } else { (tick / 5) % 4 };
        let beacon = match mood {
            Mood::Connecting | Mood::Reconnecting => ["[.  ]", "[.. ]", "[...]", "[ ..]"][phase],
            Mood::Unstable | Mood::Partial => "[ ! ]",
            Mood::Direct => "[===]",
            Mood::Relay => "[=+=]",
            _ => "[ . ]",
        };
        put(frame, x + 33, y + 2, w, h, beacon.into(), 1);
        put(frame, x, y + 5, w, h, mood.label().into(), 1);
        put(
            frame,
            x,
            y + 6,
            w,
            h,
            tr("P Companion", "P 宠物造型").into(),
            2,
        );
        return;
    }
    let x = w - 44;
    let y = h.saturating_sub(18);
    put(frame, x, y, w, h, name(s.look), 1);
    for (i, line) in artwork(s.look, mood, tick, s.reduced_motion)
        .into_iter()
        .enumerate()
    {
        put(
            frame,
            x,
            y + 1 + i as u16,
            w,
            h,
            line,
            if mood == Mood::Unstable { 3 } else { 2 },
        );
    }
    put(frame, x, y + 14, w, h, mood.label().into(), 1);
    put(
        frame,
        x,
        y + 16,
        w,
        h,
        tr("P  Companion wardrobe", "P  宠物造型").into(),
        2,
    );
}
pub fn wardrobe(w: u16, h: u16, selected: usize, s: &Settings, tick: usize, notice: &str) -> Frame {
    let mut frame = Frame {
        spans: vec![],
        pages: 1,
    };
    put(
        &mut frame,
        2,
        1,
        w,
        h,
        tr("Companion / 20 complete looks", "终端宠物 / 20 款完整造型").into(),
        1,
    );
    let count = (h.saturating_sub(8) as usize).clamp(1, 20);
    let start = selected / count * count;
    for (i, id) in (start..20).take(count).enumerate() {
        put(
            &mut frame,
            2,
            4 + i as u16,
            w.min(34),
            h,
            format!(
                "{} {:02} {}",
                if id == selected { ">" } else { " " },
                id + 1,
                name(id)
            ),
            if id == selected { 1 } else { 2 },
        );
    }
    if w >= 78 && h >= 20 {
        for (i, line) in artwork(selected, Mood::Direct, tick, s.reduced_motion)
            .into_iter()
            .enumerate()
        {
            put(&mut frame, 34, 4 + i as u16, w, h, line, 1);
        }
        put(
            &mut frame,
            34,
            18,
            w,
            h,
            tr("Preview / direct connection", "预览 / 直连状态").into(),
            2,
        );
    }
    put(
        &mut frame,
        2,
        h.saturating_sub(4),
        w,
        h,
        format!(
            "{}: {}   {}: {}",
            tr("Visible", "显示"),
            s.visible,
            tr("Reduced motion", "减少动画"),
            s.reduced_motion
        ),
        2,
    );
    put(&mut frame, 2, h.saturating_sub(3), w, h, notice.into(), 1);
    put(
        &mut frame,
        2,
        h.saturating_sub(2),
        w,
        h,
        tr(
            "Up/Down Select  Enter Save  H Hide/show  M Motion  Esc Back",
            "上下选择  Enter 保存  H 显示/隐藏  M 动画  Esc 返回",
        )
        .into(),
        2,
    );
    frame
}

static PATH: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
pub fn initialize(path: Option<std::path::PathBuf>, settings: Settings) {
    let _ = PATH.set(path);
    configure(settings);
}
pub fn persist(settings: Settings) -> anyhow::Result<()> {
    let path = PATH.get().and_then(|p| p.as_deref());
    let mut cfg = crate::config::Config::load_auto(path)?;
    cfg.pet = settings.clone();
    if let Some(p) = path {
        cfg.save(p)?;
    } else {
        cfg.save_default()?;
    }
    configure(settings);
    Ok(())
}
pub fn edit(key: crossterm::event::KeyCode, selected: &mut usize, s: &mut Settings) -> bool {
    use crossterm::event::KeyCode;
    match key {
        KeyCode::Up | KeyCode::Left => *selected = (*selected + 19) % 20,
        KeyCode::Down | KeyCode::Right => *selected = (*selected + 1) % 20,
        KeyCode::Enter => {
            s.look = *selected;
            return true;
        }
        KeyCode::Char('h' | 'H') => {
            s.visible = !s.visible;
            return true;
        }
        KeyCode::Char('m' | 'M') => {
            s.reduced_motion = !s.reduced_motion;
            return true;
        }
        _ => {}
    }
    false
}
#[cfg(test)]
mod tests;
