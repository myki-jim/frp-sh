//! Desired connection references, not credentials or arbitrary commands.
use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub schema_version: u32,
    pub enabled: bool,
    pub config: PathBuf,
    pub profile: String,
}
impl Job {
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(self.schema_version == 1, "unsupported agent job version");
        ensure!(
            self.config.is_absolute(),
            "agent config path must be absolute"
        );
        ensure!(
            !self.profile.trim().is_empty()
                && self.profile.len() <= 128
                && !self.profile.chars().any(char::is_control),
            "invalid profile name"
        );
        Ok(())
    }
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        ensure!(
            std::fs::metadata(path)?.len() <= 16384,
            "agent job is too large"
        );
        let text = std::fs::read_to_string(path)?;
        let job: Self = toml::from_str(&text).map_err(|_| anyhow::anyhow!("invalid agent job"))?;
        job.validate()?;
        Ok(job)
    }
    pub fn check_profile(&self) -> anyhow::Result<()> {
        let cfg = crate::config::Config::load(Some(&self.config))?;
        let profile = cfg
            .profiles
            .get(&self.profile)
            .context("agent profile not found")?;
        ensure!(!profile.room.is_empty(), "agent profile has no room");
        ensure!(
            matches!(profile.mode.as_str(), "lan" | "dev" | "game"),
            "unsupported agent profile mode"
        );
        Ok(())
    }
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        use std::io::Write;
        self.validate()?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        ensure!(parent.is_dir(), "agent job directory must already exist");
        let temporary = parent.join(format!(".frpsh-job-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut options = std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(toml::to_string(self)?.as_bytes())?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, path)?;
            Ok::<(), anyhow::Error>(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }
}
