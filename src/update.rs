//! 更新检查：启动时检测新版本、代差判断、交互询问与安装。
//!
//! - 版本来源：`https://frp.sh/latest-version.txt`（Cloudflare，国内可达）优先，
//!   GitHub Releases API 兜底
//! - 节流：缓存上次检查结果（默认 6 小时），避免每次启动都请求网络
//! - 代差：`version::is_breaking_gap` 判定；代差过大时不可跳过（强提示）
//! - 安装：Windows 下载新 exe 并在本进程退出后延迟替换；其他平台打印安装命令

use std::io::{BufRead, IsTerminal, Write};
use std::time::Duration;

/// 两次检查的最小间隔（秒）。
const CHECK_INTERVAL_SECS: u64 = 6 * 3600;

/// 版本来源（按顺序尝试）。
fn latest_urls() -> [&'static str; 2] {
    [
        "https://frp.sh/latest-version.txt",
        "https://api.github.com/repos/myki-jim/frp-sh/releases/latest",
    ]
}

fn cache_path() -> Option<std::path::PathBuf> {
    crate::config::Config::default_dir().map(|d| d.join("update-check.json"))
}

/// 读取上次检查时间（秒）；无缓存返回 0。
fn last_check_time() -> u64 {
    let Some(p) = cache_path() else {
        return 0;
    };
    let Ok(text) = std::fs::read_to_string(p) else {
        return 0;
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| {
            v.get("last_check")
                .and_then(|x| x.as_u64())
                .unwrap_or(0)
                .into()
        })
        .unwrap_or(0)
}

/// 写缓存。
fn write_cache(latest: &str) {
    let Some(p) = cache_path() else {
        return;
    };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(
        p,
        serde_json::json!({
            "last_check": crate::utils::now_unix(),
            "latest": latest,
        })
        .to_string(),
    );
}

/// 拉取最新版本号；失败返回 `None`（静默，不打断使用）。
async fn fetch_latest() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .ok()?;
    for url in latest_urls() {
        let Ok(resp) = client
            .get(url)
            .header("User-Agent", "frp-sh-update-check")
            .send()
            .await
        else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        if url.contains("api.github.com") {
            let Ok(v) = resp.json::<serde_json::Value>().await else {
                continue;
            };
            let Some(tag) = v.get("tag_name").and_then(|x| x.as_str()) else {
                continue;
            };
            let clean = tag.trim_start_matches('v').trim().to_string();
            if crate::version::parse(&clean).is_some() {
                return Some(clean);
            }
            continue;
        }
        let Ok(text) = resp.text().await else {
            continue;
        };
        let clean = text.trim().to_string();
        if crate::version::parse(&clean).is_some() {
            return Some(clean);
        }
    }
    None
}

/// 检查更新并交互询问（在进入长连接/向导之前调用）。
///
/// - `interactive`：false（如 serve）只打印提示、不询问、不安装
pub async fn maybe_check_update(interactive: bool) -> anyhow::Result<()> {
    let now = crate::utils::now_unix();
    // 节流：检查间隔内不重复请求
    if now.saturating_sub(last_check_time()) < CHECK_INTERVAL_SECS {
        return Ok(());
    }
    let Some(latest) = fetch_latest().await else {
        return Ok(()); // 网络失败静默
    };
    write_cache(&latest);

    let current = crate::version::VERSION;
    if !crate::version::is_newer(&latest, current) {
        return Ok(()); // 已是最新
    }
    let breaking = crate::version::is_breaking_gap(current, &latest);

    println!(
        "\n  [Update] New version v{latest} available (current v{current}){}",
        if breaking {
            " — big version gap, possible incompatibility; upgrading now is recommended"
        } else {
            ""
        }
    );
    if !interactive || !std::io::stdin().is_terminal() {
        println!("  On the server, update when convenient: curl -fsSL https://frp.sh/install.sh | sh (or the install script)\n");
        return Ok(());
    }
    print!("  Show update instructions? [y/N] ");
    std::io::stdout().flush().ok();
    let answer = read_stdin_line();
    let yes = answer.trim().eq_ignore_ascii_case("y") || answer.trim().eq_ignore_ascii_case("yes");
    if !yes {
        if breaking {
            println!("  [Warning] Skipping despite the big version gap; if you hit issues, upgrade to v{latest}.\n");
        } else {
            println!("  Update skipped (will prompt again on next start).\n");
        }
        return Ok(());
    }
    install_latest(&latest).await?;
    Ok(())
}

fn read_stdin_line() -> String {
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    line.trim().to_string()
}

/// 下载并安装最新版。
async fn install_latest(latest: &str) -> anyhow::Result<()> {
    println!("  Version {latest} is available. Stop active sessions, then run the installer:");
    if cfg!(target_os = "windows") {
        println!("    irm https://frp.sh/install.ps1 | iex");
    } else {
        println!("    curl -fsSL https://frp.sh/install.sh | sh");
    }
    println!("  The running executable has not been replaced.");
    Ok(())
}
