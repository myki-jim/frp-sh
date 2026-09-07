//! Room presets contain parameters only, never room credentials or session state.
use crate::{
    cli::{LanCreateArgs, RoomCreateArgs},
    config::Config,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RoomPreset {
    pub scene: String,
    pub ttl: u64,
    pub mtu: u16,
    pub spread: u32,
    pub relay: bool,
    pub prefix: String,
}
impl Default for RoomPreset {
    fn default() -> Self {
        Self {
            scene: "lan".into(),
            ttl: 43200,
            mtu: 1400,
            spread: 2,
            relay: false,
            prefix: String::new(),
        }
    }
}
impl RoomPreset {
    pub fn builtin(name: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(
            matches!(name, "lan" | "game" | "dev"),
            "Unknown scene: {name}"
        );
        Ok(Self {
            scene: name.into(),
            ..Self::default()
        })
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        Self::builtin(&self.scene)?;
        anyhow::ensure!(
            (60..=604800).contains(&self.ttl),
            "Room lifetime must be 60..604800 seconds"
        );
        anyhow::ensure!((576..=1400).contains(&self.mtu), "MTU must be 576..1400");
        anyhow::ensure!(self.spread <= 16, "Spread must be 0..16");
        anyhow::ensure!(
            self.prefix.len() <= 24
                && self
                    .prefix
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "Prefix must contain at most 24 letters, digits or hyphens"
        );
        Ok(())
    }
    pub fn arguments(&self) -> Vec<String> {
        let mut args = vec![
            "create".into(),
            self.scene.clone(),
            "--ttl".into(),
            self.ttl.to_string(),
            "--mtu".into(),
            self.mtu.to_string(),
            "--spread".into(),
            self.spread.to_string(),
        ];
        if self.relay {
            args.push("--relay".into());
        }
        if !self.prefix.is_empty() {
            args.extend(["--prefix".into(), self.prefix.clone()]);
        }
        args
    }
}
pub fn validate_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !name.trim().is_empty() && name.len() <= 80 && !name.chars().any(char::is_control),
        "Preset name must be 1..80 bytes without control characters"
    );
    anyhow::ensure!(
        !matches!(name, "lan" | "game" | "dev"),
        "Built-in presets are read-only; choose another name"
    );
    Ok(())
}
pub fn resolve(args: RoomCreateArgs, cfg: &Config) -> anyhow::Result<LanCreateArgs> {
    let mut p = if let Some(name) = args.preset.as_deref() {
        match cfg.presets.get(name) {
            Some(p) => p.clone(),
            None => RoomPreset::builtin(name)?,
        }
    } else {
        RoomPreset::default()
    };
    if let Some(scene) = args.scene {
        p.scene = scene;
    }
    if let Some(ttl) = args.ttl {
        p.ttl = ttl;
    }
    if let Some(mtu) = args.mtu {
        p.mtu = mtu;
    }
    if let Some(spread) = args.spread {
        p.spread = spread;
    }
    if let Some(prefix) = args.prefix {
        p.prefix = prefix;
    }
    if args.relay {
        p.relay = true;
    }
    if args.auto_route {
        p.relay = false;
    }
    p.validate()?;
    Ok(LanCreateArgs {
        prefix: (!p.prefix.is_empty()).then_some(p.prefix),
        ttl: p.ttl,
        mtu: p.mtu,
        spread: p.spread,
        relay: p.relay,
        key: args.key,
        ip: None,
        netmask: "255.255.255.0".into(),
        guest_ips: vec![],
        expose_lan: false,
    })
}
