//! Operator-controlled admission limits. Counts include the room owner.
#[derive(Clone, Copy, Debug, clap::Args, serde::Serialize)]
pub struct ServerLimits {
    /// Maximum registered, unexpired rooms (1-1024)
    #[arg(long, default_value_t = 1024, value_parser = clap::value_parser!(u32).range(1..=1024))]
    pub max_rooms: u32,
    /// Maximum registered devices per room, including its owner (2-33)
    #[arg(long, default_value_t = 33, value_parser = clap::value_parser!(u32).range(2..=33))]
    pub max_members: u32,
    /// Maximum registered memberships across all rooms, including owners (1-33792)
    #[arg(long, default_value_t = 33792, value_parser = clap::value_parser!(u32).range(1..=33792))]
    pub max_total_members: u32,
}

impl Default for ServerLimits {
    fn default() -> Self {
        Self {
            max_rooms: 1024,
            max_members: 33,
            max_total_members: 33792,
        }
    }
}

impl ServerLimits {
    pub fn validate(self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=1024).contains(&self.max_rooms),
            "max_rooms must be 1-1024"
        );
        anyhow::ensure!(
            (2..=33).contains(&self.max_members),
            "max_members must be 2-33 (including owner)"
        );
        anyhow::ensure!(
            (1..=33792).contains(&self.max_total_members),
            "max_total_members must be 1-33792"
        );
        Ok(())
    }
}
