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
