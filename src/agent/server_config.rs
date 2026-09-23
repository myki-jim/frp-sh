//! Declarative server settings; never an arbitrary executable or shell command.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub addr: String,
    pub relay_addr: String,
    pub udp_addr: Option<String>,
    pub turn: Option<String>,
    pub external_ip: Option<std::net::IpAddr>,
    pub max_rooms: u32,
    pub max_members: u32,
    pub max_total_members: u32,
    pub spaces_db: Option<std::path::PathBuf>,
    pub spaces_origin: Option<String>,
    pub invite_ttl: u64,
    pub invite_max_ttl: u64,
    pub domains_db: Option<std::path::PathBuf>,
    pub domains_origin: Option<String>,
    pub ingress_addr: Option<String>,
    pub ingress_cname: Option<String>,
    pub ingress_https_port: u16,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            addr: "0.0.0.0:8080".into(),
            relay_addr: "0.0.0.0:8081".into(),
            udp_addr: None,
            turn: None,
            external_ip: None,
            max_rooms: 1024,
            max_members: 33,
            max_total_members: 33792,
            spaces_db: None,
            spaces_origin: None,
            invite_ttl: 900,
            invite_max_ttl: 86400,
            domains_db: None,
            domains_origin: None,
            ingress_addr: None,
            ingress_cname: None,
            ingress_https_port: 443,
        }
    }
}
impl Settings {
    pub fn validate(&self, password: Option<&str>) -> anyhow::Result<()> {
        let http: std::net::SocketAddr = self.addr.parse()?;
        let relay: std::net::SocketAddr = self.relay_addr.parse()?;
        if let Some(addr) = &self.udp_addr {
            addr.parse::<std::net::SocketAddr>()?;
        }
        if let Some(addr) = &self.turn {
            addr.parse::<std::net::SocketAddr>()?;
        }
        anyhow::ensure!(
            password.is_some_and(|p| !p.trim().is_empty())
                || (http.ip().is_loopback() && relay.ip().is_loopback()),
            "public listeners require a server password"
        );
        anyhow::ensure!(
            (1..=1024).contains(&self.max_rooms)
                && (2..=33).contains(&self.max_members)
                && (1..=33792).contains(&self.max_total_members),
            "invalid server capacity"
        );
        anyhow::ensure!(
            self.invite_ttl > 0
                && self.invite_ttl <= self.invite_max_ttl
                && self.invite_max_ttl <= 86400,
            "invalid invitation lifetime"
        );
        if let Some(path) = &self.spaces_db {
            anyhow::ensure!(
                path.is_absolute(),
                "server job database path must be absolute"
            );
            crate::invite_ticket::Ticket::new(
                self.spaces_origin
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("space origin required"))?,
                &"00".repeat(32),
            )?;
            anyhow::ensure!(
                password.is_some_and(|p| !p.trim().is_empty()),
                "space creation requires a server password"
            );
        }
        if let Some(path) = &self.domains_db {
            anyhow::ensure!(path.is_absolute(), "domain database path must be absolute");
            self.ingress_addr
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("ingress address required"))?
                .parse::<std::net::SocketAddr>()?;
            let origin = self
                .domains_origin
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("domains origin required"))?;
            let parsed = reqwest::Url::parse(origin)?;
            anyhow::ensure!(
                matches!(parsed.scheme(), "http" | "https"),
                "invalid domains origin"
            );
            crate::domains::normalize_domain(
                self.ingress_cname
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("ingress CNAME required"))?,
            )?;
            anyhow::ensure!(self.ingress_https_port > 0, "invalid ingress HTTPS port");
        }
        Ok(())
    }
}
#[cfg(feature = "server")]
pub async fn run(path: &std::path::Path) -> anyhow::Result<()> {
    let cfg = crate::config::Config::load(Some(path))?;
    let settings = cfg.server.unwrap_or_default();
    settings.validate(cfg.password.as_deref())?;
    if let Some(secret) = &cfg.password {
        crate::debuglog::protect(secret);
    }
    crate::commands::acquire_role_lock("serve")?;
    crate::commands::run_serve(
        settings.addr,
        settings.relay_addr,
        settings.udp_addr,
        cfg.password,
        settings.turn,
        settings.external_ip,
        crate::signaling::limits::ServerLimits {
            max_rooms: settings.max_rooms,
            max_members: settings.max_members,
            max_total_members: settings.max_total_members,
        },
        crate::spaces::server::Options {
            spaces_db: settings.spaces_db,
            spaces_origin: settings.spaces_origin,
            invite_ttl: settings.invite_ttl,
            invite_max_ttl: settings.invite_max_ttl,
        },
        crate::domains::server::Options {
            domains_db: settings.domains_db,
            domains_origin: settings.domains_origin,
            ingress_addr: settings.ingress_addr,
            ingress_cname: settings.ingress_cname,
            ingress_https_port: settings.ingress_https_port,
        },
    )
    .await
}
