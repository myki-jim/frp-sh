//! Foreground attachment to a permanent space; transport work never blocks control polling.
use super::{
    client::Client,
    mesh::Mesh,
    types::{Admission, Peer},
    Operation,
};
use crate::{
    device::key::DeviceKey,
    runtime::{Phase, SessionManager},
};
use anyhow::{ensure, Context};
use futures_util::{SinkExt, StreamExt};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_tungstenite::{
    tungstenite::{
        client::IntoClientRequest,
        protocol::{Message, WebSocketConfig},
    },
    MaybeTlsStream, WebSocketStream,
};
type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
struct Task(tokio::task::JoinHandle<()>);
impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub async fn run(client: Client, key: DeviceKey, space: String) -> anyhow::Result<()> {
    uuid::Uuid::parse_str(&space).context("invalid space ID")?;
    crate::commands::acquire_role_lock("guest")?;
    let key = Arc::new(key);
    let admission: Admission = serde_json::from_value(
        client
            .execute(
                &key,
                Operation::OpenSession {
                    space: space.clone(),
                },
                None,
            )
            .await?,
    )?;
    let address = admission.address.octets();
    ensure!(
        admission.space == space
            && admission.device == key.public_key()
            && address[..3] == [10, 66, 0]
            && (1..=254).contains(&address[3]),
        "invalid space admission"
    );
    crate::debuglog::protect(&admission.access_token);
    let result = attach(&client, key.clone(), &admission).await;
    // Best effort close: expiry also bounds a disconnected or crashed client's lease.
    let _ = tokio::time::timeout(
        Duration::from_secs(3),
        client.execute(
            &key,
            Operation::CloseSession {
                space,
                session: admission.session_id,
            },
            None,
        ),
    )
    .await;
    result
}
async fn attach(client: &Client, key: Arc<DeviceKey>, admission: &Admission) -> anyhow::Result<()> {
    let mut url = reqwest::Url::parse(client.origin())?;
    url.set_scheme("wss")
        .map_err(|_| anyhow::anyhow!("invalid relay origin"))?;
    url.set_path(&format!("/spaces/v1/{}/relay", admission.space));
    let mut request = url.as_str().into_client_request()?;
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {}", admission.access_token).parse()?,
    );
    let config = WebSocketConfig::default()
        .read_buffer_size(2048)
        .write_buffer_size(0)
        .max_write_buffer_size(4096)
        .max_message_size(Some(2048))
        .max_frame_size(Some(2048));
    let (socket, _) = tokio::time::timeout(
        Duration::from_secs(15),
        tokio_tungstenite::connect_async_with_config(request, Some(config), true),
    )
    .await
    .map_err(|_| anyhow::anyhow!("space relay timed out"))?
    .map_err(|_| anyhow::anyhow!("space relay unavailable or unauthorized"))?;
    let config = crate::p2p::tun::TunConfig {
        name: "frp0".into(),
        ip: admission.address.to_string(),
        netmask: "255.255.255.0".into(),
        mtu: 1400,
        allow_lan: false,
    };
    let device = crate::helper::open(&config).await?;
    crate::helper::route(device.name(), "10.66.0.0/24").await?;
    crate::stats::update_info(crate::stats::SessionInfo {
        mode: "lan".into(),
        room: admission.space.clone(),
        my_id: admission.device.clone(),
        vnet_ip: admission.address.to_string(),
        encryption: true,
        ..Default::default()
    });
    let manager = SessionManager::default();
    let _status = crate::local_status::publish_with_manager("guest", Some(manager.clone())).await?;
    let lease = manager.begin(false)?;
    lease.transition(Phase::Joining, None);
    crate::ui_println!("Space {} · {}", admission.space, admission.address);
    let (updates, rx) = tokio::sync::mpsc::channel(1);
    let client = client.clone();
    let space = admission.space.clone();
    let session = admission.session_id.clone();
    let watcher = Task(tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut renew = tokio::time::Instant::now() + Duration::from_secs(240);
        loop {
            interval.tick().await;
            let result = async {
                if tokio::time::Instant::now() >= renew {
                    client
                        .execute(
                            &key,
                            Operation::RefreshSession {
                                space: space.clone(),
                                session: session.clone(),
                            },
                            None,
                        )
                        .await?;
                    renew = tokio::time::Instant::now() + Duration::from_secs(240);
                }
                let value = client
                    .execute(
                        &key,
                        Operation::PeerSessions {
                            space: space.clone(),
                            session: session.clone(),
                        },
                        None,
                    )
                    .await?;
                serde_json::from_value::<Vec<Peer>>(value)
                    .map_err(|_| anyhow::anyhow!("invalid peer response"))
            }
            .await;
            let failed = result.is_err();
            if updates.send(result).await.is_err() || failed {
                break;
            }
        }
    }));
    let result = tokio::select! {
        result=pump(socket,device,Mesh::new(admission.address),rx,&lease)=>result,
        result=tokio::signal::ctrl_c()=>{result?;Ok(())},
    };
    drop(watcher);
    lease.transition(Phase::Stopped, None);
    result
}
async fn pump<D: AsyncRead + AsyncWrite + Unpin>(
    mut socket: Socket,
    mut device: D,
    mut mesh: Mesh,
    mut peers: tokio::sync::mpsc::Receiver<anyhow::Result<Vec<Peer>>>,
    lease: &crate::runtime::SessionLease,
) -> anyhow::Result<()> {
    let mut packet = [0u8; 65535];
    let mut tick = tokio::time::interval(Duration::from_secs(3));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_server = tokio::time::Instant::now();
    loop {
        tokio::select! {
            _=tick.tick()=>{
                ensure!(last_server.elapsed()<Duration::from_secs(20),"space relay heartbeat timed out");
                for message in mesh.heartbeat() {send(&mut socket,Message::Binary(message.into())).await?;}
                send(&mut socket,Message::Ping(Vec::new().into())).await?;
                lease.transition(if mesh.connected(){Phase::Connected}else{Phase::Joining},None);
            },
            update=peers.recv()=>{
                mesh.refresh(update.context("space control connection closed")??)?;
                for message in mesh.heartbeat(){send(&mut socket,Message::Binary(message.into())).await?;}
            },
            result=device.read(&mut packet)=>{
                let count=result?;ensure!(count>0,"network helper disconnected");
                for message in mesh.packet(&packet[..count]){send(&mut socket,Message::Binary(message.into())).await?;}
            },
            message=socket.next()=>{
                let message=message.context("space relay disconnected")?.map_err(|_|anyhow::anyhow!("space relay failed"))?;
                last_server=tokio::time::Instant::now();
                match message {
                    Message::Binary(frame)=>{
                        let result=mesh.receive(&frame);
                        if let Some(reply)=result.reply {send(&mut socket,Message::Binary(reply.into())).await?;}
                        if let Some(packet)=result.packet {tokio::time::timeout(Duration::from_secs(2),device.write_all(&packet)).await??;}
                    },
                    Message::Ping(data)=>send(&mut socket,Message::Pong(data)).await?,
                    Message::Pong(_)=>{},
                    _=>anyhow::bail!("space relay closed"),
                }
            }
        }
    }
}
async fn send(socket: &mut Socket, message: Message) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(2), socket.send(message))
        .await
        .map_err(|_| anyhow::anyhow!("space relay write timed out"))?
        .map_err(|_| anyhow::anyhow!("space relay write failed"))
}
