//! Bounded asynchronous JSONL logging. No diagnostic output reaches the terminal.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, BufRead, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Mutex, OnceLock,
    },
    time::Duration,
};
const FILE_LIMIT: u64 = 5 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 64 * 1024 * 1024;
const QUEUE_CAP: usize = 1024;
static SECRETS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static LOGGER: OnceLock<Backend> = OnceLock::new();
static DROPPED: AtomicU64 = AtomicU64::new(0);
static WRITE_ERRORS: AtomicU64 = AtomicU64::new(0);

pub fn protect(value: &str) {
    if value.is_empty() {
        return;
    }
    let mut values = SECRETS.get_or_init(Default::default).lock().unwrap();
    if !values.iter().any(|v| v == value) {
        values.push(value.into());
        values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    }
}
pub fn redact(text: &str) -> String {
    let mut s = text.to_owned();
    if let Some(values) = SECRETS.get() {
        for value in values.lock().unwrap().iter() {
            s = s.replace(value, "[REDACTED]");
        }
    }
    // Cover URL userinfo, including TURN URLs; retain the server address.
    let mut pos = 0;
    while let Some(i) = s[pos..].find("://") {
        let start = pos + i + 3;
        let end = s[start..]
            .find(['/', ' ', '\n', '"', '\''])
            .map(|n| start + n)
            .unwrap_or(s.len());
        if let Some(at) = s[start..end].rfind('@') {
            s.replace_range(start..start + at, "[REDACTED]");
        }
        pos = start;
        if pos >= s.len() {
            break;
        }
    }
    // Redact assignment/header values without including the value in diagnostics.
    for key in [
        "authorization",
        "password",
        "owner_token",
        "owner-token",
        "api_key",
        "api-key",
        "token",
        "secret",
        "--key",
    ] {
        let mut cursor = 0;
        loop {
            let lower = s.to_ascii_lowercase();
            let Some(relative) = lower[cursor..].find(key) else {
                break;
            };
            let at = cursor + relative;
            let mut start = at + key.len();
            cursor = start;
            if at > 0 && s.as_bytes()[at - 1].is_ascii_alphanumeric() {
                continue;
            }
            while start < s.len()
                && matches!(
                    s.as_bytes()[start],
                    b' ' | b'\t' | b'"' | b'\'' | b':' | b'='
                )
            {
                start += 1;
            }
            if start == cursor || start >= s.len() {
                continue;
            }
            let mut end = start;
            while end < s.len()
                && !matches!(
                    s.as_bytes()[end],
                    b'\r' | b'\n' | b'"' | b'\'' | b',' | b'&' | b'}'
                )
            {
                if key != "authorization" && s.as_bytes()[end].is_ascii_whitespace() {
                    break;
                }
                end += 1;
            }
            if end > start {
                s.replace_range(start..end, "[REDACTED]");
                cursor = start + 10;
            }
        }
    }
    s
}
enum Event {
    Line(String),
    Flush(mpsc::Sender<()>),
}
struct Backend {
    tx: mpsc::SyncSender<Event>,
    path: PathBuf,
    filters: Vec<(String, log::LevelFilter)>,
}
pub fn directory() -> PathBuf {
    crate::config::Config::default_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("logs")
}
pub fn file_path() -> Option<PathBuf> {
    LOGGER.get().map(|l| l.path.clone())
}
pub fn dropped() -> u64 {
    DROPPED.load(Ordering::Relaxed)
}
pub fn write_errors() -> u64 {
    WRITE_ERRORS.load(Ordering::Relaxed)
}
fn open(path: &std::path::Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
fn prune(dir: &std::path::Path, active: &std::path::Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if !name.starts_with("frp-sh-") || !name.contains(".jsonl") {
                return None;
            }
            let m = e.metadata().ok()?;
            if !m.is_file() {
                return None;
            }
            Some((e.path(), m.len(), m.modified().ok()))
        })
        .collect();
    files.sort_by_key(|f| f.2);
    let mut bytes: u64 = files.iter().map(|f| f.1).sum();
    for (path, size, _) in files {
        if bytes <= TOTAL_LIMIT {
            break;
        }
        let live = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.split_once(".jsonl"))
            .and_then(|(stem, _)| stem.rsplit('-').next())
            .and_then(|pid| pid.parse::<u32>().ok())
            .is_some_and(process_alive);
        if path != active && !live && fs::remove_file(path).is_ok() {
            bytes = bytes.saturating_sub(size);
        }
    }
}
fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(pid as i32, 0) == 0
                || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        };
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return io::Error::last_os_error().raw_os_error() == Some(5);
        }
        unsafe {
            CloseHandle(handle);
        }
        true
    }
}
fn worker(path: PathBuf, rx: mpsc::Receiver<Event>) {
    let mut file = open(&path).ok();
    let mut written = 0u64;
    while let Ok(event) = rx.recv() {
        match event {
            Event::Flush(done) => {
                if let Some(f) = file.as_mut() {
                    let _ = f.flush();
                }
                let _ = done.send(());
            }
            Event::Line(line) => {
                if written + line.len() as u64 > FILE_LIMIT {
                    drop(file.take());
                    for n in (1..=3).rev() {
                        let from = if n == 1 {
                            path.clone()
                        } else {
                            path.with_extension(format!("jsonl.{}", n - 1))
                        };
                        let to = path.with_extension(format!("jsonl.{n}"));
                        if to.exists() {
                            let _ = fs::remove_file(&to);
                        }
                        let _ = fs::rename(from, to);
                    }
                    if let Some(dir) = path.parent() {
                        prune(dir, &path);
                    }
                    file = open(&path).ok();
                    written = 0;
                }
                if file.is_none() {
                    file = open(&path).ok();
                }
                match file.as_mut().map(|f| f.write_all(line.as_bytes())) {
                    Some(Ok(())) => written += line.len() as u64,
                    _ => {
                        WRITE_ERRORS.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}
pub fn init(default: &str) {
    if LOGGER.get().is_some() {
        return;
    }
    let dir = directory();
    let _ = fs::create_dir_all(&dir);
    let path = dir.join(format!(
        "frp-sh-{}-{}.jsonl",
        crate::utils::now_unix(),
        std::process::id()
    ));
    prune(&dir, &path);
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| default.into());
    let mut filters: Vec<(String, log::LevelFilter)> = filter
        .split(',')
        .filter_map(|part| {
            let (target, level) = part.split_once('=').unwrap_or(("", part));
            Some((target.into(), level.parse::<log::LevelFilter>().ok()?))
        })
        .collect();
    if filters.is_empty() {
        filters.push(("".into(), log::LevelFilter::Info));
    }
    filters.sort_by_key(|(target, _)| target.len());
    let max = filters.iter().map(|(_, level)| *level).max().unwrap();
    let (tx, rx) = mpsc::sync_channel(QUEUE_CAP);
    let worker_path = path.clone();
    if std::thread::Builder::new()
        .name("frpsh-log".into())
        .spawn(move || worker(worker_path, rx))
        .is_err()
    {
        return;
    }
    if LOGGER.set(Backend { tx, path, filters }).is_ok() {
        let _ = log::set_boxed_logger(Box::new(Logger));
        log::set_max_level(max);
    }
}
struct Logger;
impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        LOGGER
            .get()
            .and_then(|l| {
                l.filters.iter().rev().find(|(t, _)| {
                    t.is_empty() || m.target() == t || m.target().starts_with(&format!("{t}::"))
                })
            })
            .is_some_and(|(_, level)| m.level() <= *level)
    }
    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let Some(l) = LOGGER.get() else { return };
        let mut msg = redact(&format!("{}", r.args()));
        if msg.len() > 16 * 1024 {
            let mut n = 16 * 1024;
            while !msg.is_char_boundary(n) {
                n -= 1;
            }
            msg.truncate(n);
            msg.push_str(" [truncated]");
        }
        let line=serde_json::json!({"ts":crate::utils::now_unix(),"pid":std::process::id(),"level":r.level().as_str(),"target":r.target(),"message":msg,"dropped":dropped(),"write_errors":write_errors()}).to_string()+"\n";
        if l.tx.try_send(Event::Line(line)).is_err() {
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn flush(&self) {
        if let Some(l) = LOGGER.get() {
            let (tx, rx) = mpsc::channel();
            if l.tx.try_send(Event::Flush(tx)).is_ok() {
                let _ = rx.recv_timeout(Duration::from_secs(1));
            }
        }
    }
}
pub fn latest_file() -> Option<PathBuf> {
    fs::read_dir(directory())
        .ok()?
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".jsonl"))
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}
pub async fn tail(lines: usize, follow: bool, level: Option<&str>) -> anyhow::Result<()> {
    use std::io::{Seek, SeekFrom};
    let path =
        latest_file().ok_or_else(|| anyhow::anyhow!("No log files; run a connection first."))?;
    let text = fs::read_to_string(&path)?;
    let selected: Vec<_> = text
        .lines()
        .filter(|s| {
            level.is_none_or(|l| {
                serde_json::from_str::<serde_json::Value>(s)
                    .ok()
                    .and_then(|v| v["level"].as_str().map(str::to_owned))
                    .is_some_and(|s| s.eq_ignore_ascii_case(l))
            })
        })
        .collect();
    for s in selected.iter().skip(selected.len().saturating_sub(lines)) {
        println!("{}", redact(s));
    }
    let mut position = text.len() as u64;
    if !follow {
        return Ok(());
    }
    loop {
        tokio::select! {_ = tokio::signal::ctrl_c()=>break,_=tokio::time::sleep(Duration::from_millis(250))=>{}}
        let Ok(mut f) = File::open(&path) else {
            continue;
        };
        if f.metadata()?.len() < position {
            position = 0;
        }
        f.seek(SeekFrom::Start(position))?;
        let mut r = io::BufReader::new(f);
        let mut s = String::new();
        while r.read_line(&mut s)? > 0 {
            if !s.ends_with('\n') {
                break;
            }
            position += s.len() as u64;
            let show = level.is_none_or(|l| {
                serde_json::from_str::<serde_json::Value>(&s)
                    .ok()
                    .and_then(|v| v["level"].as_str().map(str::to_owned))
                    .is_some_and(|s| s.eq_ignore_ascii_case(l))
            });
            if show {
                print!("{}", redact(&s));
            }
            s.clear();
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_are_redacted() {
        protect("unit-test-credential");
        let s = redact(
            "unit-test-credential turn://alice:abcd@127.0.0.1 password=\"hidden\" token=hidden&x=1",
        );
        assert!(!s.contains("unit-test-credential"));
        assert!(!s.contains("abcd"));
        assert!(!s.contains("hidden"));
        assert!(s.contains("127.0.0.1"));
    }
    #[test]
    fn headers_and_unicode_are_safe() {
        assert_eq!(
            redact("Authorization: Bearer confidential\n中文"),
            "Authorization: [REDACTED]\n中文"
        );
        assert!(redact("{\"password\":\"测试密码\"}").contains("[REDACTED]"));
    }
}
