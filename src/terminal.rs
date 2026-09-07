//! Lightweight terminal output and deterministic machine-readable events.
use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
static PLAIN: AtomicBool = AtomicBool::new(false);
static JSON: AtomicBool = AtomicBool::new(false);
static DASHBOARD: AtomicBool = AtomicBool::new(false);
static COLOR: AtomicBool = AtomicBool::new(true);
static SCREEN: std::sync::Mutex<()> = std::sync::Mutex::new(());
static EXIT: tokio::sync::Notify = tokio::sync::Notify::const_new();
pub fn dashboard_active() -> bool {
    DASHBOARD.load(Ordering::Relaxed)
}
pub async fn ctrl_c() {
    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = EXIT.notified() => {} }
}
pub fn configure(plain: bool, json: bool, no_color: bool) {
    COLOR.store(
        !no_color && std::env::var_os("NO_COLOR").is_none(),
        Ordering::Relaxed,
    );
    PLAIN.store(
        plain || json || !io::stdout().is_terminal(),
        Ordering::Relaxed,
    );
    JSON.store(json, Ordering::Relaxed);
    colored::control::set_override(
        !(plain
            || json
            || no_color
            || std::env::var_os("NO_COLOR").is_some()
            || !io::stdout().is_terminal()),
    );
}
pub fn interactive() -> bool {
    !PLAIN.load(Ordering::Relaxed) && io::stdin().is_terminal() && io::stdout().is_terminal()
}
pub fn can_prompt() -> bool {
    !json() && io::stdin().is_terminal() && io::stdout().is_terminal()
}
pub fn plain() -> bool {
    PLAIN.load(Ordering::Relaxed)
}
pub fn json() -> bool {
    JSON.load(Ordering::Relaxed)
}
pub fn output(text: String, newline: bool) {
    let text = crate::debuglog::redact(&text);
    if dashboard_active() {
        log::info!(target: "session", "{}", text.trim());
        return;
    }
    if json() {
        if text.trim().is_empty() {
            return;
        }
        let line = serde_json::json!({"event":"status","message":text});
        let _ = writeln!(io::stdout().lock(), "{line}");
    } else {
        let mut out = io::stdout().lock();
        if newline {
            let _ = writeln!(out, "{text}");
        } else {
            let _ = write!(out, "{text}");
            let _ = out.flush();
        }
    }
}
pub fn error(code: &str, text: &str) {
    let safe = crate::debuglog::redact(text);
    if json() {
        let _ = writeln!(
            io::stderr().lock(),
            "{}",
            serde_json::json!({"event":"error","code":code,"message":safe})
        );
    } else {
        let _ = writeln!(
            io::stderr().lock(),
            "  [{}] {}",
            code,
            crate::i18n::message(&safe)
        );
    }
}
pub fn banner() {
    if interactive() {
        output(
            format!(
                "frp-sh {}  ·  {}",
                crate::version::VERSION,
                crate::i18n::text("Ready", "就绪")
            ),
            true,
        );
    }
}
pub fn spinner(msg: impl Into<String>) -> indicatif::ProgressBar {
    let pb = if interactive() && !dashboard_active() {
        indicatif::ProgressBar::new_spinner()
    } else {
        indicatif::ProgressBar::hidden()
    };
    pb.set_style(
        indicatif::ProgressStyle::with_template("{spinner:.cyan} {msg}").expect("template"),
    );
    pb.set_message(crate::i18n::message(&msg.into()));
    if interactive() && !dashboard_active() {
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
    }
    pb
}

/// Restores the original terminal on success, errors and cancellation.
pub struct Monitor(Option<tokio::task::JoinHandle<()>>);
impl Drop for Monitor {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            DASHBOARD.store(false, Ordering::Relaxed);
            task.abort();
            let _guard = SCREEN.lock().unwrap_or_else(|e| e.into_inner());
            let _ = crossterm::execute!(
                io::stdout(),
                crossterm::style::ResetColor,
                crossterm::cursor::Show,
                crossterm::terminal::LeaveAlternateScreen
            );
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
}
pub fn monitor(session: bool) -> Monitor {
    if !interactive() || !session || dashboard_active() {
        return Monitor(None);
    }
    if crossterm::terminal::enable_raw_mode().is_err() {
        return Monitor(None);
    }
    if crossterm::execute!(
        io::stdout(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::cursor::Hide
    )
    .is_err()
    {
        let _ = crossterm::terminal::disable_raw_mode();
        return Monitor(None);
    }
    DASHBOARD.store(true, Ordering::Relaxed);
    let task = tokio::spawn(async move {
        use crossterm::{
            event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
            style::{Color, SetForegroundColor},
        };
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
        let mut page = 0usize;
        let mut tick = 0usize;
        let mut previous: Vec<crate::dashboard::Span> = Vec::new();
        let mut previous_size = (0, 0);
        loop {
            interval.tick().await;
            while event::poll(std::time::Duration::ZERO).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key.kind == KeyEventKind::Release {
                        continue;
                    }
                    match key.code {
                        KeyCode::Char('q' | 'Q') | KeyCode::Esc => EXIT.notify_one(),
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            EXIT.notify_one()
                        }
                        KeyCode::Right | KeyCode::Down | KeyCode::PageDown => {
                            page = page.saturating_add(1)
                        }
                        KeyCode::Left | KeyCode::Up | KeyCode::PageUp => {
                            page = page.saturating_sub(1)
                        }
                        KeyCode::Char('l' | 'L') => {
                            crate::i18n::choose(if crate::i18n::chinese() {
                                "en"
                            } else {
                                "zh-CN"
                            })
                        }
                        _ => {}
                    }
                }
            }
            let info = crate::stats::info_snapshot();
            let links = crate::stats::links_snapshot();
            let (w, h) = crossterm::terminal::size().unwrap_or((80, 24));
            let frame = crate::dashboard::render(
                w,
                h,
                &info,
                &crate::stats::room_view(),
                &links,
                page,
                tick / 5,
            );
            page %= frame.pages;
            tick = tick.wrapping_add(1);
            let full = previous_size != (w, h)
                || previous.len() != frame.spans.len()
                || previous
                    .iter()
                    .zip(&frame.spans)
                    .any(|(a, b)| (a.x, a.y) != (b.x, b.y));
            if !full && previous == frame.spans {
                continue;
            }
            let _guard = SCREEN.lock().unwrap_or_else(|e| e.into_inner());
            if !dashboard_active() {
                break;
            }
            let mut out = io::stdout().lock();
            let mut buffer = Vec::new();
            let background = if COLOR.load(Ordering::Relaxed) {
                Color::Rgb {
                    r: 14,
                    g: 20,
                    b: 28,
                }
            } else {
                Color::Reset
            };
            let _ = crossterm::queue!(
                buffer,
                crossterm::terminal::BeginSynchronizedUpdate,
                crossterm::style::SetBackgroundColor(background)
            );
            if full {
                let _ = crossterm::queue!(
                    buffer,
                    crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
                );
            }
            for (index, span) in frame.spans.iter().enumerate() {
                if !full && previous[index] == *span {
                    continue;
                }
                let mut text = span.text.clone();
                if !full {
                    use unicode_width::UnicodeWidthStr;
                    let trailing = previous[index].text.width().saturating_sub(text.width());
                    text.push_str(&" ".repeat(trailing));
                }
                let color = if COLOR.load(Ordering::Relaxed) {
                    match span.tone {
                        1 => Color::Rgb {
                            r: 126,
                            g: 224,
                            b: 192,
                        },
                        2 => Color::Rgb {
                            r: 115,
                            g: 132,
                            b: 148,
                        },
                        3 => Color::Rgb {
                            r: 235,
                            g: 188,
                            b: 113,
                        },
                        _ => Color::Rgb {
                            r: 226,
                            g: 234,
                            b: 242,
                        },
                    }
                } else {
                    Color::Reset
                };
                let _ = crossterm::queue!(
                    buffer,
                    crossterm::cursor::MoveTo(span.x, span.y),
                    SetForegroundColor(color),
                    crossterm::style::Print(text)
                );
            }
            let _ = crossterm::queue!(
                buffer,
                crossterm::style::ResetColor,
                crossterm::terminal::EndSynchronizedUpdate
            );
            if out.write_all(&buffer).and_then(|_| out.flush()).is_err() {
                EXIT.notify_one();
                break;
            }
            previous = frame.spans;
            previous_size = (w, h);
        }
    });
    Monitor(Some(task))
}
