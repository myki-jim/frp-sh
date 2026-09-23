use super::{client::Client, loopback_target, normalize_domain, Operation};
use crate::{config::Config, device::key::DeviceKey};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start ownership verification and print the required DNS records
    Bind { domain: String },
    /// Verify the published ownership TXT record
    Verify { domain: String },
    /// Show ownership and verification state
    Status { domain: String },
    /// Remove a domain binding immediately
    Unbind { domain: String },
    /// Keep this device online and publish a loopback HTTP service
    Publish {
        domain: String,
        #[arg(long)]
        target: String,
    },
}

pub async fn run(
    command: Command,
    server: Option<String>,
    identity: Option<PathBuf>,
    config: Option<PathBuf>,
) -> anyhow::Result<()> {
    let cfg = Config::load_auto(config.as_deref())?;
    let key_path = match identity {
        Some(path) => path,
        None => {
            let directory = Config::default_dir()
                .ok_or_else(|| anyhow::anyhow!("no private configuration directory"))?;
            let mut builder = std::fs::DirBuilder::new();
            builder.recursive(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            builder.create(&directory)?;
            directory.join("device.key")
        }
    };
    let key = DeviceKey::load_or_create(&key_path)?;
    let origin = server.unwrap_or(cfg.signaling_addr);
    let client = Client::new(&origin)?;
    if let Command::Publish { domain, target } = command {
        let domain = normalize_domain(&domain)?;
        let target = loopback_target(&target)?;
        return client.publish(&key, domain, target).await;
    }
    let operation = match command {
        Command::Bind { domain } => Operation::Bind {
            domain: normalize_domain(&domain)?,
        },
        Command::Verify { domain } => Operation::Verify {
            domain: normalize_domain(&domain)?,
        },
        Command::Status { domain } => Operation::Status {
            domain: normalize_domain(&domain)?,
        },
        Command::Unbind { domain } => Operation::Unbind {
            domain: normalize_domain(&domain)?,
        },
        Command::Publish { .. } => unreachable!(),
    };
    let result = client.execute(&key, operation).await?;
    crate::ui_println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
