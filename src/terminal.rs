//! Lightweight terminal output and deterministic machine-readable events.
use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
static PLAIN: AtomicBool = AtomicBool::new(false);
static JSON: AtomicBool = AtomicBool::new(false);
pub fn configure(plain: bool, json: bool, no_color: bool) {
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
pub fn plain() -> bool {
    PLAIN.load(Ordering::Relaxed)
}
pub fn json() -> bool {
    JSON.load(Ordering::Relaxed)
}
pub fn output(text: String, newline: bool) {
    let text = crate::debuglog::redact(&text);
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
    let pb = if interactive() {
        indicatif::ProgressBar::new_spinner()
    } else {
        indicatif::ProgressBar::hidden()
    };
    pb.set_style(
        indicatif::ProgressStyle::with_template("{spinner:.cyan} {msg}").expect("template"),
    );
    pb.set_message(crate::i18n::message(&msg.into()));
    if interactive() {
        pb.enable_steady_tick(std::time::Duration::from_millis(100));
    }
    pb
}

/// Owns the single live status line; dropping it cancels rendering and clears it.
pub struct Monitor(Option<tokio::task::JoinHandle<()>>, indicatif::ProgressBar);
impl Drop for Monitor {
    fn drop(&mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
        }
        self.1.finish_and_clear();
    }
}
pub fn monitor() -> Monitor {
    let pb = indicatif::ProgressBar::hidden();
    if !interactive() {
        return Monitor(None, pb);
    }
    let pb = indicatif::ProgressBar::new_spinner();
    pb.set_style(indicatif::ProgressStyle::with_template("{msg}").expect("status template"));
    let view = pb.clone();
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let info = crate::stats::info_snapshot();
            let links = crate::stats::links_snapshot();
            if links.is_empty() {
                view.set_message("");
                continue;
            }
            let sent: u64 = links.iter().map(|r| r.4).sum();
            let received: u64 = links.iter().map(|r| r.5).sum();
            let latency = links.iter().map(|r| r.7).max().unwrap_or(0) as f64 / 1000.0;
            view.set_message(crate::debuglog::redact(&format!(
                "{} | {} | {} | RTT {:.1} ms | TX {:.1} KiB RX {:.1} KiB",
                info.room,
                info.device_name,
                links[0].1,
                latency,
                sent as f64 / 1024.0,
                received as f64 / 1024.0
            )));
        }
    });
    Monitor(Some(task), pb)
}
