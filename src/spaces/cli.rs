use super::{client::Client, Operation};
use crate::{config::Config, device::key::DeviceKey, invite_ticket::Ticket};
use clap::Subcommand;
use std::path::PathBuf;
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Attach this device's virtual network to an authorized permanent space
    Connect { space: String },
    /// Redeem a temporary invitation then attach the virtual network
    Join {
        #[arg(conflicts_with = "stdin")]
        invitation: Option<String>,
        #[arg(long, required_unless_present = "invitation")]
        stdin: bool,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Create a durable space registration (network attachment is a separate operation)
    Create {
        name: String,
        #[arg(long)]
        expires_in: Option<u64>,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// List spaces authorized for this device
    List,
    /// List registered devices in a space
    Members { space: String },
    /// Issue a temporary invitation; default lifetime comes from the server
    Invite {
        space: String,
        #[arg(long,default_value_t=1,value_parser=clap::value_parser!(u32).range(1..=32))]
        uses: u32,
        #[arg(long)]
        expires_in: Option<u64>,
    },
    /// Redeem a temporary invitation; read from stdin to avoid shell history
    Redeem {
        #[arg(conflicts_with = "stdin")]
        invitation: Option<String>,
        #[arg(long, required_unless_present = "invitation")]
        stdin: bool,
        #[arg(long)]
        request_id: Option<String>,
    },
    /// Revoke all outstanding invitations for an owned space
    RevokeInvites { space: String },
    /// Remove a device and its ability to authenticate as a space member
    RemoveMember { space: String, device: String },
    /// Remove this device's membership
    Leave { space: String },
    /// Permanently delete a space owned by this device
    Delete {
        space: String,
        #[arg(long, required = true)]
        yes: bool,
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
    let mut origin = server.unwrap_or_else(|| cfg.signaling_addr.clone());
    match command {
        Command::Connect { space } => {
            crate::debuglog::init("info");
            return super::network::run(Client::new(&origin)?, key, space).await;
        }
        Command::Join {
            invitation,
            stdin,
            request_id,
        } => {
            let link = read_invitation(invitation, stdin)?;
            let ticket = Ticket::parse(link.trim())?;
            origin = ticket.server().into();
            let result = Client::new(&origin)?
                .execute(
                    &key,
                    Operation::Redeem {
                        token: ticket.token().into(),
                        request_id: request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    },
                    None,
                )
                .await?;
            let space = result
                .get("space_id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("invalid invitation response"))?
                .to_owned();
            crate::debuglog::init("info");
            return super::network::run(Client::new(&origin)?, key, space).await;
        }
        _ => {}
    }
    let operation = match command {
        Command::Connect { .. } => unreachable!(),
        Command::Join { .. } => unreachable!(),
        Command::Create {
            name,
            expires_in,
            request_id,
        } => Operation::Create {
            name,
            expires_at: expires_in
                .map(|t| {
                    crate::utils::now_unix()
                        .checked_add(t)
                        .ok_or_else(|| anyhow::anyhow!("invalid lifetime"))
                })
                .transpose()?,
            request_id: request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        },
        Command::List => Operation::List,
        Command::Members { space } => Operation::Members { space },
        Command::Invite {
            space,
            uses,
            expires_in,
        } => Operation::Invite {
            space,
            uses,
            ttl: expires_in,
        },
        Command::Redeem {
            invitation,
            stdin,
            request_id,
        } => {
            let link = read_invitation(invitation, stdin)?;
            let ticket = Ticket::parse(link.trim())?;
            origin = ticket.server().into();
            Operation::Redeem {
                token: ticket.token().into(),
                request_id: request_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            }
        }
        Command::RevokeInvites { space } => Operation::RevokeInvites { space },
        Command::RemoveMember { space, device } => Operation::RemoveMember { space, device },
        Command::Leave { space } => Operation::Leave { space },
        Command::Delete { space, .. } => Operation::Delete { space },
    };
    // Never send the configured administrative secret to an invitation's server.
    let password = matches!(operation, Operation::Create { .. })
        .then_some(cfg.password.as_deref())
        .flatten();
    let result = Client::new(&origin)?
        .execute(&key, operation, password)
        .await?;
    crate::ui_println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
fn read_invitation(invitation: Option<String>, stdin: bool) -> anyhow::Result<String> {
    if !stdin {
        return invitation.ok_or_else(|| anyhow::anyhow!("invitation required"));
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin().take(6001).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() <= 6000, "invitation too large");
    String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("invalid invitation"))
}
