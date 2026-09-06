//! Locale selection happens before Clap produces help or errors.
use std::sync::atomic::{AtomicBool, Ordering};
static ZH: AtomicBool = AtomicBool::new(false);
pub fn text<'a>(en: &'a str, zh: &'a str) -> &'a str {
    if ZH.load(Ordering::Relaxed) {
        zh
    } else {
        en
    }
}
pub fn chinese() -> bool {
    ZH.load(Ordering::Relaxed)
}
pub fn choose(requested: &str) {
    let value = if requested == "auto" {
        std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_MESSAGES"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_else(|_| system_locale())
    } else {
        requested.to_string()
    };
    ZH.store(
        value.to_ascii_lowercase().starts_with("zh"),
        Ordering::Relaxed,
    );
}
fn system_locale() -> String {
    #[cfg(windows)]
    {
        let mut buf = [0u16; 85];
        let n = unsafe {
            windows_sys::Win32::Globalization::GetUserDefaultLocaleName(
                buf.as_mut_ptr(),
                buf.len() as i32,
            )
        };
        if n > 1 {
            return String::from_utf16_lossy(&buf[..n as usize - 1]);
        }
    }
    "en".into()
}
pub fn message(input: &str) -> String {
    if !chinese() {
        return input.into();
    }
    static CATALOG: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    let catalog = CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("locales/messages.json")).expect("message catalog")
    });
    let mut out = input.to_string();
    for (en, zh) in catalog {
        out = out.replace(en, zh);
    }
    out
}
pub fn command(mut cmd: clap::Command) -> clap::Command {
    if !chinese() {
        return cmd;
    }
    cmd = cmd
        .help_template(
            "{before-help}{about-with-newline}\n用法： {usage}\n\n{all-args}{after-help}",
        )
        .subcommand_help_heading("命令");
    if let Some(about) = cmd.get_about() {
        cmd = cmd.clone().about(help(&about.to_string()));
    }
    let ids: Vec<_> = cmd
        .get_arguments()
        .filter_map(|a| {
            a.get_help()
                .map(|h| (a.get_id().to_string(), help(&h.to_string())))
        })
        .collect();
    for (id, translated) in ids {
        cmd = cmd.mut_arg(id, |a| {
            let heading = if a.is_positional() {
                "参数"
            } else {
                "选项"
            };
            a.help(translated).help_heading(heading)
        });
    }
    let children: Vec<_> = cmd
        .get_subcommands()
        .map(|c| c.get_name().to_owned())
        .collect();
    for name in children {
        cmd = cmd.mut_subcommand(name, command);
    }
    cmd
}
fn help(en: &str) -> String {
    let dict: std::collections::HashMap<String, String> =
        serde_json::from_str(include_str!("locales/zh-CN.json")).expect("locale catalog");
    dict.get(en).cloned().unwrap_or_else(|| message(en))
}
