//! 更新检查：启动时检测新版本、代差判断、交互询问与安装。
//!
//! - 版本来源：`https://frp.sh/latest-version.txt`（Cloudflare，国内可达）优先，
//!   GitHub Releases API 兜底
//! - 节流：缓存上次检查结果（默认 6 小时），避免每次启动都请求网络
//! - 代差：`version::is_breaking_gap` 判定；代差过大时不可跳过（强提示）
//! - 安装：Windows 下载新 exe 并在本进程退出后延迟替换；其他平台打印安装命令

use std::io::{BufRead, Write};
use std::time::Duration;

/// 两次检查的最小间隔（秒）。
const CHECK_INTERVAL_SECS: u64 = 6 * 3600;
/// 下载超时（秒，仅 Windows 自动替换使用）。
#[cfg(target_os = "windows")]
const DOWNLOAD_TIMEOUT_SECS: u64 = 180;

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
        let resp = client
            .get(url)
            .header("User-Agent", "frp-sh-update-check")
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            continue;
        }
        if url.contains("api.github.com") {
            let v: serde_json::Value = resp.json().await.ok()?;
            let tag = v.get("tag_name").and_then(|x| x.as_str())?;
            let clean = tag.trim_start_matches('v').trim().to_string();
            if !clean.is_empty() {
                return Some(clean);
            }
            continue;
        }
        let text = resp.text().await.ok()?;
        let clean = text.trim().to_string();
        if !clean.is_empty() {
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
        "\n  [更新] 发现新版本 v{latest}（当前 v{current}）{}",
        if breaking {
            "——版本差异较大，可能存在不兼容，建议立即更新"
        } else {
            ""
        }
    );
    if !interactive {
        println!("  服务器请择机更新：curl -fsSL https://frp.sh/install.sh | sh（或安装脚本）\n");
        return Ok(());
    }
    print!("  是否现在更新？[Y/n] ");
    std::io::stdout().flush().ok();
    let answer = read_stdin_line();
    let yes = answer.trim().is_empty()
        || answer.trim().eq_ignore_ascii_case("y")
        || answer.trim().eq_ignore_ascii_case("yes");
    if !yes {
        if breaking {
            println!("  [警告] 版本差异较大仍选择跳过；如遇异常请更新到 v{latest}。\n");
        } else {
            println!("  已跳过本次更新（下次启动再提示）。\n");
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
    #[cfg(target_os = "windows")]
    {
        let url = "https://frp.sh/downloads/frp-sh-windows-x86_64.exe";
        let exe = std::env::current_exe()?;
        let tmp = std::env::temp_dir().join(format!("frp-sh-{latest}.exe"));
        println!("  正在下载 v{latest} ...");
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
            .build()?;
        let bytes = client.get(url).send().await?.bytes().await?;
        if bytes.len() < 1_000_000 {
            anyhow::bail!("下载异常（文件过小），请手动运行: irm https://frp.sh/install.ps1 | iex");
        }
        std::fs::write(&tmp, &bytes)?;
        println!("  下载完成（{} 字节）。", bytes.len());
        // 本进程退出后延迟替换（运行中的 exe 被锁定）
        let cmd = format!(
            "timeout /t 2 /nobreak >nul & move /y \"{}\" \"{}\"",
            tmp.display(),
            exe.display()
        );
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            std::process::Command::new("cmd")
                .args(["/c", &cmd])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()?;
        }
        #[cfg(not(target_os = "windows"))]
        let _ = cmd;
        println!("  更新完成，重启 frp-sh 后生效。\n");
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = latest;
        println!(
            "  请运行以下命令更新（当前进程退出后执行）：\n    curl -fsSL https://frp.sh/install.sh | sh\n"
        );
        Ok(())
    }
}
