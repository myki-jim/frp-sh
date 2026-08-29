//! 配置加载（TOML）。

use serde::{Deserialize, Serialize};
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

fn default_signaling() -> String {
    "http://127.0.0.1:8080".to_string()
}

fn default_relay() -> String {
    "127.0.0.1:8081".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 信令服务器 HTTP 基地址（REST API）。
    #[serde(default = "default_signaling")]
    pub signaling_addr: String,
    /// 信令服务器 TCP 中继地址。
    #[serde(default = "default_relay")]
    pub relay_addr: String,
    /// 可选：UDP 公网探测地址（默认与 signaling_addr 同端口；
    /// 某些云防火墙 TCP/UDP 端口需要分开时设置此项）。
    #[serde(default)]
    pub signaling_udp: Option<String>,
    /// 设备身份 UUID（首次运行生成，用于派生稳定虚拟网卡 IP 等）。
    #[serde(default)]
    pub uuid: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            signaling_addr: default_signaling(),
            relay_addr: default_relay(),
            signaling_udp: None,
            uuid: None,
        }
    }
}

impl Config {
    /// 默认配置文件路径（按平台）：
    /// - Windows：`%APPDATA%\frp-sh\config.toml`
    /// - Linux/macOS：`$XDG_CONFIG_HOME/frp-sh/config.toml` 或 `~/.config/frp-sh/config.toml`
    pub fn default_path() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("frp-sh").join("config.toml"))
        }
        #[cfg(not(windows))]
        {
            if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
                Some(PathBuf::from(x).join("frp-sh").join("config.toml"))
            } else {
                std::env::var_os("HOME").map(|h| {
                    PathBuf::from(h)
                        .join(".config")
                        .join("frp-sh")
                        .join("config.toml")
                })
            }
        }
    }

    /// 是否存在默认配置文件。
    pub fn default_exists() -> bool {
        Self::default_path().map(|p| p.exists()).unwrap_or(false)
    }

    /// 从 TOML 文件加载；未提供路径时使用内置默认值（127.0.0.1:8080 / 8081）。
    pub fn load(path: Option<&Path>) -> anyhow::Result<Self> {
        match path {
            Some(p) => {
                let text = std::fs::read_to_string(p)
                    .map_err(|e| anyhow::anyhow!("cannot read config {}: {e}", p.display()))?;
                let cfg: Config = toml::from_str(&text)
                    .map_err(|e| anyhow::anyhow!("invalid config {}: {e}", p.display()))?;
                Ok(cfg)
            }
            None => Ok(Self::default()),
        }
    }

    /// 自动加载：显式路径 > 默认配置文件 > 内置默认值；并确保设备身份 UUID。
    pub fn load_auto(path: Option<&Path>) -> anyhow::Result<Self> {
        let mut cfg = if let Some(p) = path {
            Self::load(Some(p))?
        } else if let Some(dp) = Self::default_path() {
            if dp.exists() {
                Self::load(Some(&dp))?
            } else {
                Self::default()
            }
        } else {
            Self::default()
        };
        // 设备身份：UUID 存于独立文件（不随配置文件增删而丢失）
        cfg.uuid = Some(Self::ensure_identity());
        Ok(cfg)
    }

    /// 设备身份文件路径（与默认配置同目录，文件名为 `identity`）。
    pub fn identity_path() -> Option<PathBuf> {
        Self::default_path().and_then(|p| p.parent().map(|d| d.join("identity")))
    }

    /// 读取或创建设备 UUID。
    pub fn ensure_identity() -> String {
        if let Some(p) = Self::identity_path() {
            if let Ok(s) = std::fs::read_to_string(&p) {
                let s = s.trim();
                if !s.is_empty() {
                    return s.to_string();
                }
            }
            let id = crate::utils::new_uuid();
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&p, &id);
            return id;
        }
        crate::utils::new_uuid()
    }

    /// 保存到指定路径（自动创建父目录）。
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string(self).map_err(|e| io::Error::other(e.to_string()))?;
        std::fs::write(path, text)
    }

    /// 信令服务器主机名（用于推导中继地址等）。
    pub fn signaling_host(&self) -> anyhow::Result<String> {
        let url = reqwest::Url::parse(&self.signaling_addr)
            .map_err(|e| anyhow::anyhow!("bad signaling_addr: {e}"))?;
        let host = url
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("no host in {}", self.signaling_addr))?;
        Ok(host.to_string())
    }

    /// 返回 UDP 公网探测地址：优先 `signaling_udp`，否则由 HTTP 地址同端口推导。
    pub fn signaling_udp_addr(&self) -> anyhow::Result<SocketAddr> {
        let addr = match &self.signaling_udp {
            Some(u) => u.clone(),
            None => {
                let url = reqwest::Url::parse(&self.signaling_addr)
                    .map_err(|e| anyhow::anyhow!("bad signaling_addr: {e}"))?;
                let host = url
                    .host_str()
                    .ok_or_else(|| anyhow::anyhow!("no host in {}", self.signaling_addr))?;
                let port = url
                    .port_or_known_default()
                    .ok_or_else(|| anyhow::anyhow!("no port in {}", self.signaling_addr))?;
                if host.contains(':') {
                    format!("[{host}]:{port}")
                } else {
                    format!("{host}:{port}")
                }
            }
        };
        addr.parse()
            .map_err(|e| anyhow::anyhow!("bad udp addr {addr}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.signaling_addr, "http://127.0.0.1:8080");
        assert_eq!(
            cfg.signaling_udp_addr().unwrap().to_string(),
            "127.0.0.1:8080"
        );
        assert_eq!(cfg.relay_addr, "127.0.0.1:8081");
    }

    #[test]
    fn parse_toml() {
        let cfg: Config = toml::from_str("signaling_addr = \"http://1.2.3.4:9000\"\n").unwrap();
        assert_eq!(cfg.signaling_addr, "http://1.2.3.4:9000");
        assert_eq!(cfg.relay_addr, "127.0.0.1:8081"); // 缺省字段取默认
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("frpsh-test-{}", std::process::id()));
        let path = dir.join("config.toml");
        let cfg = Config {
            signaling_addr: "http://1.2.3.4:9000".into(),
            relay_addr: "1.2.3.4:9001".into(),
            signaling_udp: Some("1.2.3.4:9002".into()),
            uuid: Some("123e4567-e89b-12d3-a456-426614174000".into()),
        };
        cfg.save(&path).unwrap();
        let loaded = Config::load(Some(&path)).unwrap();
        assert_eq!(loaded.signaling_addr, "http://1.2.3.4:9000");
        assert_eq!(loaded.relay_addr, "1.2.3.4:9001");
        assert_eq!(loaded.signaling_udp, Some("1.2.3.4:9002".into()));
        assert_eq!(
            loaded.signaling_udp_addr().unwrap().to_string(),
            "1.2.3.4:9002"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_auto_falls_back_to_defaults() {
        // 隔离 APPDATA，避免读到真实用户配置导致测试不确定
        let tmp = std::env::temp_dir().join(format!("frpsh-test-appdata-{}", std::process::id()));
        std::env::set_var("APPDATA", &tmp);
        let cfg = Config::load_auto(None).unwrap();
        assert_eq!(cfg.signaling_addr, "http://127.0.0.1:8080");
        std::env::remove_var("APPDATA");
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
