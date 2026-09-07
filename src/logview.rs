//! Bounded, read-only diagnostic page shared by the app and live room.
use crate::{dashboard::Frame, i18n::text as tr};
use crossterm::event::{KeyCode, KeyEvent};
#[derive(Default)]
pub struct LogView {
    records: Vec<(String, u8)>,
    offset: usize,
    filter: usize,
    paused: bool,
}
impl LogView {
    pub fn key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                self.offset = self.offset.saturating_add(1);
                self.paused = true;
            }
            KeyCode::PageUp => {
                self.offset = self.offset.saturating_add(10);
                self.paused = true;
            }
            KeyCode::Down => self.offset = self.offset.saturating_sub(1),
            KeyCode::PageDown => self.offset = self.offset.saturating_sub(10),
            KeyCode::End => {
                self.offset = 0;
                self.paused = false;
            }
            KeyCode::Char(' ') => self.paused = !self.paused,
            KeyCode::Char('f' | 'F') => {
                self.filter = (self.filter + 1) % 3;
                self.offset = 0;
                self.refresh();
            }
            _ => {}
        }
    }
    pub fn refresh(&mut self) {
        use std::io::{Read, Seek, SeekFrom};
        let Some(path) = std::fs::read_dir(crate::debuglog::directory())
            .ok()
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                e.file_name().to_string_lossy().ends_with(".jsonl")
                    && e.metadata().is_ok_and(|m| m.len() > 0)
            })
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
            .map(|e| e.path())
        else {
            return;
        };
        let Ok(mut file) = std::fs::File::open(path) else {
            return;
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let start = len.saturating_sub(128 * 1024);
        if file.seek(SeekFrom::Start(start)).is_err() {
            return;
        }
        let mut bytes = Vec::new();
        if file.take(128 * 1024).read_to_end(&mut bytes).is_err() {
            return;
        }
        let text = String::from_utf8_lossy(&bytes);
        self.records = text
            .lines()
            .skip(usize::from(start > 0))
            .filter_map(|line| {
                let v: serde_json::Value = serde_json::from_str(line).ok()?;
                let level = v["level"].as_str()?;
                if self.filter == 1 && !matches!(level, "WARN" | "ERROR")
                    || self.filter == 2 && level != "ERROR"
                {
                    return None;
                }
                let ts = v["ts"].as_u64().unwrap_or(0) % 86400;
                Some((
                    format!(
                        "{:02}:{:02}:{:02}Z {:5} {}",
                        ts / 3600,
                        ts / 60 % 60,
                        ts % 60,
                        level,
                        crate::debuglog::redact(v["message"].as_str().unwrap_or(""))
                    ),
                    if matches!(level, "WARN" | "ERROR") {
                        3
                    } else {
                        2
                    },
                ))
            })
            .collect();
    }
    pub fn frame(&mut self, w: u16, h: u16, tick: usize) -> Frame {
        if !self.paused && tick.is_multiple_of(10) {
            self.refresh();
        }
        let width = w.saturating_sub(8).max(1) as usize;
        let mut lines = Vec::new();
        for (text, tone) in &self.records {
            let mut line = String::new();
            let mut used = 0;
            for c in text.chars().filter(|c| !c.is_control()) {
                let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
                if used + cw > width {
                    lines.push((line.clone(), *tone));
                    line.clear();
                    used = 0;
                }
                line.push(c);
                used += cw;
            }
            if !line.is_empty() {
                lines.push((line, *tone));
            }
        }
        let count = h.saturating_sub(9) as usize;
        self.offset = self.offset.min(lines.len().saturating_sub(count));
        let end = lines.len().saturating_sub(self.offset);
        let start = end.saturating_sub(count);
        let title = format!(
            "{} / {} / {}",
            tr("Logs", "日志"),
            ["ALL", "WARN+ERROR", "ERROR"][self.filter],
            if self.paused {
                tr("Paused", "已暂停")
            } else {
                tr("Live", "实时")
            }
        );
        if lines.is_empty() {
            lines.push((
                tr("No matching log records", "暂无符合条件的日志").into(),
                2,
            ));
            return crate::app::shell(
                w,
                h,
                &title,
                tr(
                    "F Filter   Space Pause   Esc Back",
                    "F 筛选   空格 暂停   Esc 返回",
                ),
                &lines,
            );
        }
        crate::app::shell(
            w,
            h,
            &title,
            tr(
                "↑ ↓ Scroll   F Filter   Space Pause   End Live   Esc Back",
                "↑ ↓ 滚动   F 筛选   空格 暂停   End 实时   Esc 返回",
            ),
            &lines[start..end],
        )
    }
}
