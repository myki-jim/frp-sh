//! STUN（RFC 5389）消息编解码 + 公网地址学习（Binding 客户端）。
//!
//! frp-sh 用 STUN 做两件事：
//! 1. 公网地址学习：客户端从打洞 socket 发 Binding 请求，服务器回包报出
//!    NAT 映射后的 IP:port（替代自建 UDP echo，`stun.cloudflare.com:3478`
//!    免费且不限量，自建 TURN/coturn 也内置 STUN）；
//! 2. TURN 中继的基础：TURN（RFC 5766）本身就是建立在 STUN 消息格式上的。
//!
//! 本模块只实现需要的子集：消息头、常用 attribute 编解码、Binding 请求/
//! 响应解析、MESSAGE-INTEGRITY（HMAC-SHA1）计算（供 TURN 认证复用）。

use bytes::{BufMut, BytesMut};
use md5::Md5;
use rand::Rng;
use sha1::Sha1;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// STUN 魔法 Cookie。
pub const MAGIC_COOKIE: u32 = 0x2112_A442;

// 消息类型（方法 | 类别位）。
pub const METHOD_BINDING: u16 = 0x001;
pub const METHOD_ALLOCATE: u16 = 0x003;
pub const METHOD_REFRESH: u16 = 0x004;
pub const METHOD_SEND: u16 = 0x006;
pub const METHOD_DATA: u16 = 0x007;
pub const METHOD_CREATE_PERMISSION: u16 = 0x008;

pub const CLASS_REQUEST: u16 = 0;
pub const CLASS_INDICATION: u16 = 1;
pub const CLASS_SUCCESS: u16 = 2;
pub const CLASS_ERROR: u16 = 3;

// 常用 attribute 类型。
pub const ATTR_MAPPED_ADDRESS: u16 = 0x0001;
pub const ATTR_USERNAME: u16 = 0x0006;
pub const ATTR_MESSAGE_INTEGRITY: u16 = 0x0008;
pub const ATTR_ERROR_CODE: u16 = 0x0009;
pub const ATTR_UNKNOWN_ATTRIBUTES: u16 = 0x000A;
pub const ATTR_REALM: u16 = 0x0014;
pub const ATTR_NONCE: u16 = 0x0015;
pub const ATTR_XOR_MAPPED_ADDRESS: u16 = 0x0020;
/// Allocate 响应中的 relay 传输地址（TURN RFC 5766）
pub const ATTR_XOR_RELAYED_ADDRESS: u16 = 0x0016;
pub const ATTR_REQUESTED_TRANSPORT: u16 = 0x0019;
pub const ATTR_XOR_PEER_ADDRESS: u16 = 0x0012;
pub const ATTR_LIFETIME: u16 = 0x000D;
pub const ATTR_DATA: u16 = 0x0013;
pub const ATTR_SOFTWARE: u16 = 0x8022;

fn msg_type(method: u16, class: u16) -> u16 {
    // 方法：低 5 位 + 高 2 位（bit5-6 → bit9-10 区域：`(method & 0x0070) << 1`）
    let m = (method & 0x000F) | ((method & 0x0070) << 1);
    // 类别：bit0 → bit4，bit1 → bit8
    m | ((class & 0x1) << 4) | ((class & 0x2) << 7)
}

fn decode_class(t: u16) -> u16 {
    ((t >> 4) & 0x1) | ((t >> 7) & 0x2)
}

fn decode_method(t: u16) -> u16 {
    (t & 0x000F) | ((t >> 1) & 0x0070)
}

/// 一个解码后的 STUN 消息。
#[derive(Debug, Clone)]
pub struct Message {
    pub method: u16,
    pub class: u16,
    pub txid: [u8; 12],
    pub attrs: Vec<(u16, Vec<u8>)>,
}

impl Message {
    pub fn get(&self, t: u16) -> Option<&[u8]> {
        self.attrs
            .iter()
            .find(|(at, _)| *at == t)
            .map(|(_, v)| v.as_slice())
    }

    pub fn get_str(&self, t: u16) -> Option<String> {
        self.get(t).map(|v| String::from_utf8_lossy(v).into_owned())
    }
}

/// 生成一个随机事务 ID。
pub fn new_txid() -> [u8; 12] {
    let mut txid = [0u8; 12];
    rand::thread_rng().fill(&mut txid);
    txid
}

/// 构建 STUN 消息（不自动加 MESSAGE-INTEGRITY）。
pub fn build(method: u16, class: u16, txid: &[u8; 12], attrs: &[(u16, &[u8])]) -> BytesMut {
    let mut body = BytesMut::new();
    for (t, v) in attrs {
        body.put_u16(*t);
        body.put_u16(v.len() as u16);
        body.put_slice(v);
        let pad = (4 - (v.len() % 4)) % 4;
        body.put_bytes(0, pad);
    }
    let mut out = BytesMut::with_capacity(20 + body.len());
    out.put_u16(msg_type(method, class));
    out.put_u16(body.len() as u16);
    out.put_u32(MAGIC_COOKIE);
    out.put_slice(txid);
    out.put_slice(&body);
    out
}

/// 构建一个携带 MESSAGE-INTEGRITY 的消息。
///
/// `key` 为认证密钥（long-term credential 下是 `MD5(user:realm:pass)`）。
/// 按 libstun/coturn 的实现（实测对照）：
/// - HMAC 输入 = 消息头 + 全部前置 attribute（**不含** MI 的 type/length）；
/// - 但消息头里的 **length 字段必须是"包含 MI 之后"的值**
///   （attrs + 24，不含 FINGERPRINT）——因为客户端在写入 MI 后 length 字段
///   已更新，服务器按收到的 length 计算。
pub fn build_with_integrity(
    method: u16,
    class: u16,
    txid: &[u8; 12],
    attrs: &[(u16, &[u8])],
    key: &[u8],
) -> BytesMut {
    use hmac::{Hmac, Mac};
    type HmacSha1 = Hmac<Sha1>;

    // 头部 + attributes
    let mut out = build(method, class, txid, attrs);
    // length 字段 = attrs 长度 + MI 的 24 字节（type+len+value）
    let body_len = (out.len() - 20) as u16 + 24;
    out[2..4].copy_from_slice(&body_len.to_be_bytes());
    // HMAC 输入 = 头（length 含 MI）+ attrs（不含 MI）
    let mut mac = HmacSha1::new_from_slice(key).expect("hmac key");
    mac.update(&out);
    let digest = mac.finalize().into_bytes();
    out.put_u16(ATTR_MESSAGE_INTEGRITY);
    out.put_u16(20);
    out.put_slice(&digest);
    out
}

/// 解析一个 STUN 消息。
pub fn parse(buf: &[u8]) -> Option<Message> {
    if buf.len() < 20 || u32::from_be_bytes(buf[4..8].try_into().ok()?) != MAGIC_COOKIE {
        return None;
    }
    let t = u16::from_be_bytes(buf[0..2].try_into().ok()?);
    let len = u16::from_be_bytes(buf[2..4].try_into().ok()?) as usize;
    if buf.len() < 20 + len {
        return None;
    }
    let mut txid = [0u8; 12];
    txid.copy_from_slice(&buf[8..20]);
    let mut attrs = Vec::new();
    let mut off = 20;
    while off + 4 <= 20 + len {
        let at = u16::from_be_bytes(buf[off..off + 2].try_into().ok()?);
        let alen = u16::from_be_bytes(buf[off + 2..off + 4].try_into().ok()?) as usize;
        let start = off + 4;
        if start + alen > 20 + len {
            return None;
        }
        attrs.push((at, buf[start..start + alen].to_vec()));
        off = start + alen + (4 - (alen % 4)) % 4;
    }
    Some(Message {
        method: decode_method(t),
        class: decode_class(t),
        txid,
        attrs,
    })
}

/// 编码 XOR-MAPPED-ADDRESS / XOR-PEER-ADDRESS 的值（family + 异或后的 port/ip）。
pub fn encode_xor_addr(addr: SocketAddr, txid: &[u8; 12]) -> Vec<u8> {
    let mut v = Vec::with_capacity(20);
    v.push(0); // 保留
    match addr {
        SocketAddr::V4(a) => {
            v.push(0x01);
            let port = u16::from_be_bytes(a.port().to_be_bytes()) ^ ((MAGIC_COOKIE >> 16) as u16);
            v.extend_from_slice(&port.to_be_bytes());
            let ip = u32::from(*a.ip()) ^ MAGIC_COOKIE;
            v.extend_from_slice(&ip.to_be_bytes());
        }
        SocketAddr::V6(a) => {
            v.push(0x02);
            let port = u16::from_be_bytes(a.port().to_be_bytes()) ^ ((MAGIC_COOKIE >> 16) as u16);
            v.extend_from_slice(&port.to_be_bytes());
            let mut ip = a.ip().octets();
            let mask: [u8; 16] = {
                let mut m = [0u8; 16];
                m[..4].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
                m[4..].copy_from_slice(txid); // 完整 12 字节事务 ID
                m
            };
            for (b, k) in ip.iter_mut().zip(mask.iter()) {
                *b ^= k;
            }
            v.extend_from_slice(&ip);
        }
    }
    v
}

/// 解码 XOR-MAPPED-ADDRESS / XOR-PEER-ADDRESS。
pub fn decode_xor_addr(v: &[u8], txid: &[u8; 12]) -> Option<SocketAddr> {
    if v.len() < 4 {
        return None;
    }
    let family = v[1];
    let port = u16::from_be_bytes([v[2], v[3]]) ^ ((MAGIC_COOKIE >> 16) as u16);
    match family {
        0x01 => {
            if v.len() < 8 {
                return None;
            }
            let ip = u32::from_be_bytes(v[4..8].try_into().ok()?) ^ MAGIC_COOKIE;
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), port))
        }
        0x02 => {
            if v.len() < 20 {
                return None;
            }
            let mask: [u8; 16] = {
                let mut m = [0u8; 16];
                m[..4].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
                m[4..].copy_from_slice(txid); // 完整 12 字节事务 ID
                m
            };
            let mut ip = [0u8; 16];
            for (i, b) in v[4..20].iter().enumerate() {
                ip[i] = b ^ mask[i];
            }
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(ip)), port))
        }
        _ => None,
    }
}

/// long-term credential 密钥：`MD5(user ":" realm ":" password)`。
pub fn long_term_key(user: &str, realm: &str, password: &str) -> [u8; 16] {
    use md5::Digest;
    let mut h = Md5::new();
    h.update(user.as_bytes());
    h.update(b":");
    h.update(realm.as_bytes());
    h.update(b":");
    h.update(password.as_bytes());
    h.finalize().into()
}

/// 解析 ERROR-CODE attribute（返回 (code, reason)）。
pub fn decode_error_code(v: &[u8]) -> Option<(u16, String)> {
    if v.len() < 4 {
        return None;
    }
    let code = u16::from(v[2]) * 100 + u16::from(v[3]);
    let reason = String::from_utf8_lossy(&v[4..]).into_owned();
    Some((code, reason))
}

/// 编码 REQUESTED-TRANSPORT（UDP=17）。
pub fn encode_requested_transport() -> Vec<u8> {
    vec![17, 0, 0, 0]
}

/// 编码 LIFETIME。
pub fn encode_lifetime(secs: u32) -> Vec<u8> {
    secs.to_be_bytes().to_vec()
}

/// 向 `server` 发送 STUN Binding 请求并解析 XOR-MAPPED-ADDRESS。
///
/// 用打洞 socket 发出（NAT 映射与打洞一致），超时后返回错误。
pub async fn binding_probe(
    socket: &tokio::net::UdpSocket,
    server: SocketAddr,
) -> crate::error::Result<SocketAddr> {
    let txid = new_txid();
    let req = build(METHOD_BINDING, CLASS_REQUEST, &txid, &[]);
    socket
        .send_to(&req, server)
        .await
        .map_err(crate::error::FrpError::Io)?;
    let mut buf = [0u8; 1024];
    let deadline = std::time::Duration::from_secs(3);
    for _ in 0..30 {
        match tokio::time::timeout(deadline / 30, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, _))) => {
                if let Some(msg) = parse(&buf[..n]) {
                    if msg.txid != txid {
                        continue; // 其他响应（如打洞回声），忽略
                    }
                    if msg.class == CLASS_ERROR {
                        let code = msg
                            .get(ATTR_ERROR_CODE)
                            .and_then(decode_error_code)
                            .map(|(c, _)| c)
                            .unwrap_or(0);
                        return Err(crate::error::FrpError::Relay(format!(
                            "STUN binding error {code}"
                        )));
                    }
                    if let Some(v) = msg.get(ATTR_XOR_MAPPED_ADDRESS) {
                        if let Some(addr) = decode_xor_addr(v, &txid) {
                            return Ok(addr);
                        }
                    }
                    if let Some(v) = msg.get(ATTR_MAPPED_ADDRESS) {
                        // 旧式服务器可能回 MAPPED-ADDRESS（无 XOR）
                        if let Some(addr) = decode_plain_addr(v) {
                            return Ok(addr);
                        }
                    }
                }
            }
            Ok(Err(_)) => continue,
            Err(_) => {
                // 超时：重发一次请求
                let _ = socket.send_to(&req, server).await;
            }
        }
    }
    Err(crate::error::FrpError::Relay(
        "STUN binding timed out".into(),
    ))
}

/// 解码 MAPPED-ADDRESS（无 XOR 的旧式 attribute）。
fn decode_plain_addr(v: &[u8]) -> Option<SocketAddr> {
    if v.len() < 4 {
        return None;
    }
    let family = v[1];
    let port = u16::from_be_bytes([v[2], v[3]]);
    match family {
        0x01 => {
            if v.len() < 8 {
                return None;
            }
            Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(v[4], v[5], v[6], v[7])),
                port,
            ))
        }
        0x02 => {
            if v.len() < 20 {
                return None;
            }
            let mut ip = [0u8; 16];
            ip.copy_from_slice(&v[4..20]);
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(ip)), port))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msg_type_roundtrip() {
        for method in [
            METHOD_BINDING,
            METHOD_ALLOCATE,
            METHOD_CREATE_PERMISSION,
            METHOD_SEND,
        ] {
            for class in [CLASS_REQUEST, CLASS_SUCCESS, CLASS_ERROR] {
                let t = msg_type(method, class);
                assert_eq!(decode_method(t), method);
                assert_eq!(decode_class(t), class);
            }
        }
    }

    #[test]
    fn build_parse_roundtrip() {
        let txid = new_txid();
        let req = build(METHOD_BINDING, CLASS_REQUEST, &txid, &[]);
        let m = parse(&req).unwrap();
        assert_eq!(m.method, METHOD_BINDING);
        assert_eq!(m.class, CLASS_REQUEST);
        assert_eq!(m.txid, txid);
        assert!(m.attrs.is_empty());
    }

    #[test]
    fn xor_addr_roundtrip_v4_v6() {
        let txid = new_txid();
        for addr in [
            "1.2.3.4:1234".parse().unwrap(),
            "203.0.113.5:3478".parse().unwrap(),
            "[2001:db8::1]:5349".parse().unwrap(),
        ] {
            let enc = encode_xor_addr(addr, &txid);
            let dec = decode_xor_addr(&enc, &txid).unwrap();
            assert_eq!(dec, addr);
        }
    }

    #[test]
    fn message_integrity_matches_reference() {
        let key = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef,
        ];
        let txid = new_txid();
        let m = build_with_integrity(METHOD_BINDING, CLASS_REQUEST, &txid, &[], &key);
        // 末 20 字节为 HMAC-SHA1；HMAC 输入 = 头（length 含 MI）+ attributes（不含 MI）
        use hmac::{Hmac, Mac};
        type HmacSha1 = Hmac<Sha1>;
        let mut mac = HmacSha1::new_from_slice(&key).unwrap();
        mac.update(&m[..m.len() - 20 - 4]);
        assert_eq!(&mac.finalize().into_bytes()[..], &m[m.len() - 20..]);
        // length 字段 = 4（MI 完整 24 字节）
        assert_eq!(u16::from_be_bytes([m[2], m[3]]), 24);
    }

    #[test]
    fn long_term_key_format() {
        // RFC 5389：key = MD5(user ":" realm ":" password)
        let k = long_term_key("user", "example.org", "secret");
        let expect = hex::decode("a9832fed4a7567b40e43443f9f30c272").unwrap();
        assert_eq!(&k[..], &expect[..]);
    }
}
