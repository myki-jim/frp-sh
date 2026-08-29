//! 版本与协议兼容性。
//!
//! - `VERSION`：当前程序版本（来自 Cargo.toml）
//! - `PROTOCOL_VERSION`：线协议（信令 REST / 打洞 / 隧道帧）版本。
//!   每次出现**不向后兼容**的协议变更时递增；客户端与服务器协议版本不一致
//!   即视为版本冲突，拒绝运行（防止新旧混用导致无法解释的错误）。
//! - 语义化版本解析与比较（`major.minor.patch`），用于更新检查与代差判断。

/// 当前程序版本（`major.minor.patch`）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 线协议版本。向后兼容的新增（如 serde default 字段）不递增；
/// 破坏性变更（改路由、改帧格式、改语义）必须递增，并发布说明。
pub const PROTOCOL_VERSION: u32 = 1;

/// 解析 `major.minor.patch`（可含 `-pre` 后缀，忽略后缀）。
pub fn parse(version: &str) -> Option<(u32, u32, u32)> {
    let core = version.trim().split(['-', '+']).next()?;
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next()?.parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// 语义化比较：`a > b`。
pub fn is_newer(a: &str, b: &str) -> bool {
    match (parse(a), parse(b)) {
        (Some(x), Some(y)) => x > y,
        _ => false,
    }
}

/// 两个版本是否"代差过大"（不可跳过更新）。
///
/// 规则（保守）：
/// - 主版本不同（如 0.x → 1.x）：不兼容，代差过大
/// - 次版本相差 ≥ 2（如 0.1.x → 0.3.x）：可能跨过破坏性变更，代差过大
/// - 其余（同主版本且次版本差 ≤ 1）：视为平滑升级，可跳过
pub fn is_breaking_gap(current: &str, latest: &str) -> bool {
    let (Some(c), Some(l)) = (parse(current), parse(latest)) else {
        return true; // 解析失败按保守处理
    };
    if c.0 != l.0 {
        return true;
    }
    l.1.saturating_sub(c.1) >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_versions() {
        assert_eq!(parse("0.1.11"), Some((0, 1, 11)));
        assert_eq!(parse("1.2"), Some((1, 2, 0)));
        assert_eq!(parse("0.1.11-beta.1"), Some((0, 1, 11)));
        assert_eq!(parse("abc"), None);
    }

    #[test]
    fn compare_versions() {
        assert!(is_newer("0.1.12", "0.1.11"));
        assert!(is_newer("0.2.0", "0.1.99"));
        assert!(!is_newer("0.1.11", "0.1.12"));
        assert!(!is_newer("0.1.11", "0.1.11"));
    }

    #[test]
    fn breaking_gap() {
        assert!(!is_breaking_gap("0.1.10", "0.1.11"), "同 minor 平滑");
        assert!(!is_breaking_gap("0.1.11", "0.2.0"), "minor 差 1 可跳");
        assert!(is_breaking_gap("0.1.11", "0.3.0"), "minor 差 2 过大");
        assert!(is_breaking_gap("0.9.0", "1.0.0"), "主版本不同过大");
        assert!(is_breaking_gap("0.1.11", "garbage"), "解析失败保守");
    }
}
