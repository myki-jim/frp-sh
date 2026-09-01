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
    /// 可选：信令服务器密码（服务器 `serve --password` 设置后必须填写；
    /// 用于请求认证与中继流量加密）。
    #[serde(default)]
    pub password: Option<String>,
    /// 可选：STUN 服务器地址（如 `stun.cloudflare.com:3478`）。
    /// 设置后公网地址学习优先走 STUN（免费、不依赖自建 UDP 探测），失败回退自建 echo。
    #[serde(default)]
    pub stun_addr: Option<String>,
    /// 可选：TURN 供应商列表（`turn://user:pass@host:port` 或 `turn://host:port`）。
    /// 打洞失败时按序尝试，测速选最快者建立 TURN 中继数据面；全部失败回退私有 TCP 中继。
    #[serde(default)]
    pub turn_providers: Vec<String>,
    /// 可选：设备显示名（面板/拓扑显示；默认取主机名；房间内重名自动加 -2/-3 后缀）
    #[serde(default)]
    pub name: Option<String>,
    /// 连接配置档案（`frp-sh profile` 子命令管理；面板"一键接入"生成）。
    #[serde(default, skip_serializing_if = "profiles_empty")]
    pub profiles: std::collections::BTreeMap<String, Profile>,
}

fn profiles_empty(m: &std::collections::BTreeMap<String, Profile>) -> bool {
    m.is_empty()
}

/// 一条客户端连接配置：面板"一键接入"或 `frp-sh profile add` 写入，
/// `frp-sh profile run <name>` 按它启动会话。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    /// 档案名（默认 profile1、profile2 … 顺序分配；可 `profile edit --rename` 改名）
    pub name: String,
    /// 信令服务器 URL（如 http://101.43.41.195:8080）
    pub server: String,
    /// 要加入的房间号
    pub room: String,
    /// 服务器密码（面板不显示明文；CLI 列表输出同样打码）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// 连接模式：lan（虚拟网卡组网）| dev | game（端口转发）
    #[serde(default = "default_profile_mode")]
    pub mode: String,
    /// 对端显示的设备名（缺省取主机名）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    /// TCP 中继地址（缺省由 server 主机名 :8081 推导）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_addr: Option<String>,
    /// dev/game 模式的本地监听地址
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen: Option<String>,
    /// lan 模式：把本机局域网接入隧道
    #[serde(default)]
    pub expose_lan: bool,
    /// 默认档案（`frp-sh profile run` 不带名字时用它）
    #[serde(default)]
    pub default: bool,
}

fn default_profile_mode() -> String {
    "lan".to_string()
}

impl Profile {
    /// 密码打码显示（面板/列表）：只露前 2 后 2。
    pub fn masked_password(&self) -> String {
        match &self.password {
            Some(p) if p.len() > 8 => format!("{}••••{}", &p[..2], &p[p.len() - 2..]),
            Some(_) => "••••••".into(),
            None => "—".into(),
        }
    }
}

/// 设备显示名：`--name` / config `name` 优先，否则主机名。
pub fn device_name(configured: Option<&str>) -> String {
    if let Some(n) = CLI_NAME.get() {
        if !n.is_empty() {
            return n.clone();
        }
    }
    configured
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(hostname)
}

/// CLI `--name`（优先于 config）。
static CLI_NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// main 启动时注入 `--name`。
pub fn set_cli_name(name: Option<String>) {
    if let Some(n) = name {
        if !n.trim().is_empty() {
            let _ = CLI_NAME.set(n.trim().to_string());
        }
    }
}

/// 本机主机名（跨平台；失败回退 "device"）。
pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "device".into())
}

impl Default for Config {
    fn default() -> Self {
        Self {
            signaling_addr: default_signaling(),
            relay_addr: default_relay(),
            signaling_udp: None,
            uuid: None,
            password: None,
            stun_addr: None,
            turn_providers: Vec::new(),
            name: None,
            profiles: std::collections::BTreeMap::new(),
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

    /// 默认配置目录（存放 config.toml / identity / 更新检查缓存等）。
    pub fn default_dir() -> Option<PathBuf> {
        Self::default_path().and_then(|p| p.parent().map(|d| d.to_path_buf()))
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

    /// 保存到默认配置路径（无默认路径时报错）。
    pub fn save_default(&self) -> anyhow::Result<()> {
        let p = Self::default_path()
            .ok_or_else(|| anyhow::anyhow!("cannot determine config path"))?;
        self.save(&p).map_err(|e| anyhow::anyhow!("save config: {e}"))
    }

    /// 下一个空闲档案名：profile1、profile2 …
    pub fn next_profile_name(&self) -> String {
        let mut n = 1;
        loop {
            let name = format!("profile{n}");
            if !self.profiles.contains_key(&name) {
                return name;
            }
            n += 1;
        }
    }

    /// 取默认档案（无标记时取第一个）。
    pub fn default_profile(&self) -> Option<&Profile> {
        self.profiles
            .values()
            .find(|p| p.default)
            .or_else(|| self.profiles.values().next())
    }

    /// 把某档案设为默认（清掉其余标记）。
    pub fn mark_default_profile(&mut self, name: &str) {
        for (k, p) in self.profiles.iter_mut() {
            p.default = k == name;
        }
    }

    /// 由 server URL 推导中继地址（同主机 :8081）。
    pub fn derive_relay(server: &str) -> Option<String> {
        let url = reqwest::Url::parse(server).ok()?;
        let host = url.host_str()?;
        let port = url.port().unwrap_or(80);
        // 常见约定：HTTP 8080 → 中继 8081；其他端口 +1
        let relay_port = if port == 80 { 8081 } else { port + 1 };
        Some(format!("{host}:{relay_port}"))
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

    /// 可选 STUN 服务器地址（解析为 `SocketAddr`）。
    pub fn stun_addr_opt(&self) -> anyhow::Result<Option<SocketAddr>> {
        match &self.stun_addr {
            Some(s) => Ok(Some(
                s.parse()
                    .map_err(|e| anyhow::anyhow!("bad stun addr {s}: {e}"))?,
            )),
            None => Ok(None),
        }
    }

    /// 解析 TURN 供应商列表（`turn://user:pass@host:port`）。
    pub fn turn_credentials(&self) -> Vec<crate::p2p::turn::TurnCredentials> {
        self.turn_providers
            .iter()
            .filter_map(|s| parse_turn_url(s))
            .collect()
    }
}

/// 解析 `turn://[user:pass@]host:port`。
pub fn parse_turn_url(s: &str) -> Option<crate::p2p::turn::TurnCredentials> {
    let rest = s.strip_prefix("turn://")?;
    let (userinfo, hostport) = match rest.split_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, rest),
    };
    let server: SocketAddr = hostport.parse().ok()?;
    let (username, password) = match userinfo {
        Some(u) => match u.split_once(':') {
            Some((n, p)) => (n.to_string(), p.to_string()),
            None => (u.to_string(), String::new()),
        },
        None => (String::new(), String::new()),
    };
    Some(crate::p2p::turn::TurnCredentials {
        server,
        username,
        password,
    })
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
            password: Some("secret".into()),
            stun_addr: Some("stun.cloudflare.com:3478".into()),
            turn_providers: vec!["turn://frp-sh:secret@1.2.3.4:3478".into()],
            name: Some("test-box".into()),
            profiles: std::collections::BTreeMap::new(),
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
