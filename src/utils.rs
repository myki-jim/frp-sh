//! 通用辅助函数：随机数、房间号、时间戳等。

use rand::Rng;

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
}
