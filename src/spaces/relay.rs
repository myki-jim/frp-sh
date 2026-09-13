//! Owner-independent relay. Routes only between authenticated members of one space.
use super::{
    leases::Lease,
    server::{ApiError, Service},
};
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, State, WebSocketUpgrade,
    },
    http::{HeaderMap, StatusCode},
    response::Response,
};
use bytes::Bytes;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

pub const MAX_FRAME: usize = 2048;
type Destination = (String, u8);
struct Peer {
    epoch: uuid::Uuid,
    cancelled: CancellationToken,
    tx: mpsc::Sender<Bytes>,
}
#[derive(Clone, Default)]
pub struct Hub(Arc<Mutex<HashMap<Destination, Peer>>>);
struct Guard {
    hub: Hub,
    key: Destination,
    epoch: uuid::Uuid,
    cancelled: CancellationToken,
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.cancelled.cancel();
        if let Ok(mut peers) = self.hub.0.lock() {
            if peers.get(&self.key).is_some_and(|p| p.epoch == self.epoch) {
                peers.remove(&self.key);
            }
        }
    }
}
impl Hub {
    fn register(&self, lease: &Lease) -> (Guard, mpsc::Receiver<Bytes>) {
        let key = (lease.space.clone(), lease.address.octets()[3]);
        let epoch = uuid::Uuid::new_v4();
        let cancelled = lease.cancelled.child_token();
        let (tx, rx) = mpsc::channel(8);
        let old = self.0.lock().unwrap().insert(
            key.clone(),
            Peer {
                epoch,
                cancelled: cancelled.clone(),
                tx,
            },
        );
        if let Some(old) = old {
            old.cancelled.cancel();
        }
        (
            Guard {
                hub: self.clone(),
                key,
                epoch,
                cancelled,
            },
            rx,
        )
    }
    fn forward(&self, source: &Guard, frame: Bytes) {
        if source.cancelled.is_cancelled() || !(2..=MAX_FRAME).contains(&frame.len()) {
            return;
        }
        let destination = frame[0];
        if destination == source.key.1 || destination == 0 || destination == 255 {
            return;
        }
        let peers = self.0.lock().unwrap();
        if !peers
            .get(&source.key)
            .is_some_and(|p| p.epoch == source.epoch)
        {
            return;
        }
        let Some(target) = peers.get(&(source.key.0.clone(), destination)) else {
            return;
        };
        if target.cancelled.is_cancelled() {
            return;
        }
        let mut forwarded = Vec::with_capacity(frame.len());
        // A client never chooses the source identity presented to the recipient.
        forwarded.push(source.key.1);
        forwarded.extend_from_slice(&frame[1..]);
        let _ = target.tx.try_send(forwarded.into());
    }
}
pub async fn upgrade(
    State(service): State<Service>,
    Path(space): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|s| s.len() == 64)
        .ok_or(ApiError(StatusCode::UNAUTHORIZED, "access_token_required"))?
        .to_owned();
    let lease = service
        .authenticate_session(space, token.clone())
        .await
        .map_err(|_| ApiError(StatusCode::UNAUTHORIZED, "access_denied"))?;
    Ok(ws
        .read_buffer_size(MAX_FRAME)
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_FRAME * 2)
        .max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| serve(service, lease, token, socket)))
}
async fn serve(service: Service, lease: Lease, token: String, mut socket: WebSocket) {
    let hub = service.relay_hub();
    let (guard, mut incoming) = hub.register(&lease);
    let mut tick = tokio::time::interval(Duration::from_secs(5));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_seen = tokio::time::Instant::now();
    loop {
        tokio::select! {
            biased;
            _=guard.cancelled.cancelled()=>break,
            _=tick.tick()=> {
                if last_seen.elapsed()>Duration::from_secs(30) || service.authenticate_session(lease.space.clone(),token.clone()).await.is_err() { break; }
                if !send(&mut socket,Message::Ping(Bytes::new())).await { break; }
            },
            packet=incoming.recv()=> {
                let Some(packet)=packet else {break};
                if guard.cancelled.is_cancelled() || !send(&mut socket,Message::Binary(packet)).await {break;}
            },
            message=socket.recv()=> {
                match message {
                    Some(Ok(Message::Binary(frame)))=> {last_seen=tokio::time::Instant::now();hub.forward(&guard,frame);},
                    Some(Ok(Message::Ping(data)))=> {last_seen=tokio::time::Instant::now();if !send(&mut socket,Message::Pong(data)).await {break;}},
                    Some(Ok(Message::Pong(_)))=> {last_seen=tokio::time::Instant::now();},
                    _=>break,
                }
            }
        }
    }
}
async fn send(socket: &mut WebSocket, message: Message) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(2), socket.send(message)).await,
        Ok(Ok(()))
    )
}
