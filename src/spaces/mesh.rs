//! Per-peer authenticated packets. The relay supplies routing only, never plaintext IP.
use super::types::Peer;
use crate::p2p::security::{DatagramSecurity, Incoming};
use crate::stats::{LinkEntry, StreamStats, KIND_RELAY};
use anyhow::ensure;
use std::{collections::HashMap, net::Ipv4Addr, sync::Arc, time::Instant};
pub struct Mesh {
    local: Ipv4Addr,
    peers: HashMap<u8, Link>,
}
struct Link {
    info: Peer,
    security: DatagramSecurity,
    stats: Arc<StreamStats>,
    ping: Option<([u8; 8], Instant)>,
    last_seen: Instant,
}
pub struct Received {
    pub reply: Option<Vec<u8>>,
    pub packet: Option<Vec<u8>>,
}
impl Mesh {
    pub fn new(local: Ipv4Addr) -> Self {
        Self {
            local,
            peers: HashMap::new(),
        }
    }
    pub fn refresh(&mut self, peers: Vec<Peer>) -> anyhow::Result<()> {
        ensure!(peers.len() <= 32, "too many space peers");
        let mut next = HashMap::new();
        for info in peers {
            crate::device::public_key(&info.device)?;
            uuid::Uuid::parse_str(&info.session_id)?;
            let address = info.address.octets();
            ensure!(
                address[..3] == [10, 66, 0]
                    && (1..=254).contains(&address[3])
                    && info.address != self.local
                    && !next.contains_key(&address[3]),
                "invalid peer address"
            );
            let old = self.peers.remove(&address[3]).filter(|p| {
                p.info.device == info.device
                    && p.info.session_id == info.session_id
                    && p.info.key == info.key
            });
            next.insert(
                address[3],
                old.unwrap_or_else(|| Link {
                    security: DatagramSecurity::new(info.key),
                    info,
                    stats: StreamStats::new(KIND_RELAY),
                    ping: None,
                    last_seen: Instant::now(),
                }),
            );
        }
        self.peers = next;
        self.publish();
        Ok(())
    }
    pub fn heartbeat(&mut self) -> Vec<Vec<u8>> {
        let mut result = Vec::new();
        for (&address, link) in &mut self.peers {
            let message = if link.security.ready() {
                let nonce: [u8; 8] = rand::random();
                link.ping = Some((nonce, Instant::now()));
                let mut ping = vec![1];
                ping.extend(nonce);
                link.security.seal(&ping)
            } else {
                Some(link.security.hello([0; 16]))
            };
            if let Some(message) = message {
                result.push(envelope(address, message));
            }
        }
        self.publish();
        result
    }
    pub fn packet(&mut self, packet: &[u8]) -> Vec<Vec<u8>> {
        let Some((source, destination)) = addresses(packet) else {
            return vec![];
        };
        if source != self.local || destination == self.local {
            return vec![];
        }
        let broadcast = destination == Ipv4Addr::BROADCAST
            || destination == Ipv4Addr::new(10, 66, 0, 255)
            || destination.is_multicast();
        let mut result = Vec::new();
        for (&address, link) in &mut self.peers {
            if !broadcast && destination != link.info.address {
                continue;
            }
            let mut plain = vec![0];
            plain.extend_from_slice(packet);
            if let Some(sealed) = link.security.seal(&plain) {
                link.stats.on_sent(packet.len());
                result.push(envelope(address, sealed));
            }
        }
        result
    }
    pub fn receive(&mut self, frame: &[u8]) -> Received {
        let mut result = Received {
            reply: None,
            packet: None,
        };
        let Some((&source, message)) = frame.split_first() else {
            return result;
        };
        let Some(link) = self.peers.get_mut(&source) else {
            return result;
        };
        match link.security.receive(message) {
            Incoming::Reply(reply) => result.reply = Some(envelope(source, reply)),
            Incoming::Data(plain) => {
                link.last_seen = Instant::now();
                match plain.first() {
                    Some(1) if plain.len() == 9 => {
                        let mut pong = plain;
                        pong[0] = 2;
                        result.reply = link.security.seal(&pong).map(|p| envelope(source, p));
                    }
                    Some(2) if plain.len() == 9 => {
                        if let Some((nonce, started)) = link.ping {
                            if plain[1..] == nonce {
                                link.stats.on_rtt(
                                    started.elapsed().as_micros().clamp(1, u32::MAX as u128) as u32,
                                );
                                link.ping = None;
                            }
                        }
                    }
                    Some(0) => {
                        let packet = &plain[1..];
                        if addresses(packet).is_some_and(|(src, dst)| {
                            src == link.info.address
                                && (dst == self.local
                                    || dst == Ipv4Addr::BROADCAST
                                    || dst == Ipv4Addr::new(10, 66, 0, 255)
                                    || dst.is_multicast())
                        }) {
                            link.stats.on_recv(packet.len());
                            result.packet = Some(packet.to_vec());
                        }
                    }
                    _ => {}
                }
            }
            Incoming::Ignore => {}
        }
        result
    }
    pub fn connected(&self) -> bool {
        self.peers
            .values()
            .any(|p| p.security.ready() && p.ping.is_none() && p.last_seen.elapsed().as_secs() < 15)
    }
    fn publish(&self) {
        crate::stats::set_links(
            self.peers
                .values()
                .filter(|p| p.security.ready())
                .map(|p| LinkEntry {
                    peer: p.info.device.clone(),
                    kind: "relay",
                    detail: p.info.address.to_string(),
                    stats: p.stats.clone(),
                })
                .collect(),
        );
    }
}
impl Drop for Mesh {
    fn drop(&mut self) {
        crate::stats::clear_links();
    }
}
fn envelope(destination: u8, message: Vec<u8>) -> Vec<u8> {
    let mut frame = Vec::with_capacity(message.len() + 1);
    frame.push(destination);
    frame.extend(message);
    frame
}
fn addresses(packet: &[u8]) -> Option<(Ipv4Addr, Ipv4Addr)> {
    if !(20..=1400).contains(&packet.len()) || packet[0] >> 4 != 4 {
        return None;
    }
    let header = usize::from(packet[0] & 15) * 4;
    if header < 20
        || header > packet.len()
        || usize::from(u16::from_be_bytes([packet[2], packet[3]])) != packet.len()
    {
        return None;
    }
    Some((
        Ipv4Addr::from(<[u8; 4]>::try_from(&packet[12..16]).ok()?),
        Ipv4Addr::from(<[u8; 4]>::try_from(&packet[16..20]).ok()?),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn peer(address: u8, seed: u8) -> Peer {
        let signing = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
        Peer {
            device: hex::encode(signing.verifying_key().to_bytes()),
            session_id: uuid::Uuid::new_v4().to_string(),
            address: Ipv4Addr::new(10, 66, 0, address),
            key: [seed; 32],
        }
    }
    fn packet(source: u8, destination: u8) -> Vec<u8> {
        let mut value = vec![0u8; 20];
        value[0] = 0x45;
        value[2..4].copy_from_slice(&(20u16).to_be_bytes());
        value[8] = 64;
        value[9] = 1;
        value[12..16].copy_from_slice(&[10, 66, 0, source]);
        value[16..20].copy_from_slice(&[10, 66, 0, destination]);
        value
    }
    fn source(frame: Vec<u8>, address: u8) -> Vec<u8> {
        [vec![address], frame[1..].to_vec()].concat()
    }
    #[test]
    fn encrypted_packets_are_bound_to_the_member_addresses() {
        let mut left = Mesh::new(Ipv4Addr::new(10, 66, 0, 1));
        let mut right = Mesh::new(Ipv4Addr::new(10, 66, 0, 2));
        let session = uuid::Uuid::new_v4().to_string();
        let a = Peer {
            session_id: session.clone(),
            ..peer(2, 7)
        };
        let b = Peer {
            session_id: session,
            ..peer(1, 7)
        };
        left.refresh(vec![a]).unwrap();
        right.refresh(vec![b]).unwrap();
        // Complete the encrypted handshake through the server's addressed envelope.
        let first = left.heartbeat().pop().unwrap();
        let second = right.receive(&source(first, 1)).reply.unwrap();
        let third = left.receive(&source(second, 2)).reply.unwrap();
        let _ = right.receive(&source(third, 1));
        let frame = left.packet(&packet(1, 2)).pop().unwrap();
        let delivered = right.receive(&source(frame, 1));
        assert_eq!(delivered.packet.unwrap(), packet(1, 2));
        let mut forged = packet(9, 2);
        // Ciphertext can be valid but source attribution still cannot be forged.
        forged[12..16].copy_from_slice(&[10, 66, 0, 9]);
        assert!(left.packet(&forged).is_empty());
    }
}
