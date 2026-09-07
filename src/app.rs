//! Keyboard application shell. Session subprocesses own their network lifecycle.
use crate::{
    config::{Config, Profile},
    dashboard::{clean, Frame, Span},
    i18n::text as tr,
};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Profiles,
    Join,
    Settings,
    Help,
    Logs,
}
impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Home => tr("Home", "首页"),
            Self::Profiles => tr("Profiles", "连接配置"),
            Self::Join => tr("Join", "加入房间"),
            Self::Settings => tr("Settings", "设置"),
            Self::Help => tr("Help", "帮助"),
            Self::Logs => tr("Logs", "日志"),
        }
    }
    fn next(self) -> Self {
        match self {
            Self::Home => Self::Profiles,
            Self::Profiles => Self::Join,
            Self::Join => Self::Settings,
            Self::Settings => Self::Help,
            Self::Help => Self::Logs,
            Self::Logs => Self::Home,
        }
    }
}
#[derive(Clone)]
struct Field {
    label: &'static str,
    value: String,
    secret: bool,
}
struct Form {
    fields: Vec<Field>,
    focus: usize,
    original: Option<String>,
    settings: bool,
    launch: Option<String>,
}
struct State {
    cfg: Config,
    path: Option<PathBuf>,
    page: Page,
    selected: usize,
    form: Option<Form>,
    notice: String,
    deleting: bool,
    join: String,
    logs: crate::logview::LogView,
}
enum Action {
    Quit,
    Session(Vec<String>),
}

pub(crate) fn shell(
    width: u16,
    height: u16,
    title: &str,
    hint: &str,
    lines: &[(String, u8)],
) -> Frame {
    let mut spans = Vec::new();
    let x = if width > 100 {
        (width - 96) / 2
    } else {
        2.min(width.saturating_sub(1))
    };
    let available = width.saturating_sub(x + 2) as usize;
    let mut put = |y: u16, text: &str, tone| {
        if y < height {
            spans.push(Span {
                x: x + if y >= 5 && y < height.saturating_sub(3) {
                    2
                } else {
                    0
                },
                y,
                text: clean(
                    text,
                    if y >= 5 && y < height.saturating_sub(3) {
                        available.saturating_sub(4)
                    } else {
                        available
                    },
                ),
                tone,
            });
        }
    };
    put(1, &format!("frp.sh   /   {title}"), 1);
    put(
        3,
        &format!("╭{}╮", "─".repeat(available.saturating_sub(2))),
        2,
    );
    for (i, (line, tone)) in lines
        .iter()
        .take(height.saturating_sub(9) as usize)
        .enumerate()
    {
        put(5 + i as u16, line, *tone);
    }
    put(
        height.saturating_sub(3),
        &format!("╰{}╯", "─".repeat(available.saturating_sub(2))),
        2,
    );
    put(height.saturating_sub(2), hint, 2);
    if available >= 4 {
        for y in 4..height.saturating_sub(3) {
            spans.push(Span {
                x,
                y,
                text: "│".into(),
                tone: 2,
            });
            spans.push(Span {
                x: x + available as u16 - 1,
                y,
                text: "│".into(),
                tone: 2,
            });
        }
    }
    Frame { spans, pages: 1 }
}
pub fn invite_frame(w: u16, h: u16, selected: usize, notice: &str) -> Frame {
    let options = [
        tr("Copy invitation link", "复制邀请链接"),
        tr(
            "Copy Windows install + join command",
            "复制 Windows 安装并加入命令",
        ),
        tr(
            "Copy macOS / Linux install + join command",
            "复制 macOS / Linux 安装并加入命令",
        ),
    ];
    let mut lines = vec![
        (
            tr("Invite friends to this room", "邀请朋友加入当前房间").into(),
            0,
        ),
        (String::new(), 0),
    ];
    for (i, label) in options.iter().enumerate() {
        lines.push((
            format!("{}  {}", if i == selected { "›" } else { " " }, label),
            if i == selected { 1 } else { 0 },
        ));
        lines.push((String::new(), 0));
    }
    lines.push((
        tr(
            "Contains room access credentials. Valid until revoked or expired.",
            "邀请包含房间访问凭据，到期或撤销后失效。",
        )
        .into(),
        3,
    ));
    lines.push((
        tr(
            "Share privately with people you want to invite.",
            "请私下发给你想邀请的人。",
        )
        .into(),
        2,
    ));
    lines.push((notice.into(), 1));
    shell(
        w,
        h,
        tr("Invitation", "邀请"),
        tr(
            "↑ ↓ Select   Enter Copy   Esc Back",
            "↑ ↓ 选择   Enter 复制   Esc 返回",
        ),
        &lines,
    )
}
impl State {
    fn save(&self) -> anyhow::Result<()> {
        if let Some(path) = &self.path {
            self.cfg.save(path)?;
        } else {
            self.cfg.save_default()?;
        }
        Ok(())
    }
    fn selected_profile(&self) -> Option<Profile> {
        self.cfg.profiles.values().nth(self.selected).cloned()
    }
    fn open_form(&mut self, profile: Option<Profile>, settings: bool) {
        let make = |label, value, secret| Field {
            label,
            value,
            secret,
        };
        let fields = if settings {
            vec![
                make(
                    tr("Device name", "设备名称"),
                    self.cfg.name.clone().unwrap_or_default(),
                    false,
                ),
                make(
                    tr("Server URL", "服务器地址"),
                    self.cfg.signaling_addr.clone(),
                    false,
                ),
                make(
                    tr("Server password", "服务器密码"),
                    self.cfg.password.clone().unwrap_or_default(),
                    true,
                ),
                make(
                    tr("Relay address", "中继地址"),
                    self.cfg.relay_addr.clone(),
                    false,
                ),
            ]
        } else {
            let p = profile.clone().unwrap_or_else(|| Profile {
                name: self.cfg.next_profile_name(),
                server: self.cfg.signaling_addr.clone(),
                room: String::new(),
                password: self.cfg.password.clone(),
                key: None,
                mode: "lan".into(),
                device_name: None,
                relay_addr: Some(self.cfg.relay_addr.clone()),
                listen: None,
                expose_lan: false,
                default: false,
            });
            vec![
                make(tr("Profile name", "配置名称"), p.name, false),
                make(tr("Server URL", "服务器地址"), p.server, false),
                make(tr("Room", "房间号"), p.room, false),
                make(
                    tr("Server password", "服务器密码"),
                    p.password.unwrap_or_default(),
                    true,
                ),
                make(tr("Room key", "房间密钥"), p.key.unwrap_or_default(), true),
                make(
                    tr("Mode: lan / dev / game", "模式：lan / dev / game"),
                    p.mode,
                    false,
                ),
                make(
                    tr("Device name", "设备名称"),
                    p.device_name.unwrap_or_default(),
                    false,
                ),
                make(
                    tr("Relay address", "中继地址"),
                    p.relay_addr.unwrap_or_default(),
                    false,
                ),
                make(
                    tr("Listen address", "监听地址"),
                    p.listen.unwrap_or_default(),
                    false,
                ),
                make(
                    tr("Share LAN: true / false", "共享局域网：true / false"),
                    p.expose_lan.to_string(),
                    false,
                ),
            ]
        };
        self.form = Some(Form {
            fields,
            focus: 0,
            original: profile.map(|p| p.name),
            settings,
            launch: None,
        });
    }
    fn service_form(&mut self, kind: &str) {
        self.form = Some(Form {
            fields: vec![
                Field {
                    label: tr("Service name", "服务名称"),
                    value: String::new(),
                    secret: false,
                },
                Field {
                    label: tr(
                        "TCP ports / origins (space separated)",
                        "TCP 端口或 URL（空格分隔）",
                    ),
                    value: String::new(),
                    secret: false,
                },
                Field {
                    label: tr("UDP ports (space separated)", "UDP 端口（空格分隔）"),
                    value: String::new(),
                    secret: false,
                },
            ],
            focus: 1,
            original: None,
            settings: false,
            launch: Some(kind.into()),
        });
    }
    fn save_form(&mut self) -> anyhow::Result<()> {
        let form = self.form.as_ref().unwrap();
        let v = |i: usize| form.fields[i].value.trim().to_string();
        let secret = |i: usize| {
            let s = &form.fields[i].value;
            if s.is_empty() {
                None
            } else {
                Some(s.clone())
            }
        };
        let server = v(1);
        let url = reqwest::Url::parse(&server)?;
        anyhow::ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none(),
            "Invalid server URL"
        );
        let mut cfg = self.cfg.clone();
        if form.settings {
            cfg.name = Some(v(0));
            cfg.signaling_addr = server;
            cfg.password = secret(2);
            cfg.relay_addr = if v(3).is_empty() {
                Config::derive_relay(&cfg.signaling_addr).unwrap_or_default()
            } else {
                v(3)
            };
        } else {
            let name = v(0);
            anyhow::ensure!(!name.is_empty(), "Profile name is required");
            anyhow::ensure!(
                form.original.as_deref() == Some(&name) || !cfg.profiles.contains_key(&name),
                "Profile name already exists"
            );
            anyhow::ensure!(
                matches!(v(5).as_str(), "lan" | "dev" | "game"),
                "Mode must be lan, dev or game"
            );
            anyhow::ensure!(
                matches!(v(9).as_str(), "true" | "false"),
                "LAN sharing must be true or false"
            );
            let default = form
                .original
                .as_ref()
                .and_then(|n| cfg.profiles.get(n))
                .is_some_and(|p| p.default);
            if let Some(old) = &form.original {
                cfg.profiles.remove(old);
            }
            cfg.profiles.insert(
                name.clone(),
                Profile {
                    name,
                    server: server.clone(),
                    room: v(2),
                    password: secret(3),
                    key: secret(4),
                    mode: v(5),
                    device_name: (!v(6).is_empty()).then(|| v(6)),
                    relay_addr: Some(if v(7).is_empty() {
                        Config::derive_relay(&server).unwrap_or_default()
                    } else {
                        v(7)
                    }),
                    listen: (!v(8).is_empty()).then(|| v(8)),
                    expose_lan: v(9) == "true",
                    default,
                },
            );
        }
        if let Some(path) = &self.path {
            cfg.save(path)?;
        } else {
            cfg.save_default()?;
        }
        self.cfg = cfg;
        self.form = None;
        self.notice = tr("Saved", "已保存").into();
        Ok(())
    }
    fn event(&mut self, event: Event) -> anyhow::Result<Option<Action>> {
        if let Event::Paste(text) = &event {
            let text: String = text
                .chars()
                .filter(|c| !c.is_control())
                .take(12000)
                .collect();
            if let Some(form) = &mut self.form {
                let f = &mut form.fields[form.focus];
                if f.value.len() + text.len() <= 12000 {
                    f.value.push_str(&text);
                }
            } else if self.page == Page::Join && self.join.len() + text.len() <= 12000 {
                self.join.push_str(&text);
            }
            return Ok(None);
        }
        let Event::Key(key) = event else {
            return Ok(None);
        };
        if key.kind == KeyEventKind::Release {
            return Ok(None);
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(Some(Action::Quit));
        }
        if self.form.is_some() {
            if key.code == KeyCode::Esc {
                self.form = None;
                return Ok(None);
            }
            if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
                if let Some(kind) = self.form.as_ref().and_then(|f| f.launch.clone()) {
                    let f = self.form.as_ref().unwrap();
                    let tcp: Vec<_> = f.fields[1]
                        .value
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect();
                    let udp: Vec<_> = f.fields[2]
                        .value
                        .split_whitespace()
                        .map(str::to_owned)
                        .collect();
                    crate::services::published(&tcp, &udp, None)?;
                    let mut args = vec![kind.clone(), "create".into()];
                    if kind == "game" {
                        args.extend(["--kind".into(), "server".into()]);
                    }
                    if !f.fields[0].value.trim().is_empty() {
                        args.extend(["--label".into(), f.fields[0].value.trim().into()]);
                    }
                    for port in tcp {
                        args.extend(["--tcp".into(), port]);
                    }
                    for port in udp {
                        args.extend(["--udp".into(), port]);
                    }
                    self.form = None;
                    return Ok(Some(Action::Session(args)));
                }
                self.save_form()?;
                return Ok(None);
            }
            let f = self.form.as_mut().unwrap();
            let field = &mut f.fields[f.focus];
            match key.code {
                KeyCode::Tab | KeyCode::Down | KeyCode::Enter => {
                    f.focus = (f.focus + 1) % f.fields.len()
                }
                KeyCode::BackTab | KeyCode::Up => {
                    f.focus = (f.focus + f.fields.len() - 1) % f.fields.len()
                }
                KeyCode::Backspace => {
                    field.value.pop();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    field.value.clear()
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && field.value.len() < 12000 =>
                {
                    field.value.push(c)
                }
                _ => {}
            }
            return Ok(None);
        }
        if self.deleting {
            if key.code == KeyCode::Enter {
                if let Some(p) = self.selected_profile() {
                    self.cfg.profiles.remove(&p.name);
                    self.save()?;
                    self.selected = self.selected.saturating_sub(1);
                }
                self.deleting = false;
            } else if key.code == KeyCode::Esc {
                self.deleting = false;
            }
            return Ok(None);
        }
        if key.code == KeyCode::Esc {
            self.page = Page::Home;
            self.notice.clear();
            return Ok(None);
        }
        if key.code == KeyCode::Tab {
            self.page = self.page.next();
            self.selected = 0;
            self.notice.clear();
            return Ok(None);
        }
        if self.page == Page::Logs {
            self.logs.key(key);
            return Ok(None);
        }
        if self.page == Page::Join {
            match key.code {
                KeyCode::Enter => {
                    let input = self.join.trim();
                    if input.starts_with("https://") {
                        let invitation = crate::invite::Invitation::parse(input)?;
                        let name = invitation.save(&mut self.cfg);
                        self.save()?;
                        self.join.clear();
                        return Ok(Some(Action::Session(vec![
                            "profile".into(),
                            "run".into(),
                            name,
                        ])));
                    }
                    anyhow::ensure!(
                        !input.is_empty()
                            && input.len() <= 128
                            && input
                                .bytes()
                                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
                        "Enter a room ID or invitation link"
                    );
                    return Ok(Some(Action::Session(
                        vec!["join".into(), input.into()]
                            .into_iter()
                            .chain(
                                key.modifiers
                                    .contains(KeyModifiers::CONTROL)
                                    .then(|| "--network".into()),
                            )
                            .collect(),
                    )));
                }
                KeyCode::Backspace => {
                    self.join.pop();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.join.clear()
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                        && self.join.len() < 12000 =>
                {
                    self.join.push(c)
                }
                _ => {}
            }
            return Ok(None);
        }
        match key.code {
            KeyCode::Char('q' | 'Q') => return Ok(Some(Action::Quit)),
            KeyCode::Char('l' | 'L') => {
                crate::i18n::choose(if crate::i18n::chinese() {
                    "en"
                } else {
                    "zh-CN"
                });
                self.cfg.language = Some(
                    if crate::i18n::chinese() {
                        "zh-CN"
                    } else {
                        "en"
                    }
                    .into(),
                );
                self.save()?;
            }
            KeyCode::Char('1') => {
                self.page = Page::Home;
                self.selected = 0;
            }
            KeyCode::Char('2') => {
                self.page = Page::Profiles;
                self.selected = 0;
            }
            KeyCode::Char('3') => self.page = Page::Join,
            KeyCode::Char('4') => self.page = Page::Settings,
            KeyCode::Char('5') => self.page = Page::Help,
            KeyCode::Char('6' | 'g' | 'G') => {
                self.page = Page::Logs;
                self.logs.refresh();
            }
            KeyCode::Up | KeyCode::Left => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Right => {
                self.selected = (self.selected + 1).min(if self.page == Page::Profiles {
                    self.cfg.profiles.len().saturating_sub(1)
                } else {
                    if self.page == Page::Home {
                        5
                    } else {
                        3
                    }
                })
            }
            KeyCode::Char('n' | 'N') if self.page == Page::Profiles => self.open_form(None, false),
            KeyCode::Char('e' | 'E') if self.page == Page::Profiles => {
                if let Some(p) = self.selected_profile() {
                    self.open_form(Some(p), false);
                }
            }
            KeyCode::Char('d' | 'D') if self.page == Page::Profiles => {
                if let Some(p) = self.selected_profile() {
                    self.cfg.mark_default_profile(&p.name);
                    self.save()?;
                    self.notice = tr("Default profile updated", "已设置默认配置").into();
                }
            }
            KeyCode::Delete if self.page == Page::Profiles => {
                self.deleting = self.selected_profile().is_some()
            }
            KeyCode::Enter => match self.page {
                Page::Home => match self.selected {
                    0 => return Ok(Some(Action::Session(vec!["game".into(), "create".into()]))),
                    1 => self.service_form("game"),
                    2 => self.service_form("dev"),
                    3 => self.page = Page::Join,
                    4 => {
                        self.page = Page::Profiles;
                        self.selected = 0;
                    }
                    _ => self.page = Page::Settings,
                },
                Page::Profiles => {
                    if let Some(p) = self.selected_profile() {
                        anyhow::ensure!(!p.room.is_empty(), "Set a room with E before joining");
                        return Ok(Some(Action::Session(vec![
                            "profile".into(),
                            "run".into(),
                            p.name,
                        ])));
                    }
                }
                Page::Settings => self.open_form(None, true),
                _ => {}
            },
            _ => {}
        }
        Ok(None)
    }
    fn frame(&mut self, w: u16, h: u16, tick: usize) -> Frame {
        if self.page == Page::Logs {
            return self.logs.frame(w, h, tick);
        }
        let mut lines = Vec::new();
        let mut hint = tr(
            "Tab Pages   Enter Open   G Logs   L Language   Q Quit",
            "Tab 切页   Enter 打开   G 日志   L 语言   Q 退出",
        );
        let title = if self.form.is_some() {
            tr("Edit configuration", "编辑配置")
        } else {
            self.page.title()
        };
        if let Some(form) = &self.form {
            let per = (h.saturating_sub(12) as usize / 2).max(1);
            let start = form.focus / per * per;
            for (i, f) in form.fields.iter().enumerate().skip(start).take(per) {
                lines.push((
                    format!("{} {}", if i == form.focus { "›" } else { " " }, f.label),
                    if i == form.focus { 1 } else { 2 },
                ));
                let value = if f.secret {
                    "•".repeat(f.value.chars().count().min(24))
                } else {
                    f.value.clone()
                };
                let max = w.saturating_sub(10) as usize;
                let tail: String = value
                    .chars()
                    .rev()
                    .take(max / 2)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                lines.push((
                    format!("  {tail}{}", if i == form.focus { "▏" } else { "" }),
                    0,
                ));
            }
            hint = tr(
                "Tab / ↑ ↓ Field   Ctrl+U Clear   Ctrl+S Save   Esc Cancel",
                "Tab / ↑ ↓ 字段   Ctrl+U 清空   Ctrl+S 保存   Esc 取消",
            );
        } else {
            match self.page {
                Page::Home => {
                    lines.push((
                        tr(
                            "Your devices. One shared space.",
                            "你的设备，同一个共享空间。",
                        )
                        .into(),
                        0,
                    ));
                    lines.push((String::new(), 0));
                    for (i, label) in [
                        tr("Game / LAN multiplayer", "游戏 / 局域网联机"),
                        tr(
                            "Game server · explicit TCP / UDP services",
                            "游戏服务器 · 指定 TCP / UDP 服务",
                        ),
                        tr(
                            "Dev · share Web, API or SSH",
                            "开发协作 · 共享 Web、API 或 SSH",
                        ),
                        tr("Join with a link or room ID", "使用链接或房间号加入"),
                        tr("Saved connections", "管理连接配置"),
                        tr("Device & server settings", "设备与服务器设置"),
                    ]
                    .iter()
                    .enumerate()
                    {
                        lines.push((
                            format!("{}  {label}", if i == self.selected { "›" } else { " " }),
                            if i == self.selected { 1 } else { 0 },
                        ));
                        lines.push((String::new(), 0));
                    }
                }
                Page::Profiles => {
                    if self.cfg.profiles.is_empty() {
                        lines.push((
                            tr(
                                "No saved connections. Press N to create one.",
                                "还没有连接配置，按 N 新建。",
                            )
                            .into(),
                            0,
                        ));
                    }
                    let per = (h.saturating_sub(13) as usize / 3).max(1);
                    let start = self.selected / per * per;
                    for (i, p) in self.cfg.profiles.values().enumerate().skip(start).take(per) {
                        lines.push((
                            format!(
                                "{} {}{}",
                                if i == self.selected { "›" } else { " " },
                                p.name,
                                if p.default {
                                    tr("  · default", "  · 默认")
                                } else {
                                    ""
                                }
                            ),
                            if i == self.selected { 1 } else { 0 },
                        ));
                        lines.push((
                            format!(
                                "  {}  ·  {} {}  ·  {}",
                                p.mode,
                                tr("Room", "房间"),
                                p.room,
                                p.server
                            ),
                            2,
                        ));
                        lines.push((String::new(), 0));
                    }
                    hint = tr(
                        "Enter Join   N New   E Edit   D Default   Del Remove   Tab Pages",
                        "Enter 连接   N 新建   E 编辑   D 默认   Del 删除   Tab 切页",
                    );
                }
                Page::Join => {
                    lines.push((
                        tr(
                            "Paste an invitation link or enter a room ID",
                            "粘贴邀请链接，或输入房间号",
                        )
                        .into(),
                        0,
                    ));
                    lines.push((String::new(), 0));
                    let value = if self.join.starts_with("https://") {
                        format!(
                            "{} ({} bytes)",
                            tr("Invitation entered", "已输入邀请"),
                            self.join.len()
                        )
                    } else {
                        self.join.clone()
                    };
                    lines.push((format!("› {value}▏"), 1));
                    lines.push((String::new(), 0));
                    lines.push((
                        tr(
                            "Invitations save a profile. Ctrl+Enter explicitly permits device networking.",
                            "邀请会保存配置。Ctrl+Enter 明确允许整机网络访问。",
                        )
                        .into(),
                        2,
                    ));
                    hint = tr(
                        "Enter Services   Ctrl+Enter Allow device network   Tab Pages",
                        "Enter 加入服务   Ctrl+Enter 允许设备组网   Tab 切页",
                    );
                }
                Page::Settings => {
                    lines.push((
                        format!(
                            "{}  {}",
                            tr("Device", "设备"),
                            crate::config::device_name(self.cfg.name.as_deref())
                        ),
                        0,
                    ));
                    lines.push((
                        format!("{}  {}", tr("Server", "服务器"), self.cfg.signaling_addr),
                        2,
                    ));
                    lines.push((
                        format!(
                            "{}  {}",
                            tr("Password", "密码"),
                            if self.cfg.password.is_some() {
                                "••••••"
                            } else {
                                "—"
                            }
                        ),
                        2,
                    ));
                    lines.push((String::new(), 0));
                    lines.push((
                        tr("Enter  Edit device and server", "Enter  编辑设备与服务器").into(),
                        1,
                    ));
                    lines.push((tr("L      Switch language", "L      切换中英文").into(), 1));
                    lines.push((String::new(), 0));
                    lines.push((
                        tr("LAN sharing is off by default.", "默认不共享本地局域网。").into(),
                        2,
                    ));
                }
                Page::Logs => unreachable!(),
                Page::Help => {
                    for text in [
                        tr("Tab / 1–6    Navigate pages", "Tab / 1–6    切换页面"),
                        tr(
                            "Esc          Back to home / cancel editing",
                            "Esc          返回首页 / 取消编辑",
                        ),
                        tr(
                            "I            Invite friends inside a room",
                            "I            在房间中打开邀请",
                        ),
                        tr(
                            "Q            Leave room and return to app",
                            "Q            退出房间并返回应用",
                        ),
                        tr(
                            "frp-sh logs tail    Read separate diagnostic logs",
                            "frp-sh logs tail    查看独立诊断日志",
                        ),
                        tr(
                            "--plain / --json    Keep script-friendly output",
                            "--plain / --json    保留脚本友好的输出",
                        ),
                    ] {
                        lines.push((text.into(), 0));
                        lines.push((String::new(), 0));
                    }
                }
            }
        }
        if self.deleting {
            lines.clear();
            lines.push((
                format!(
                    "{} {}?",
                    tr("Remove", "删除"),
                    self.selected_profile().map(|p| p.name).unwrap_or_default()
                ),
                3,
            ));
            hint = tr(
                "Enter Confirm removal   Esc Cancel",
                "Enter 确认删除   Esc 取消",
            );
        }
        let mut frame = shell(w, h, title, hint, &lines);
        if !self.notice.is_empty() && h > 4 {
            frame.spans.push(Span {
                x: 4,
                y: h - 4,
                text: clean(&self.notice, w.saturating_sub(8) as usize),
                tone: 3,
            });
        }
        if h > 3 {
            frame.spans.push(Span {
                x: 2,
                y: 2,
                text: clean(
                    tr(
                        "1 Home   2 Profiles   3 Join   4 Settings   5 Help   6 Logs",
                        "1 首页   2 配置   3 加入   4 设置   5 帮助   6 日志",
                    ),
                    w.saturating_sub(4) as usize,
                ),
                tone: 2,
            });
        }
        frame
    }
}

pub async fn run(path: Option<PathBuf>, page: Page) -> anyhow::Result<()> {
    let cfg = Config::load_auto(path.as_deref())?;
    let state = Arc::new(Mutex::new(State {
        cfg,
        path: path.clone(),
        page,
        selected: 0,
        form: None,
        notice: String::new(),
        deleting: false,
        join: String::new(),
        logs: Default::default(),
    }));
    loop {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let shared = state.clone();
        let screen = crate::terminal::monitor_with(move |w, h, events, tick| {
            let mut state = shared.lock().unwrap();
            for event in events {
                match state.event(event) {
                    Ok(Some(action)) => {
                        let _ = sender.send(action);
                        break;
                    }
                    Err(e) => state.notice = crate::debuglog::redact(&e.to_string()),
                    _ => {}
                }
            }
            state.frame(w, h, tick)
        });
        anyhow::ensure!(
            crate::terminal::dashboard_active(),
            "Cannot open interactive terminal"
        );
        let action = tokio::select! {value=receiver.recv()=>value,_=crate::terminal::ctrl_c()=>Some(Action::Quit)};
        drop(screen);
        let Some(Action::Session(args)) = action else {
            return Ok(());
        };
        let executable = std::env::current_exe()?;
        let config = path.clone();
        let language = if crate::i18n::chinese() {
            "zh-CN"
        } else {
            "en"
        };
        let result = tokio::task::spawn_blocking(move || {
            let mut child = std::process::Command::new(executable);
            child.args(["--lang", language]);
            if let Some(path) = config {
                child.arg("--config").arg(path);
            }
            child
                .args(args)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::piped())
                .output()
        })
        .await?;
        let mut state = state.lock().unwrap();
        state.cfg = Config::load_auto(path.as_deref())?;
        state.page = Page::Home;
        state.selected = 0;
        state.notice = match result {
            Ok(output) if output.status.success() => tr("Session ended", "已退出房间").into(),
            Ok(output) => crate::debuglog::redact(String::from_utf8_lossy(&output.stderr).trim()),
            Err(e) => e.to_string(),
        };
    }
}
#[cfg(test)]
mod navigation_tests {
    use super::*;
    fn state() -> State {
        State {
            cfg: Config::default(),
            path: None,
            page: Page::Home,
            selected: 0,
            form: None,
            notice: String::new(),
            deleting: false,
            join: String::new(),
            logs: Default::default(),
        }
    }
    fn key(code: KeyCode) -> Event {
        Event::Key(crossterm::event::KeyEvent::new(code, KeyModifiers::NONE))
    }
    #[test]
    fn pages_and_forms_handle_navigation_without_triggering_shortcuts() {
        let mut s = state();
        s.event(key(KeyCode::Char('2'))).unwrap();
        assert!(s.page == Page::Profiles);
        s.event(key(KeyCode::Char('n'))).unwrap();
        assert!(s.form.is_some());
        s.event(Event::Paste("my-profile".into())).unwrap();
        s.event(key(KeyCode::Char('q'))).unwrap();
        assert!(s.form.is_some());
        s.event(key(KeyCode::Esc)).unwrap();
        assert!(s.form.is_none());
        s.event(key(KeyCode::Char('3'))).unwrap();
        s.event(Event::Paste("1234".into())).unwrap();
        assert!(
            matches!(s.event(key(KeyCode::Enter)).unwrap(),Some(Action::Session(args)) if args==["join","1234"])
        );
        s.event(key(KeyCode::Esc)).unwrap();
        assert!(s.page == Page::Home);
        s.event(key(KeyCode::Char('g'))).unwrap();
        assert!(s.page == Page::Logs);
    }
    #[test]
    fn every_page_is_bounded_and_passwords_are_never_rendered() {
        let mut s = state();
        s.cfg.password = Some("confidential-test-password".into());
        for page in [
            Page::Home,
            Page::Profiles,
            Page::Join,
            Page::Settings,
            Page::Help,
            Page::Logs,
        ] {
            s.page = page;
            for (w, h) in [(20, 8), (40, 18), (80, 24), (120, 40)] {
                let f = s.frame(w, h, 1);
                for span in f.spans {
                    assert!(span.x < w && span.y < h);
                    assert!(!span.text.contains("confidential-test-password"));
                    assert!(
                        unicode_width::UnicodeWidthStr::width(span.text.as_str())
                            <= usize::from(w - span.x)
                    );
                }
            }
        }
        s.open_form(None, true);
        let f = s.frame(100, 40, 1);
        assert!(f
            .spans
            .iter()
            .all(|s| !s.text.contains("confidential-test-password")));
    }
}
