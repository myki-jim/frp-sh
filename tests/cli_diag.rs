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
