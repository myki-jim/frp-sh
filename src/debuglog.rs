//! 进程内日志后端：**终端静默**，日志写入本地文件并保留在内存环形缓冲中，
//! 供 Web 面板 `/api/debug` 实时查看。
//!
//! - 文件：`<配置目录>/logs/frp-sh.log`（追加；超过 5MB 轮转为 `.old`）
//! - 环形缓冲：最近 [`RING_CAP`] 条（带全局递增序号，面板增量拉取）
//! - `RUST_LOG` 过滤规则仍然生效（默认 `-v` 为 debug，否则 info）

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// 内存环形缓冲容量（条）。
const RING_CAP: usize = 1000;
/// 单个日志文件上限（字节），超过后轮转为 `.old`。
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// 面板展示的一条日志。
#[derive(Clone, serde::Serialize)]
pub struct LogLine {
    /// 全局递增序号（面板增量拉取的游标）
    pub seq: u64,
    /// unix 秒
    pub ts: u64,
    /// INFO / WARN / ERROR / DEBUG / TRACE
    pub level: String,
    /// 日志消息（已含 target 前缀）
    pub msg: String,
}

struct Inner {
    ring: VecDeque<LogLine>,
    seq: u64,
    file: Option<std::fs::File>,
    file_path: Option<PathBuf>,
    written: u64,
}

static INNER: std::sync::OnceLock<Mutex<Inner>> = std::sync::OnceLock::new();

fn inner() -> &'static Mutex<Inner> {
    INNER.get_or_init(|| {
        Mutex::new(Inner {
            ring: VecDeque::with_capacity(RING_CAP),
            seq: 0,
            file: None,
            file_path: None,
            written: 0,
        })
    })
}

/// 日志文件路径（`<配置目录>/logs/frp-sh.log`；无法定位配置目录时为 None）。
pub fn file_path() -> Option<PathBuf> {
    inner().lock().unwrap().file_path.clone()
}

fn level_str(level: log::Level) -> &'static str {
    match level {
        log::Level::Error => "ERROR",
        log::Level::Warn => "WARN",
        log::Level::Info => "INFO",
        log::Level::Debug => "DEBUG",
        log::Level::Trace => "TRACE",
    }
}

fn stamp(ts: u64) -> String {
    // 本地可读时间（进程内简单格式化，避免额外依赖）
    match chrono_like(ts) {
        Some(s) => s,
        None => ts.to_string(),
    }
}

/// unix 秒 → `YYYY-MM-DD HH:MM:SS`（UTC）。
fn chrono_like(ts: u64) -> Option<String> {
    let days = ts / 86400;
    let secs = ts % 86400;
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    // civil_from_days（Howard Hinnant 算法）
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mth <= 2 { y + 1 } else { y };
    Some(format!("{y:04}-{mth:02}-{d:02} {h:02}:{m:02}:{s:02}"))
}

/// 初始化全局日志后端（替换 env_logger —— 终端不再输出任何 log 行）。
pub fn init(filter: &str) {
    let dir = crate::config::Config::default_dir().map(|d| d.join("logs"));
    if let Some(dir) = &dir {
        let _ = std::fs::create_dir_all(dir);
    }
    let path = dir.map(|d| d.join("frp-sh.log"));
    let file = path.as_ref().and_then(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .ok()
            .map(|f| {
                let written = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                (f, written)
            })
    });
    let (file, written) = match file {
        Some((f, w)) => (Some(f), w),
        None => (None, 0),
    };
    {
        let mut g = inner().lock().unwrap();
        g.file = file;
        g.file_path = path;
        g.written = written;
    }
    let level = filter
        .split(',')
        .next()
        .and_then(|s| s.parse::<log::LevelFilter>().ok())
        .unwrap_or(log::LevelFilter::Info);
    log::set_max_level(level);
    let _ = log::set_boxed_logger(Box::new(DebugLogger));
}

struct DebugLogger;

impl log::Log for DebugLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        log::max_level() >= metadata.level()
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let ts = crate::utils::now_unix();
        let level = level_str(record.level()).to_string();
        let msg = format!("[{}] {}", record.target(), record.args());
        let line = LogLine {
            seq: 0,
            ts,
            level,
            msg,
        };
        // 终端静默：仅写入环形缓冲与文件（FRPSH_LOG_STDERR=1 时回显 stderr 调试用）
        let stderr_echo = std::env::var_os("FRPSH_LOG_STDERR").is_some();
        if stderr_echo {
            eprintln!("{} [{}] {}", stamp(line.ts), line.level, line.msg);
        }
        let mut g = inner().lock().unwrap();
        g.seq += 1;
        let mut line = line;
        line.seq = g.seq;
        if g.ring.len() >= RING_CAP {
            g.ring.pop_front();
        }
        g.ring.push_back(line.clone());
        // 文件写入 + 轮转
        if g.written > MAX_FILE_BYTES {
            let old = g.file_path.as_ref().map(|p| p.with_extension("log.old"));
            g.file = None;
            if let (Some(p), Some(old)) = (&g.file_path, old) {
                let _ = std::fs::rename(p, old);
                g.file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)
                    .ok();
            }
            g.written = 0;
        }
        if let Some(f) = g.file.as_mut() {
            let text = format!("{} [{}] {}\n", stamp(line.ts), line.level, line.msg);
            if f.write_all(text.as_bytes()).is_ok() {
                g.written += text.len() as u64;
            }
        }
    }

    fn flush(&self) {
        if let Some(f) = inner().lock().unwrap().file.as_mut() {
            let _ = f.flush();
        }
    }
}

/// 面板 `/api/debug`：环形缓冲快照 + 日志文件路径。
pub fn debug_json() -> serde_json::Value {
    let g = inner().lock().unwrap();
    serde_json::json!({
        "total": g.seq,
        "lines": g.ring.iter().collect::<Vec<_>>(),
        "file": g.file_path.as_ref().map(|p| p.display().to_string()),
    })
}
