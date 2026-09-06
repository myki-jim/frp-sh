//! Versioned local network helper. It never accepts shell commands or executable paths.
use crate::p2p::tun::TunConfig;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io,
    net::Ipv4Addr,
    pin::Pin,
    sync::{Arc, Mutex, OnceLock},
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
pub mod platform;
pub mod service;
const VERSION: u32 = 1;
const MAX_PACKET: usize = 65535;
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Open {
        version: u32,
        config: TunConfig,
    },
    Route {
        version: u32,
        session: String,
        cidr: String,
    },
    Forward {
        version: u32,
        session: String,
    },
    Status {
        version: u32,
    },
}
#[derive(Serialize, Deserialize)]
struct Response {
    ok: bool,
    version: u32,
    name: Option<String>,
    session: Option<String>,
    error: Option<String>,
}
impl Response {
    fn ok() -> Self {
        Self {
            ok: true,
            version: VERSION,
            name: None,
            session: None,
            error: None,
        }
    }
    fn failure(e: &str) -> Self {
        Self {
            ok: false,
            error: Some(e.into()),
            ..Self::ok()
        }
    }
}
pub fn validate(cfg: &TunConfig) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(cfg.name.as_str(), "frp0" | "frp1"),
        "Unsupported device role"
    );
    let ip: Ipv4Addr = cfg.ip.parse()?;
    anyhow::ensure!(ip.is_private(), "Virtual address must be private IPv4");
    let mask: u32 = u32::from(cfg.netmask.parse::<Ipv4Addr>()?);
    let prefix = mask.leading_ones();
    anyhow::ensure!(
        (16..=30).contains(&prefix) && mask == u32::MAX << (32 - prefix),
        "Netmask must be contiguous /16 through /30"
    );
    anyhow::ensure!(
        (576..=9000).contains(&cfg.mtu),
        "MTU must be 576 through 9000"
    );
    Ok(())
}
fn validate_route(cidr: &str) -> anyhow::Result<()> {
    let (net, prefix) =
        crate::utils::parse_cidr(cidr).ok_or_else(|| anyhow::anyhow!("Invalid route"))?;
    anyhow::ensure!(
        (16..=32).contains(&prefix) && Ipv4Addr::from(net).is_private(),
        "Only private IPv4 routes /16 or narrower are allowed"
    );
    Ok(())
}
async fn write_json<S: AsyncWrite + Unpin, T: Serialize>(s: &mut S, v: &T) -> io::Result<()> {
    let b = serde_json::to_vec(v).map_err(io::Error::other)?;
    s.write_u32(b.len() as u32).await?;
    s.write_all(&b).await?;
    s.flush().await
}
async fn read_json<S: AsyncRead + Unpin, T: serde::de::DeserializeOwned>(
    s: &mut S,
) -> io::Result<T> {
    let n = s.read_u32().await? as usize;
    if n > 8192 {
        return Err(io::Error::other("Helper request too large"));
    }
    let mut b = vec![0; n];
    s.read_exact(&mut b).await?;
    serde_json::from_slice(&b).map_err(io::Error::other)
}
async fn request(r: Request) -> anyhow::Result<Response> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut s = platform::connect().await?;
        write_json(&mut s, &r).await?;
        let reply: Response = read_json(&mut s).await?;
        anyhow::ensure!(
            reply.version == VERSION,
            "Network helper version mismatch; repair the installation"
        );
        anyhow::ensure!(
            reply.ok,
            "{}",
            reply
                .error
                .unwrap_or_else(|| "Network helper failed".into())
        );
        Ok(reply)
    })
    .await
    .map_err(|_| anyhow::anyhow!("Network helper timed out"))?
}
static CLIENT_SESSIONS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
fn session(name: &str) -> anyhow::Result<String> {
    CLIENT_SESSIONS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .get(name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("No active network session"))
}
pub async fn route(name: &str, cidr: &str) -> anyhow::Result<()> {
    validate_route(cidr)?;
    request(Request::Route {
        version: VERSION,
        session: session(name)?,
        cidr: cidr.into(),
    })
    .await?;
    Ok(())
}
pub async fn forward(name: &str) -> anyhow::Result<()> {
    request(Request::Forward {
        version: VERSION,
        session: session(name)?,
    })
    .await?;
    Ok(())
}
pub async fn status() -> anyhow::Result<()> {
    request(Request::Status { version: VERSION }).await?;
    Ok(())
}
pub struct Device {
    name: String,
    session: String,
    rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    tx: tokio_util::sync::PollSender<Vec<u8>>,
    task: tokio::task::JoinHandle<()>,
}
impl Device {
    pub fn name(&self) -> &str {
        &self.name
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        self.task.abort();
        let mut sessions = CLIENT_SESSIONS
            .get_or_init(Default::default)
            .lock()
            .unwrap();
        if sessions.get(&self.name) == Some(&self.session) {
            sessions.remove(&self.name);
        }
    }
}
pub async fn open(cfg: &TunConfig) -> anyhow::Result<Device> {
    validate(cfg)?;
    let mut s=platform::connect().await.map_err(|e|anyhow::anyhow!("Network helper unavailable: {e}. Run the installer to install or repair the system service."))?;
    let response: Response = tokio::time::timeout(Duration::from_secs(15), async {
        write_json(
            &mut s,
            &Request::Open {
                version: VERSION,
                config: cfg.clone(),
            },
        )
        .await?;
        read_json(&mut s).await
    })
    .await??;
    anyhow::ensure!(
        response.version == VERSION && response.ok,
        "{}",
        response
            .error
            .unwrap_or_else(|| "Network helper version mismatch".into())
    );
    let name = response
        .name
        .ok_or_else(|| anyhow::anyhow!("Missing device name"))?;
    let id = response
        .session
        .ok_or_else(|| anyhow::anyhow!("Missing session identifier"))?;
    let (to_tx, mut to_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let (from_tx, from_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let task = tokio::spawn(async move {
        let (mut rd, mut wr) = tokio::io::split(s);
        let read = async {
            loop {
                let n = rd.read_u32().await? as usize;
                if !(20..=MAX_PACKET).contains(&n) {
                    return Err(io::Error::other("Invalid packet length"));
                }
                let mut b = vec![0; n];
                rd.read_exact(&mut b).await?;
                if from_tx.send(b).await.is_err() {
                    return Ok(());
                }
            }
        };
        let write = async {
            while let Some(b) = to_rx.recv().await {
                wr.write_u32(b.len() as u32).await?;
                wr.write_all(&b).await?;
            }
            wr.shutdown().await
        };
        if let Err(e) = tokio::try_join!(read, write) {
            log::debug!("network helper session ended: {e}");
        }
    });
    CLIENT_SESSIONS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(name.clone(), id.clone());
    Ok(Device {
        name,
        session: id,
        rx: from_rx,
        tx: tokio_util::sync::PollSender::new(to_tx),
        task,
    })
}
impl AsyncRead for Device {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.rx.poll_recv(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Network helper disconnected",
            ))),
            Poll::Ready(Some(p)) => {
                if buf.remaining() < p.len() {
                    return Poll::Ready(Err(io::Error::other("Packet buffer too small")));
                }
                buf.put_slice(&p);
                Poll::Ready(Ok(()))
            }
        }
    }
}
impl AsyncWrite for Device {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if buf.len() > MAX_PACKET {
            return Poll::Ready(Err(io::Error::other("Packet too large")));
        }
        std::task::ready!(self.tx.poll_reserve(cx)).map_err(io::Error::other)?;
        self.tx.send_item(buf.to_vec()).map_err(io::Error::other)?;
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.tx.close();
        Poll::Ready(Ok(()))
    }
}
struct OwnedNetwork {
    name: String,
    role: String,
    ip: String,
    routes: Vec<String>,
    forward: bool,
}
impl Drop for OwnedNetwork {
    fn drop(&mut self) {
        if self.forward {
            crate::p2p::tun::release_forward_local();
        }
        for cidr in self.routes.iter().rev() {
            let _ = crate::p2p::tun::delete_route_local(cidr, &self.name, &self.ip);
        }
        let _ = crate::p2p::tun::remove_firewall_local(&self.name);
    }
}
type Sessions = Arc<Mutex<HashMap<String, OwnedNetwork>>>;
struct Lease {
    sessions: Sessions,
    id: String,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.sessions.lock().unwrap().remove(&self.id);
    }
}
async fn handle<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    mut io: S,
    sessions: Sessions,
) -> anyhow::Result<()> {
    let req: Request = tokio::time::timeout(Duration::from_secs(5), read_json(&mut io)).await??;
    let version = match &req {
        Request::Open { version, .. }
        | Request::Route { version, .. }
        | Request::Forward { version, .. }
        | Request::Status { version } => *version,
    };
    if version != VERSION {
        write_json(&mut io, &Response::failure("Unsupported IPC version")).await?;
        return Ok(());
    }
    match req {
        Request::Status { .. } => write_json(&mut io, &Response::ok()).await?,
        Request::Route { session, cidr, .. } => {
            let result = (|| -> anyhow::Result<()> {
                validate_route(&cidr)?;
                let mut map = sessions.lock().unwrap();
                let s = map
                    .get_mut(&session)
                    .ok_or_else(|| anyhow::anyhow!("Unknown session"))?;
                anyhow::ensure!(s.routes.len() < 128, "Route limit reached");
                if !s.routes.contains(&cidr) {
                    crate::p2p::tun::add_route_local(&cidr, &s.name, &s.ip)?;
                    s.routes.push(cidr);
                }
                Ok(())
            })();
            let reply = match result {
                Ok(()) => Response::ok(),
                Err(e) => Response::failure(&e.to_string()),
            };
            write_json(&mut io, &reply).await?;
        }
        Request::Forward { session, .. } => {
            let reply = {
                let mut map = sessions.lock().unwrap();
                if let Some(s) = map.get_mut(&session) {
                    if s.forward {
                        Response::ok()
                    } else {
                        match crate::p2p::tun::enable_forward_local(&s.name) {
                            Ok(()) => {
                                s.forward = true;
                                Response::ok()
                            }
                            Err(e) => Response::failure(&e.to_string()),
                        }
                    }
                } else {
                    Response::failure("Unknown session")
                }
            };
            write_json(&mut io, &reply).await?;
        }
        Request::Open { config, .. } => {
            let result = (|| -> anyhow::Result<_> {
                validate(&config)?;
                let mut map = sessions.lock().unwrap();
                anyhow::ensure!(
                    map.len() < 2 && !map.values().any(|s| s.role == config.name),
                    "Device role already in use"
                );
                // Keep serialization through creation to prevent two callers acquiring the same role.
                let dev = crate::p2p::tun::create_local(&config)?;
                let name = crate::p2p::tun::device_name_local(&dev)
                    .ok_or_else(|| anyhow::anyhow!("Device name unavailable"))?;
                let mut network = OwnedNetwork {
                    name: name.clone(),
                    role: config.name.clone(),
                    ip: config.ip.clone(),
                    routes: Vec::new(),
                    forward: false,
                };
                crate::p2p::tun::allow_firewall_local(&name)?;
                if let Some(cidr) = crate::utils::cidr_from_ip_netmask(&config.ip, &config.netmask)
                {
                    crate::p2p::tun::add_subnet_route_local(&cidr, &name)?;
                    #[cfg(target_os = "macos")]
                    network.routes.push(cidr);
                    #[cfg(not(target_os = "macos"))]
                    let _ = &mut network;
                }
                let id = uuid::Uuid::new_v4().to_string();
                map.insert(id.clone(), network);
                Ok((dev, name, id))
            })();
            let (mut dev, name, id) = match result {
                Ok(v) => v,
                Err(e) => {
                    write_json(&mut io, &Response::failure(&e.to_string())).await?;
                    return Ok(());
                }
            };
            let reply = Response {
                session: Some(id.clone()),
                name: Some(name),
                ..Response::ok()
            };
            let _lease = Lease { sessions, id };
            write_json(&mut io, &reply).await?;
            let (mut rd, mut wr) = tokio::io::split(io);
            let (mut tun_rd, mut tun_wr) = tokio::io::split(&mut dev);
            let read = async {
                loop {
                    let n = rd.read_u32().await? as usize;
                    anyhow::ensure!((20..=MAX_PACKET).contains(&n), "Invalid packet size");
                    let mut p = vec![0; n];
                    rd.read_exact(&mut p).await?;
                    anyhow::ensure!(p[0] >> 4 == 4, "Only IPv4 is supported");
                    tun_wr.write_all(&p).await?;
                }
                #[allow(unreachable_code)]
                Ok::<(), anyhow::Error>(())
            };
            let write = async {
                let mut p = vec![0; MAX_PACKET];
                loop {
                    let n = tun_rd.read(&mut p).await?;
                    if n == 0 {
                        break;
                    }
                    wr.write_u32(n as u32).await?;
                    wr.write_all(&p[..n]).await?;
                }
                Ok::<(), anyhow::Error>(())
            };
            tokio::try_join!(read, write)?;
        }
    }
    Ok(())
}
pub async fn run() -> anyhow::Result<()> {
    let policy = platform::Policy::load()?;
    let sessions: Sessions = Default::default();
    platform::listen(policy, move |io| {
        let sessions = sessions.clone();
        async move {
            let _ = handle(io, sessions).await;
        }
    })
    .await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helper_rejects_unbounded_privileges() {
        let mut c = TunConfig {
            name: "frp0".into(),
            ip: "10.66.0.1".into(),
            netmask: "255.255.255.0".into(),
            mtu: 1400,
        };
        assert!(validate(&c).is_ok());
        c.name = "eth0;reboot".into();
        assert!(validate(&c).is_err());
        for s in [
            "0.0.0.0/0",
            "127.0.0.1/32",
            "8.8.8.8/32",
            "10.0.0.0/8",
            "192.168.1.0/24;id",
        ] {
            assert!(validate_route(s).is_err(), "{s}");
        }
        assert!(validate_route("192.168.1.0/24").is_ok());
    }
}
