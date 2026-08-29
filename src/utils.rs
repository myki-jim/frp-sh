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

#[cfg(test)]
mod tests {
    use super::*;

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
