use std::process::Command;
fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_frp-sh"))
        .args(args)
        .env("FRPSH_LANG", "en")
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}
#[test]
fn localized_help_and_json_are_deterministic() {
    let english = cli(&["--lang", "en", "--plain", "--help"]);
    assert!(english.status.success());
    let en = String::from_utf8(english.stdout).unwrap();
    assert!(en.contains("Usage:") && en.contains("doctor") && !en.contains("--panel-addr"));
    assert!(!en.contains('\x1b'));
    let chinese = cli(&["--lang", "zh-CN", "--json", "--help"]);
    assert!(chinese.status.success());
    let value: serde_json::Value = serde_json::from_slice(&chinese.stdout).unwrap();
    assert_eq!(value["event"], "status");
    assert!(value["message"].as_str().unwrap().contains("用法："));
    assert!(chinese.stderr.is_empty());
    let old = cli(&["--json", "--no-panel"]);
    assert_eq!(old.status.code(), Some(2));
    let error: serde_json::Value = serde_json::from_slice(&old.stderr).unwrap();
    assert_eq!(error["code"], "E_ARGUMENT");
    assert!(old.stdout.is_empty());
}
#[test]
fn localization_preserves_dynamic_user_values_and_formatting() {
    frp_sh::i18n::choose("zh-CN");
    let name = "Room created : default connected {x} 中文";
    assert_eq!(
        frp_sh::ui_format!("Room created : {name}"),
        format!("房间已创建： {name}")
    );
    assert_eq!(frp_sh::ui_format!("{{{name}}}"), format!("{{{name}}}"));
    assert_eq!(frp_sh::ui_format!("{:04} {:.2}", 3, 1.25), "0003 1.25");
}
#[test]
fn unspecified_punch_candidates_are_discarded() {
    assert!(frp_sh::p2p::hole_punch::punch_targets("0.0.0.0:100".parse().unwrap(), 3).is_empty());
    assert!(frp_sh::p2p::hole_punch::punch_targets("224.0.0.1:100".parse().unwrap(), 3).is_empty());
}
