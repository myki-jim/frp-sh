//! 通用辅助函数：随机数、房间号、时间戳、本机局域网信息等。

use rand::Rng;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// 生成 `n` 字节随机数并编码为十六进制字符串（2n 位）。
pub fn random_hex(n: usize) -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..n).map(|_| rng.gen_range(0..=255)).collect();
    hex::encode(bytes)
}

/// 生成打洞会话 token（8 位十六进制）。
pub fn rand_token() -> String {
    random_hex(4)
}

/// 生成完整房间号：`<prefix>-<6位hex>`，如 `game-a3f9c2`。
pub fn new_room_id(prefix: &str) -> String {
    format!("{}-{}", sanitize_prefix(prefix), random_hex(3))
}

/// 清理房间前缀：仅保留小写字母数字与 `-_`，最长 16 字符。
pub fn sanitize_prefix(prefix: &str) -> String {
    let clean: String = prefix
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(16)
        .collect::<String>()
        .to_lowercase();
    if clean.is_empty() {
        "game".to_string()
    } else {
        clean
    }
}

/// 校验房间号格式（`prefix-hex6`）。
pub fn validate_room_id(room_id: &str) -> bool {
    match room_id.rsplit_once('-') {
        Some((prefix, suffix)) => {
            !prefix.is_empty() && suffix.len() == 6 && suffix.chars().all(|c| c.is_ascii_hexdigit())
        }
        None => false,
    }
}

/// 当前 Unix 时间戳（秒）。
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 生成本机设备 UUID（v4）。
pub fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 由设备 UUID 确定性派生虚拟网卡 IP（10.66.0.2 ~ 10.66.0.254）。
///
/// 同一设备每次派生结果一致 → 稳定的虚拟 IP，朋友可长期使用该 IP 直连。
pub fn derive_vnet_ip(uuid: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8492_2225;
    for b in uuid.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    let host = 2 + (h % 253);
    format!("10.66.0.{host}")
}

/// 本机一个局域网接口（IPv4）。
#[derive(Debug, Clone)]
pub struct LanIface {
    pub name: String,
    pub ip: IpAddr,
    /// CIDR 前缀长度（如 /24）。
    pub prefix: u8,
}

/// 是否为 frp-sh 虚拟网段地址（10.66.0.0/24）。
fn is_vnet_ip(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10 && o[1] == 66 && o[2] == 0
}

/// 枚举本机局域网 IPv4 接口：排除回环、链路本地与 frp-sh 虚拟网卡。
pub fn local_lan_interfaces() -> Vec<LanIface> {
    let Ok(addrs) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    addrs
        .into_iter()
        .filter(|a| !a.is_loopback() && !a.is_link_local())
        .filter_map(|a| match a.addr {
            if_addrs::IfAddr::V4(v4) if !is_vnet_ip(v4.ip) => Some(LanIface {
                name: a.name,
                ip: IpAddr::V4(v4.ip),
                prefix: v4.prefixlen,
            }),
            _ => None,
        })
        .collect()
}

/// 本机局域网打洞地址列表：`(局域网IP, 端口)`，端口与打洞 socket 一致。
pub fn lan_socket_addrs(port: u16) -> Vec<SocketAddr> {
    local_lan_interfaces()
        .into_iter()
        .map(|i| SocketAddr::new(i.ip, port))
        .collect()
}

/// 由前缀长度计算 IPv4 掩码。
pub fn mask_from_prefix(prefix: u8) -> u32 {
    if prefix >= 32 {
        u32::MAX
    } else if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    }
}

/// 由 (IP, 前缀) 计算网络地址。
pub fn network_addr(ip: IpAddr, prefix: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => IpAddr::V4(Ipv4Addr::from(u32::from(v4) & mask_from_prefix(prefix))),
        IpAddr::V6(_) => ip,
    }
}

/// 本机局域网子网 CIDR 列表（如 `192.168.1.0/24`），供 TUN 模式通告/加路由。
pub fn lan_subnet_cidrs() -> Vec<String> {
    local_lan_interfaces()
        .iter()
        .map(|i| format!("{}/{}", network_addr(i.ip, i.prefix), i.prefix))
        .collect()
}

/// 解析 `a.b.c.d/prefix` → (网络地址, 前缀)。
pub fn parse_cidr(cidr: &str) -> Option<(u32, u8)> {
    let (ip, pfx) = cidr.split_once('/')?;
    let pfx: u8 = pfx.parse().ok()?;
    let ip: Ipv4Addr = ip.parse().ok()?;
    Some((u32::from(ip), pfx))
}

/// 由 (IP, 子网掩码) 计算 CIDR（如 `10.66.0.18/255.255.255.0` → `10.66.0.0/24`）。
pub fn cidr_from_ip_netmask(ip: &str, netmask: &str) -> Option<String> {
    let ip: Ipv4Addr = ip.parse().ok()?;
    let mask: Ipv4Addr = netmask.parse().ok()?;
    let mask_u = u32::from(mask);
    let prefix = mask_u.count_ones() as u8;
    let net = u32::from(ip) & mask_u;
    Some(format!("{}/{}", Ipv4Addr::from(net), prefix))
}

/// 判断两个 CIDR 子网是否重叠。
pub fn cidrs_overlap(a: &str, b: &str) -> bool {
    let Some((na, pa)) = parse_cidr(a) else {
        return false;
    };
    let Some((nb, pb)) = parse_cidr(b) else {
        return false;
    };
    let m = mask_from_prefix(pa.min(pb));
    (na & m) == (nb & m)
}

/// 该 IP 是否属于本机局域网（回环 / 任一接口同子网）。
pub fn is_local_ip(ip: IpAddr) -> bool {
    if ip.is_loopback() {
        return true;
    }
    let IpAddr::V4(v4) = ip else {
        return false;
    };
    if v4.is_link_local() {
        return true;
    }
    let u = u32::from(v4);
    local_lan_interfaces().iter().any(|i| {
        let IpAddr::V4(iv) = i.ip else {
            return false;
        };
        (u & mask_from_prefix(i.prefix)) == (u32::from(iv) & mask_from_prefix(i.prefix))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vnet_ip_stable_and_in_range() {
        let u1 = "123e4567-e89b-12d3-a456-426614174000";
        let u2 = "223e4567-e89b-12d3-a456-426614174000";
        assert_eq!(derive_vnet_ip(u1), derive_vnet_ip(u1));
        let ip = derive_vnet_ip(u1);
        let octet: u32 = ip.rsplit('.').next().unwrap().parse().unwrap();
        assert!((2..=254).contains(&octet));
        assert_ne!(derive_vnet_ip(u1), derive_vnet_ip(u2));
    }

    #[test]
    fn room_id_format() {
        let id = new_room_id("Game");
        assert!(id.starts_with("game-"));
        assert!(validate_room_id(&id));
        assert!(!validate_room_id("nope"));
        assert!(!validate_room_id("game-xyz123"));
    }

    #[test]
    fn token_length() {
        assert_eq!(rand_token().len(), 8);
    }

    #[test]
    fn cidr_helpers() {
        assert_eq!(mask_from_prefix(24), 0xffff_ff00);
        assert_eq!(mask_from_prefix(0), 0);
        assert_eq!(mask_from_prefix(32), u32::MAX);
        assert_eq!(
            network_addr("192.168.1.77".parse().unwrap(), 24).to_string(),
            "192.168.1.0"
        );
        assert!(cidrs_overlap("192.168.1.0/24", "192.168.1.128/25"));
        assert!(cidrs_overlap("10.66.0.0/24", "10.66.0.5/32"));
        assert!(!cidrs_overlap("192.168.1.0/24", "192.168.2.0/24"));
        assert!(!cidrs_overlap("10.66.0.0/24", "10.66.1.0/24"));
    }

    #[test]
    fn loopback_is_local() {
        assert!(is_local_ip("127.0.0.1".parse().unwrap()));
    }
}
