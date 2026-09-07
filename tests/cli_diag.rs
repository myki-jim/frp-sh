//! 临时诊断：定位栈溢出。
use clap::Parser;
use frp_sh::cli::Cli;

#[test]
fn parse_plain_ok() {
    let cli = Cli::try_parse_from(["frp-sh"]).unwrap();
    assert!(cli.command.is_none());
}

#[test]
fn parse_lan_ok() {
    let cli = Cli::try_parse_from(["frp-sh", "lan", "create"]).unwrap();
    assert!(cli.command.is_some());
}

#[test]
fn game_defaults_to_device_network_and_explicit_service_does_not() {
    let mesh = Cli::try_parse_from(["frp-sh", "game", "create"]).unwrap();
    assert!(frp_sh::commands::needs_network_helper(&mesh.command));
    let service = Cli::try_parse_from(["frp-sh", "game", "create", "--service", "3000"]).unwrap();
    assert!(!frp_sh::commands::needs_network_helper(&service.command));
    let guest = Cli::try_parse_from(["frp-sh", "game", "join", "1234"]).unwrap();
    assert!(!frp_sh::commands::needs_network_helper(&guest.command));
}

#[test]
fn create_presets_merge_explicit_values_without_changing_saved_parameters() {
    use frp_sh::{
        cli::Commands,
        config::Config,
        presets::{resolve, RoomPreset},
    };
    let mut cfg = Config::default();
    cfg.presets.insert(
        "mine".into(),
        RoomPreset {
            relay: true,
            mtu: 1280,
            ..Default::default()
        },
    );
    let cli = Cli::try_parse_from([
        "frp-sh",
        "create",
        "dev",
        "--preset",
        "mine",
        "--mtu",
        "1300",
        "--auto-route",
    ])
    .unwrap();
    let Some(Commands::Create(args)) = cli.command else {
        panic!("create not parsed")
    };
    let resolved = resolve(args, &cfg).unwrap();
    assert_eq!(resolved.mtu, 1300);
    assert!(!resolved.relay);
    assert!(!resolved.expose_lan);
    assert_eq!(cfg.presets["mine"].mtu, 1280);
    let cli = Cli::try_parse_from(["frp-sh", "0038"]).unwrap();
    assert!(matches!(cli.command, Some(Commands::Shortcut(a)) if a[0] == "0038"));
}
