//! Portable invitations. The URL fragment stays in the recipient's browser.
use crate::config::{Config, Profile};
use anyhow::{bail, ensure, Context};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

const PREFIX: &str = "https://frp.sh/join#v1.";
static ACTIVE: Mutex<Option<Invitation>> = Mutex::new(None);

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invitation {
    pub server: String,
    pub room: String,
    pub relay: String,
    pub password: Option<String>,
    pub key: Option<String>,
}
impl Invitation {
    pub fn validate(&self) -> anyhow::Result<()> {
        let url = reqwest::Url::parse(&self.server).context("Invalid invitation server")?;
        ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "Invalid invitation server"
        );
        ensure!(
            !self.room.is_empty()
                && self.room.len() <= 128
                && self
                    .room
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'),
            "Invalid room ID"
        );
        ensure!(
            self.relay.len() <= 255
                && !self
                    .relay
                    .chars()
                    .any(|c| c.is_control() || c.is_whitespace()),
            "Invalid relay address"
        );
        for value in [&self.password, &self.key].into_iter().flatten() {
            ensure!(value.len() <= 1024, "Invitation credential is too long");
        }
        Ok(())
    }
    pub fn link(&self) -> anyhow::Result<String> {
        self.validate()?;
        Ok(format!(
            "{PREFIX}{}",
            hex::encode(serde_json::to_vec(self)?)
        ))
    }
    pub fn parse(link: &str) -> anyhow::Result<Self> {
        ensure!(link.len() <= 12000, "Invitation is too long");
        let token = link
            .trim()
            .strip_prefix(PREFIX)
            .context("Expected a frp.sh invitation link")?;
        let bytes = hex::decode(token).context("Invalid invitation encoding")?;
        // Do not include serde's input fragments (which may contain credentials) in errors.
        let invite: Self = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("Invalid invitation data"))?;
        invite.validate()?;
        crate::debuglog::protect(link);
        for value in [&invite.password, &invite.key].into_iter().flatten() {
            crate::debuglog::protect(value);
        }
        Ok(invite)
    }
    pub fn command(&self, windows: bool) -> anyhow::Result<String> {
        let link = self.link()?; // Only fixed URL punctuation and hex; never interpolate raw data into a shell.
        Ok(if windows {
            format!("irm https://frp.sh/install.ps1 | iex; if ($?) {{ & \"$env:ProgramFiles\\frp-sh\\frp-sh.exe\" connect '{link}' }}")
        } else {
            format!("sh -c 's=$(curl -fsSL https://frp.sh/install.sh) && printf \"%s\\n\" \"$s\" | sh && exec /usr/local/bin/frp-sh connect \"$1\"' sh '{link}'")
        })
    }
    pub fn save(&self, cfg: &mut Config) -> String {
        let name = cfg
            .profiles
            .values()
            .find(|p| p.server == self.server && p.room == self.room && p.mode == "lan")
            .map(|p| p.name.clone())
            .unwrap_or_else(|| cfg.next_profile_name());
        cfg.profiles.insert(
            name.clone(),
            Profile {
                name: name.clone(),
                server: self.server.clone(),
                room: self.room.clone(),
                password: self.password.clone(),
                key: self.key.clone(),
                mode: "lan".into(),
                device_name: None,
                relay_addr: Some(self.relay.clone()),
                listen: None,
                expose_lan: false,
                default: false,
            },
        );
        cfg.mark_default_profile(&name);
        name
    }
}
pub struct Active;
impl Drop for Active {
    fn drop(&mut self) {
        *ACTIVE.lock().unwrap() = None;
    }
}
pub fn activate(cfg: &Config, room: &str, key: Option<&str>, lan: bool) -> Active {
    *ACTIVE.lock().unwrap() = lan.then(|| Invitation {
        server: cfg.signaling_addr.clone(),
        room: room.into(),
        relay: cfg.relay_addr.clone(),
        password: cfg.password.clone(),
        key: key.map(str::to_owned),
    });
    Active
}
pub fn current() -> Option<Invitation> {
    ACTIVE.lock().unwrap().clone()
}

pub fn copy(text: &str) -> anyhow::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    #[cfg(windows)]
    let candidates: &[(&str, &[&str])] = &[(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$input | Set-Clipboard",
        ],
    )];
    #[cfg(target_os = "macos")]
    let candidates: &[(&str, &[&str])] = &[("pbcopy", &[])];
    #[cfg(all(unix, not(target_os = "macos")))]
    let candidates: &[(&str, &[&str])] = &[
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard", "-in"]),
    ];
    for (program, args) in candidates {
        let mut command = Command::new(program);
        command
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        if let Ok(mut child) = command.spawn() {
            let written = child
                .stdin
                .take()
                .context("Clipboard input unavailable")?
                .write_all(text.as_bytes());
            let result = child.wait();
            if written.is_ok() && result.is_ok_and(|s| s.success()) {
                return Ok(());
            }
        }
    }
    bail!("Clipboard unavailable; use the invitation page to select the command")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invitation_roundtrip_is_data_not_shell_code() {
        let invite = Invitation {
            server: "https://example.test:8080".into(),
            room: "room_1234".into(),
            relay: "127.0.0.1:8081".into(),
            password: Some("'; $(touch /tmp/no); 测试".into()),
            key: Some("private-test-key".into()),
        };
        let link = invite.link().unwrap();
        let decoded = Invitation::parse(&link).unwrap();
        assert_eq!(decoded.password, invite.password);
        for windows in [true, false] {
            let cmd = invite.command(windows).unwrap();
            assert!(!cmd.contains("touch /tmp"));
            assert!(!cmd.contains("private-test-key"));
            assert!(cmd.contains(&link));
        }
        assert!(!crate::debuglog::redact(&link).contains("#v1."));
        let mut cfg = Config::default();
        let first = decoded.save(&mut cfg);
        let second = decoded.save(&mut cfg);
        assert_eq!(first, second);
        assert_eq!(cfg.profiles.len(), 1);
        assert!(!cfg.profiles[&first].expose_lan);
        assert!(cfg.profiles[&first].default);
    }
    #[test]
    fn malformed_invites_are_rejected_without_echoing_credentials() {
        for link in [
            "https://evil.test/join#v1.00",
            "https://frp.sh/join#v1.123",
            "https://frp.sh/join#v1.7b7d",
        ] {
            assert!(Invitation::parse(link).is_err());
        }
        let mut invite = Invitation {
            server: "file:///tmp/test".into(),
            room: "1234".into(),
            relay: "127.0.0.1:8081".into(),
            password: None,
            key: None,
        };
        assert!(invite.link().is_err());
        invite.server = "https://example.test".into();
        invite.room = "1234;whoami".into();
        assert!(invite.link().is_err());
    }
}
