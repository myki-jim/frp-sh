//! Authenticated UDP envelopes with session challenges and replay protection.
use super::enc::{proof, verify};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
pub struct DatagramSecurity {
    key: [u8; 32],
    local: [u8; 16],
    remote: Option<[u8; 16]>,
    tx: u64,
    high: Option<u64>,
    seen: u128,
}
pub enum Incoming {
    Data(Vec<u8>),
    Reply(Vec<u8>),
    Ignore,
}
impl DatagramSecurity {
    pub fn new(key: [u8; 32]) -> Self {
        Self {
            key,
            local: rand::random(),
            remote: None,
            tx: 0,
            high: None,
            seen: 0,
        }
    }
    pub fn ready(&self) -> bool {
        self.remote.is_some()
    }
    pub fn hello(&self, echo: [u8; 16]) -> Vec<u8> {
        let mut b = b"FRS2H".to_vec();
        b.extend(self.local);
        b.extend(echo);
        b.extend(proof(&self.key, b"frpsh-udp-v2-hello", &b, &[]));
        b
    }
    pub fn seal(&mut self, data: &[u8]) -> Option<Vec<u8>> {
        let remote = self.remote?;
        let seq = self.tx;
        self.tx = self.tx.checked_add(1)?;
        let mut nonce = [0; 24];
        nonce[..16].copy_from_slice(&self.local);
        nonce[16..].copy_from_slice(&seq.to_be_bytes());
        let cipher = XChaCha20Poly1305::new((&self.key).into());
        let ct = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: data,
                    aad: &remote,
                },
            )
            .ok()?;
        let mut b = b"FRS2E".to_vec();
        b.extend(nonce);
        b.extend(ct);
        Some(b)
    }
    pub fn receive(&mut self, b: &[u8]) -> Incoming {
        if b.starts_with(b"FRS2H") && b.len() == 69 {
            if !verify(&self.key, b"frpsh-udp-v2-hello", &b[..37], &[], &b[37..]) {
                return Incoming::Ignore;
            }
            let remote: [u8; 16] = b[5..21].try_into().unwrap();
            let echo: &[u8] = &b[21..37];
            if remote == self.local || self.remote.is_some_and(|r| r != remote) {
                return Incoming::Ignore;
            }
            if echo == [0; 16] {
                return Incoming::Reply(self.hello(remote));
            }
            if echo != self.local {
                return Incoming::Ignore;
            }
            if self.remote.is_none() {
                self.remote = Some(remote);
                return Incoming::Reply(self.hello(remote));
            }
            return Incoming::Ignore;
        }
        if !b.starts_with(b"FRS2E") || b.len() < 45 {
            return Incoming::Ignore;
        }
        let Some(remote) = self.remote else {
            return Incoming::Ignore;
        };
        if b[5..21] != remote {
            return Incoming::Ignore;
        }
        let seq = u64::from_be_bytes(b[21..29].try_into().unwrap());
        if let Some(high) = self.high {
            if seq <= high && (high - seq >= 128 || self.seen & (1u128 << (high - seq)) != 0) {
                return Incoming::Ignore;
            }
        }
        let cipher = XChaCha20Poly1305::new((&self.key).into());
        let Ok(pt) = cipher.decrypt(
            XNonce::from_slice(&b[5..29]),
            Payload {
                msg: &b[29..],
                aad: &self.local,
            },
        ) else {
            return Incoming::Ignore;
        };
        match self.high {
            None => {
                self.high = Some(seq);
                self.seen = 1;
            }
            Some(h) if seq > h => {
                self.seen = if seq - h >= 128 {
                    1
                } else {
                    (self.seen << (seq - h)) | 1
                };
                self.high = Some(seq);
            }
            Some(h) => self.seen |= 1u128 << (h - seq),
        }
        Incoming::Data(pt)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authentication_replay_and_session_binding() {
        let mut a = DatagramSecurity::new([7; 32]);
        let mut b = DatagramSecurity::new([7; 32]);
        let Incoming::Reply(h) = b.receive(&a.hello([0; 16])) else {
            panic!()
        };
        let Incoming::Reply(h) = a.receive(&h) else {
            panic!()
        };
        let _ = b.receive(&h);
        let p = a.seal(b"payload").unwrap();
        assert!(matches!(b.receive(&p),Incoming::Data(v) if v==b"payload"));
        assert!(matches!(b.receive(&p), Incoming::Ignore));
        let mut c = DatagramSecurity::new([7; 32]);
        assert!(matches!(c.receive(&p), Incoming::Ignore));
        let mut bad = a.seal(b"payload").unwrap();
        *bad.last_mut().unwrap() ^= 1;
        assert!(matches!(b.receive(&bad), Incoming::Ignore));
        let mut wrong = DatagramSecurity::new([8; 32]);
        assert!(matches!(wrong.receive(&a.hello([0; 16])), Incoming::Ignore));
    }
}
