//! Explicit service IDs over a bounded, multiplexed room transport.
use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{mpsc, Semaphore},
    task::JoinSet,
    time::Instant,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceInfo {
    pub id: u16,
    pub label: String,
    pub protocol: Protocol,
    pub port: u16,
}
#[derive(Clone)]
pub struct Published {
    pub info: ServiceInfo,
    pub target: SocketAddr,
}
pub fn validate_catalog(items: &[ServiceInfo]) -> anyhow::Result<()> {
    ensure!(
        !items.is_empty() && items.len() <= 16,
        "publish between 1 and 16 services"
    );
    let mut ids = std::collections::HashSet::new();
    for s in items {
        ensure!(
            s.id != 0
                && ids.insert(s.id)
                && s.port != 0
                && !s.label.trim().is_empty()
                && s.label.len() <= 80
                && !s.label.chars().any(char::is_control),
            "invalid service metadata"
        );
    }
    Ok(())
}
pub fn published(
    tcp: &[String],
    udp: &[String],
    label: Option<&str>,
) -> anyhow::Result<Vec<Published>> {
    let mut result = Vec::new();
    for (values, protocol) in [(tcp, Protocol::Tcp), (udp, Protocol::Udp)] {
        for value in values {
            let endpoint = if value.starts_with("http://") || value.starts_with("https://") {
                let u = reqwest::Url::parse(value)?;
                ensure!(
                    u.username().is_empty()
                        && u.password().is_none()
                        && u.query().is_none()
                        && u.fragment().is_none()
                        && u.path() == "/",
                    "use a service origin URL without credentials, path or query"
                );
                let host = u.host_str().context("missing local host")?;
                format!(
                    "{host}:{}",
                    u.port_or_known_default().context("missing port")?
                )
            } else {
                value.clone()
            };
            let target = crate::access::local_endpoint(&endpoint)?;
            let id = result.len() as u16 + 1;
            result.push(Published {
                info: ServiceInfo {
                    id,
                    label: label.map(str::to_owned).unwrap_or_else(|| {
                        format!(
                            "{} {}",
                            if protocol == Protocol::Tcp {
                                "TCP"
                            } else {
                                "UDP"
                            },
                            target.port()
                        )
                    }),
                    protocol: protocol.clone(),
                    port: target.port(),
                },
                target,
            });
        }
    }
    validate_catalog(&result.iter().map(|s| s.info.clone()).collect::<Vec<_>>())?;
    Ok(result)
}
const OPEN: u8 = 1;
const DATA: u8 = 2;
const FIN: u8 = 3;
const RESET: u8 = 4;
const PING: u8 = 5;
const PONG: u8 = 6;
const MAX_DATA: usize = 16 * 1024;
#[derive(Debug)]
struct Frame {
    kind: u8,
    id: u32,
    data: Vec<u8>,
}
impl Frame {
    fn control(kind: u8, id: u32) -> Self {
        Self {
            kind,
            id,
            data: Vec::new(),
        }
    }
}
struct Rate {
    start: Instant,
    bytes: usize,
}
static RATE: std::sync::OnceLock<tokio::sync::Mutex<Rate>> = std::sync::OnceLock::new();
async fn throttle(bytes: usize) {
    const LIMIT: usize = 64 * 1024 * 1024;
    loop {
        let mut rate = RATE
            .get_or_init(|| {
                tokio::sync::Mutex::new(Rate {
                    start: Instant::now(),
                    bytes: 0,
                })
            })
            .lock()
            .await;
        if rate.start.elapsed() >= Duration::from_secs(1) {
            rate.start = Instant::now();
            rate.bytes = 0;
        }
        if rate.bytes + bytes <= LIMIT {
            rate.bytes += bytes;
            return;
        }
        let wait = Duration::from_secs(1).saturating_sub(rate.start.elapsed());
        drop(rate);
        tokio::time::sleep(wait).await;
    }
}
async fn read_frame(r: &mut (impl tokio::io::AsyncRead + Unpin)) -> anyhow::Result<Frame> {
    let kind = r.read_u8().await?;
    let id = r.read_u32().await?;
    let len = r.read_u32().await? as usize;
    ensure!(
        (OPEN..=PONG).contains(&kind) && len <= MAX_DATA,
        "invalid service frame"
    );
    ensure!(
        (kind == OPEN && len == 2 && id != 0)
            || (kind == DATA && id != 0)
            || ((FIN..=PONG).contains(&kind) && len == 0),
        "invalid service frame shape"
    );
    let mut data = vec![0; len];
    r.read_exact(&mut data).await?;
    Ok(Frame { kind, id, data })
}
async fn send(tx: &mpsc::Sender<Frame>, frame: Frame) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(5), tx.send(frame)).await??;
    Ok(())
}
enum Event {
    Tcp(u32, u16, TcpStream),
    Udp(u32, u16, Arc<UdpSocket>, SocketAddr),
    Datagram(u32, Vec<u8>),
    Close(u32),
}
pub enum Binding {
    Tcp(ServiceInfo, TcpListener),
    Udp(ServiceInfo, Arc<UdpSocket>),
}
pub async fn bind(items: &[ServiceInfo], listen: Option<&str>) -> anyhow::Result<Vec<Binding>> {
    validate_catalog(items)?;
    ensure!(
        listen.is_none() || items.len() == 1,
        "--listen is only valid when selecting one service"
    );
    let mut result = Vec::new();
    for s in items {
        let addr = if let Some(value) = listen {
            crate::access::local_endpoint(value)?
        } else {
            SocketAddr::from(([127, 0, 0, 1], s.port))
        };
        let b = match s.protocol {
            Protocol::Tcp => {
                let socket = match TcpListener::bind(addr).await {
                    Ok(s) => s,
                    Err(e) if listen.is_none() && e.kind() == std::io::ErrorKind::AddrInUse => {
                        TcpListener::bind("127.0.0.1:0").await?
                    }
                    Err(e) => return Err(e.into()),
                };
                Binding::Tcp(s.clone(), socket)
            }
            Protocol::Udp => {
                let socket = match UdpSocket::bind(addr).await {
                    Ok(s) => s,
                    Err(e) if listen.is_none() && e.kind() == std::io::ErrorKind::AddrInUse => {
                        UdpSocket::bind("127.0.0.1:0").await?
                    }
                    Err(e) => return Err(e.into()),
                };
                Binding::Udp(s.clone(), Arc::new(socket))
            }
        };
        result.push(b);
    }
    Ok(result)
}
async fn tcp_pump(
    socket: TcpStream,
    id: u32,
    mut input: mpsc::Receiver<Frame>,
    out: mpsc::Sender<Frame>,
) -> anyhow::Result<()> {
    socket.set_nodelay(true)?;
    let (mut r, mut w) = socket.into_split();
    let mut buf = vec![0; MAX_DATA];
    let mut read_done = false;
    let mut write_done = false;
    while !(read_done && write_done) {
        tokio::select! {
            r=r.read(&mut buf),if !read_done=>{let n=r?;if n==0{read_done=true;send(&out,Frame::control(FIN,id)).await?;}else{send(&out,Frame{kind:DATA,id,data:buf[..n].to_vec()}).await?;}},
            frame=input.recv()=>{match frame {
                Some(f) if f.kind==DATA && !write_done=>tokio::time::timeout(Duration::from_secs(5),w.write_all(&f.data)).await??,
                Some(f) if f.kind==FIN && !write_done=>{w.shutdown().await?;write_done=true;},
                _=>break,
            }},
            _=tokio::time::sleep(Duration::from_secs(300))=>break,
        }
    }
    Ok(())
}
async fn udp_host(
    target: SocketAddr,
    id: u32,
    mut input: mpsc::Receiver<Frame>,
    out: mpsc::Sender<Frame>,
) -> anyhow::Result<()> {
    let socket = UdpSocket::bind(if target.is_ipv6() {
        "[::1]:0"
    } else {
        "127.0.0.1:0"
    })
    .await?;
    socket.connect(target).await?;
    let mut buf = vec![0; MAX_DATA + 1];
    loop {
        tokio::select! {
            packet=socket.recv(&mut buf)=>{match packet {Ok(n) if n<=MAX_DATA=>send(&out,Frame{kind:DATA,id,data:buf[..n].to_vec()}).await?,Err(e) if matches!(e.kind(),std::io::ErrorKind::ConnectionRefused|std::io::ErrorKind::ConnectionReset)=>{},Err(e)=>return Err(e.into()),_=>{}}},
            frame=input.recv()=>{match frame{Some(f) if f.kind==DATA=>{socket.send(&f.data).await?;},_=>break}},
            _=tokio::time::sleep(Duration::from_secs(30))=>break,
        }
    }
    Ok(())
}
async fn udp_guest(
    socket: Arc<UdpSocket>,
    peer: SocketAddr,
    mut input: mpsc::Receiver<Frame>,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {f=input.recv()=>match f{Some(f) if f.kind==DATA=>{socket.send_to(&f.data,peer).await?;},_=>break},_=tokio::time::sleep(Duration::from_secs(30))=>break}
    }
    Ok(())
}
/// One transport per member, with independent TCP streams and UDP source flows.
/// Every task is owned by this JoinSet; disconnect drops listeners and all streams.
pub async fn session(
    transport: crate::p2p::relay::RelayStream,
    published: Vec<Published>,
    bindings: Vec<Binding>,
    budget: Arc<Semaphore>,
) -> anyhow::Result<()> {
    let host = !published.is_empty();
    let targets: HashMap<_, _> = published.into_iter().map(|s| (s.info.id, s)).collect();
    let (mut reader, mut writer) = tokio::io::split(transport);
    let (out, mut outgoing) = mpsc::channel::<Frame>(128);
    let (remote, mut incoming) = mpsc::channel(64);
    let (events, mut local) = mpsc::channel(64);
    let mut tasks: JoinSet<(u32, anyhow::Result<()>)> = JoinSet::new();
    tasks.spawn(async move {
        (
            0,
            async {
                let mut count = 0usize;
                let mut window = Instant::now();
                loop {
                    let f = read_frame(&mut reader).await?;
                    if window.elapsed() >= Duration::from_secs(1) {
                        window = Instant::now();
                        count = 0;
                    }
                    count += 1;
                    ensure!(count <= 16384, "service frame rate exceeded");
                    remote.send(f).await?;
                }
            }
            .await,
        )
    });
    tasks.spawn(async move {
        (
            0,
            async {
                while let Some(f) = outgoing.recv().await {
                    throttle(f.data.len() + 9).await;
                    writer.write_u8(f.kind).await?;
                    writer.write_u32(f.id).await?;
                    writer.write_u32(f.data.len() as u32).await?;
                    writer.write_all(&f.data).await?;
                    writer.flush().await?;
                }
                Ok(())
            }
            .await,
        )
    });
    let counter = Arc::new(AtomicU32::new(1));
    for b in bindings {
        let events = events.clone();
        let counter = counter.clone();
        tasks.spawn(async move{(0,async {match b {
        Binding::Tcp(info,listener)=>loop{let(socket,_)=listener.accept().await?;let id=counter.fetch_add(1,Ordering::Relaxed);ensure!(id!=0,"stream ID exhausted");events.send(Event::Tcp(id,info.id,socket)).await?;},
        Binding::Udp(info,socket)=>{let mut flows:HashMap<SocketAddr,(u32,Instant)>=HashMap::new();let mut buf=vec![0;MAX_DATA+1];let mut sweep=tokio::time::interval(Duration::from_secs(5));loop{tokio::select!{
            r=socket.recv_from(&mut buf)=>{let(n,peer)=r?;if n>MAX_DATA{continue} let id=if let Some((id,last))=flows.get_mut(&peer){*last=Instant::now();*id}else{if flows.len()>=64{continue}let id=counter.fetch_add(1,Ordering::Relaxed);ensure!(id!=0,"stream ID exhausted");events.send(Event::Udp(id,info.id,socket.clone(),peer)).await?;flows.insert(peer,(id,Instant::now()));id};events.send(Event::Datagram(id,buf[..n].to_vec())).await?;},
            _=sweep.tick()=>{let stale:Vec<_>=flows.iter().filter(|(_,(_,last))|last.elapsed()>Duration::from_secs(25)).map(|(p,(id,_))|(*p,*id)).collect();for(p,id)in stale{flows.remove(&p);events.send(Event::Close(id)).await?;}}
        }}}
    }}.await)});
    }
    let mut streams: HashMap<u32, mpsc::Sender<Frame>> = HashMap::new();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(3));
    let mut last = Instant::now();
    let mut opens = 0;
    let mut window = Instant::now();
    loop {
        tokio::select! {
            f=incoming.recv()=>{let f=f.context("transport ended")?;last=Instant::now();if !host {if let Some(v)=VIEW.lock().unwrap().as_mut(){v.mood=crate::pet::Mood::Relay;v.status=crate::i18n::text("Connected · encrypted TCP relay", "已连接 · 加密 TCP 中继").into();}}match f.kind{
                PING=>send(&out,Frame::control(PONG,0)).await?,PONG=>{},
                OPEN=>{ensure!(host,"publisher cannot request guest ports");if window.elapsed()>=Duration::from_secs(1){window=Instant::now();opens=0;}opens+=1;
                    let service=u16::from_be_bytes(f.data[..2].try_into()?);let permit=budget.clone().try_acquire_owned();
                    if streams.contains_key(&f.id)||streams.len()>=64||opens>64||permit.is_err()||!targets.contains_key(&service){send(&out,Frame::control(RESET,f.id)).await?;continue;}
                    let target=targets[&service].clone();crate::access::validate_local_endpoint(target.target)?;let(tx,rx)=mpsc::channel(16);streams.insert(f.id,tx);let out=out.clone();tasks.spawn(async move{let _permit=permit.unwrap();let result=async{match target.info.protocol{Protocol::Tcp=>{let socket=tokio::time::timeout(Duration::from_secs(3),TcpStream::connect(target.target)).await??;tcp_pump(socket,f.id,rx,out.clone()).await},Protocol::Udp=>udp_host(target.target,f.id,rx,out.clone()).await}}.await;let _=send(&out,Frame::control(RESET,f.id)).await;(f.id,result)});
                },
                RESET=>{streams.remove(&f.id);},
                DATA|FIN=>{if let Some(tx)=streams.get(&f.id){let id=f.id;if tx.try_send(f).is_err(){streams.remove(&id);send(&out,Frame::control(RESET,id)).await?;}}},_=>unreachable!()
            }},
            e=local.recv(),if !host=>{match e.context("listeners ended")?{
                Event::Tcp(id,service,socket)=>{let Ok(permit)=budget.clone().try_acquire_owned()else{continue};if streams.len()>=64{continue}let(tx,rx)=mpsc::channel(16);streams.insert(id,tx);send(&out,Frame{kind:OPEN,id,data:service.to_be_bytes().to_vec()}).await?;let out=out.clone();tasks.spawn(async move{let _permit=permit;let r=tcp_pump(socket,id,rx,out.clone()).await;let _=send(&out,Frame::control(RESET,id)).await;(id,r)});},
                Event::Udp(id,service,socket,peer)=>{let Ok(permit)=budget.clone().try_acquire_owned()else{continue};if streams.len()>=64{continue}let(tx,rx)=mpsc::channel(16);streams.insert(id,tx);send(&out,Frame{kind:OPEN,id,data:service.to_be_bytes().to_vec()}).await?;tasks.spawn(async move{let _permit=permit;(id,udp_guest(socket,peer,rx).await)});},
                Event::Datagram(id,data)=>{if streams.contains_key(&id){send(&out,Frame{kind:DATA,id,data}).await?;}},
                Event::Close(id)=>{streams.remove(&id);send(&out,Frame::control(RESET,id)).await?;}
            }},
            done=tasks.join_next()=>{let(id,result)=done.context("service tasks ended")??;if id==0{return result;}streams.remove(&id);if let Err(e)=result{log::warn!(target:"services","stream {id} closed: {e}");}},
            _=heartbeat.tick()=>{ensure!(last.elapsed()<Duration::from_secs(12),"service heartbeat expired");send(&out,Frame::control(PING,0)).await?;}
        }
    }
}

#[derive(Default)]
struct View {
    room: String,
    status: String,
    mood: crate::pet::Mood,
    lines: Vec<String>,
}
static VIEW: std::sync::Mutex<Option<View>> = std::sync::Mutex::new(None);
static REVOKE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
struct ViewGuard;
impl Drop for ViewGuard {
    fn drop(&mut self) {
        *VIEW.lock().unwrap() = None;
    }
}
pub fn copy_addresses() -> anyhow::Result<()> {
    let text = {
        let guard = VIEW.lock().unwrap();
        let v = guard.as_ref().context("no service view")?;
        ensure!(
            v.status.starts_with("Connected") || v.status.starts_with("已连接"),
            "wait for a connection before copying addresses"
        );
        v.lines
            .iter()
            .filter_map(|line| {
                line.split_whitespace()
                    .find(|s| s.parse::<SocketAddr>().is_ok())
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    ensure!(!text.is_empty(), "no local addresses available");
    crate::invite::copy(&text)
}
pub fn request_revoke() {
    REVOKE.store(true, Ordering::Relaxed);
}
pub fn companion_mood() -> Option<crate::pet::Mood> {
    VIEW.lock().unwrap().as_ref().map(|v| v.mood)
}
fn view(room: &str, status: &str, lines: Vec<String>, mood: crate::pet::Mood) {
    *VIEW.lock().unwrap() = Some(View {
        room: room.into(),
        status: status.into(),
        mood,
        lines,
    });
}
pub fn frame(w: u16, h: u16, page: usize) -> Option<crate::dashboard::Frame> {
    let guard = VIEW.lock().unwrap();
    let v = guard.as_ref()?;
    let size = h.saturating_sub(12).max(1) as usize;
    let pages = v.lines.len().div_ceil(size).max(1);
    let mut lines = vec![
        (
            format!("{}  {}", crate::i18n::text("ROOM", "房间"), v.room),
            1,
        ),
        (v.status.clone(), 2),
        (String::new(), 0),
    ];
    lines.extend(
        v.lines
            .iter()
            .skip((page % pages) * size)
            .take(size)
            .map(|s| (s.clone(), 0)),
    );
    let mut frame = crate::app::shell(
        w,
        h,
        crate::i18n::text("Services", "共享服务"),
        crate::i18n::text(
            "↑ ↓ Pages  C Copy addresses  I Invite  G Logs  R Revoke invites (host)  Q Exit",
            "↑ ↓ 翻页  C 复制地址  I 邀请  G 日志  R 撤销邀请（房主）  Q 退出",
        ),
        &lines,
    );
    frame.pages = pages;
    Some(frame)
}
fn binding_lines(bindings: &[Binding]) -> Vec<String> {
    bindings
        .iter()
        .map(|b| match b {
            Binding::Tcp(s, l) => {
                format!("□ #{} {}  TCP  {}", s.id, s.label, l.local_addr().unwrap())
            }
            Binding::Udp(s, l) => format!(
                "□ #{} {}  UDP  {} · {}",
                s.id,
                s.label,
                l.local_addr().unwrap(),
                crate::i18n::text("unverified until traffic", "收到流量前未验证")
            ),
        })
        .collect()
}
#[derive(Serialize, Deserialize)]
struct Control {
    next_id: u16,
    server: String,
    room: String,
    password: String,
    owner: String,
    targets: Vec<(ServiceInfo, String)>,
}
fn control_path() -> anyhow::Result<std::path::PathBuf> {
    Ok(crate::config::Config::default_path()
        .context("configuration directory unavailable")?
        .with_file_name("service-host.json"))
}
fn save_control(c: &Control) -> anyhow::Result<()> {
    use std::io::Write;
    let path = control_path()?;
    std::fs::create_dir_all(path.parent().unwrap())?;
    let temp = path.with_extension("new");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut f = options.open(&temp)?;
    f.write_all(&serde_json::to_vec(c)?)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(temp, path)?;
    Ok(())
}
fn read_control() -> anyhow::Result<Control> {
    let bytes = std::fs::read(control_path()?)
        .context("no active service host; create a service room first")?;
    ensure!(bytes.len() < 64 * 1024, "invalid host control file");
    serde_json::from_slice(&bytes).context("invalid host control file")
}
pub fn change(add: Vec<Published>, remove: Option<u16>) -> anyhow::Result<()> {
    let mut c = read_control()?;
    apply_change(&mut c, add, remove)?;
    save_control(&c)?;
    crate::ui_println!("Service update queued; active connections will reconnect");
    Ok(())
}
fn apply_change(c: &mut Control, add: Vec<Published>, remove: Option<u16>) -> anyhow::Result<()> {
    if let Some(id) = remove {
        c.targets.retain(|(s, _)| s.id != id);
    }
    let mut next = c.next_id;
    for mut s in add {
        s.info.id = next;
        next = next.checked_add(1).context("service IDs exhausted")?;
        c.targets.push((s.info, s.target.to_string()));
    }
    validate_catalog(&c.targets.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>())?;
    c.next_id = next;
    Ok(())
}

pub async fn revoke() -> anyhow::Result<()> {
    let mut c = read_control()?;
    let api = crate::signaling::SignalingClient::new_with_password(&c.server, Some(&c.password));
    api.set_room_token(&c.room, c.owner.clone());
    c.password = api.secure_room(&c.room).await?;
    save_control(&c)?;
    crate::ui_println!("Room invitations revoked; copy a new invitation from the host");
    Ok(())
}
struct ControlGuard(String);
impl Drop for ControlGuard {
    fn drop(&mut self) {
        if read_control().is_ok_and(|c| c.owner == self.0) {
            if let Ok(p) = control_path() {
                let _ = std::fs::remove_file(p);
            }
        }
    }
}

fn payload_stream(
    stream: crate::p2p::relay::RelayStream,
    key: Option<&str>,
) -> crate::p2p::relay::RelayStream {
    if let Some(key) = key {
        Box::new(crate::p2p::enc::EncStream::new(
            stream,
            &crate::p2p::enc::key_from_password(key),
        ))
    } else {
        stream
    }
}
pub async fn host(
    mut cfg: crate::config::Config,
    prefix: String,
    ttl: u64,
    mut targets: Vec<Published>,
    key: Option<String>,
) -> anyhow::Result<()> {
    let _screen = crate::terminal::monitor(true);
    let _view = ViewGuard;
    let api = crate::signaling::SignalingClient::new_with_password(
        &cfg.signaling_addr,
        cfg.password.as_deref(),
    );
    let created = api
        .create_room(
            &prefix,
            ttl,
            "127.0.0.1:1".parse()?,
            None,
            vec![],
            vec![],
            vec![],
            crate::version::VERSION.into(),
            None,
            Some(crate::config::device_name(cfg.name.as_deref())),
        )
        .await?;
    let token = api.secure_room(&created.room_id).await?;
    cfg.password = Some(token.clone());
    api.set_services(
        &created.room_id,
        &targets.iter().map(|s| s.info.clone()).collect::<Vec<_>>(),
    )
    .await?;
    let mut control = Control {
        next_id: targets.len() as u16 + 1,
        server: cfg.signaling_addr.clone(),
        room: created.room_id.clone(),
        password: token,
        owner: created.owner_token.clone(),
        targets: targets
            .iter()
            .map(|s| (s.info.clone(), s.target.to_string()))
            .collect(),
    };
    save_control(&control)?;
    let _control = ControlGuard(created.owner_token.clone());
    let _invite = crate::invite::activate_mode(&cfg, &created.room_id, key.as_deref(), "dev");
    crate::ui_println!("Room created : {}", created.room_id);
    let budget = Arc::new(Semaphore::new(256));
    let mut clients: HashMap<String, (SocketAddr, tokio::task::AbortHandle)> = HashMap::new();
    let mut jobs = JoinSet::new();
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let result: anyhow::Result<()>=async {loop{tokio::select!{
        _=crate::terminal::ctrl_c()=>break,
        done=jobs.join_next(),if !jobs.is_empty()=>{if let Some(Ok((id,addr,result)))=done{if clients.get(&id).is_some_and(|(a,_)|*a==addr){clients.remove(&id);}
if let Err(e)=result{log::warn!(target:"services","member stream ended: {e}");}}},
        _=tick.tick()=>{
            if REVOKE.swap(false,Ordering::Relaxed){control.password=api.secure_room(&created.room_id).await?;save_control(&control)?;}
            if let Ok(updated)=read_control(){if updated.owner==control.owner && serde_json::to_vec(&updated)?!=serde_json::to_vec(&control)?{
                targets=updated.targets.iter().map(|(info,target)|Ok(Published{info:info.clone(),target:crate::access::local_endpoint(target)?})).collect::<anyhow::Result<Vec<_>>>()?;
                api.set_services(&created.room_id,&targets.iter().map(|s|s.info.clone()).collect::<Vec<_>>()).await?;control=updated;
                jobs.abort_all();clients.clear();
            }}
            if cfg.password.as_deref()!=Some(&control.password){cfg.password=Some(control.password.clone());crate::invite::replace_mode(&cfg,&created.room_id,key.as_deref(),"dev");jobs.abort_all();clients.clear();}
            let info=api.get_room(&created.room_id).await?;
            let mut lines:Vec<_>=targets.iter().map(|s|format!("□ #{} {}  {:?}  {}",s.info.id,s.info.label,s.info.protocol,s.target)).collect();
            lines.push(String::new());lines.push(format!("{}  {}",crate::i18n::text("Devices","设备"),info.guests.len()+1));
            for g in info.guests{lines.push(format!("◇ {}",g.name.as_deref().unwrap_or(&g.uuid)));if clients.get(&g.uuid).is_some_and(|(a,_)|*a==g.addr){continue}
if let Some((_,task))=clients.remove(&g.uuid){task.abort();}
                if clients.len()>=32{continue}let id=g.uuid.clone();let addr=g.addr;let relay=cfg.relay_addr.parse()?;let room=created.room_id.clone();let token=control.password.clone();let owner=created.owner_token.clone();let targets=targets.clone();let budget=budget.clone();let key=key.clone();
                let handle=jobs.spawn(async move{let result=async {let(stream,_)=crate::p2p::relay::connect(relay,&room,crate::p2p::relay::RelayRole::Host,Some(&token),true,Some(&id),Some(owner)).await?;session(payload_stream(stream,key.as_deref()),targets,vec![],budget).await}.await;(id,addr,result)});clients.insert(g.uuid,(g.addr,handle));
            }
            view(&created.room_id,crate::i18n::text("Private services · encrypted TCP relay","私有服务 · 加密 TCP 中继"),lines,crate::pet::Mood::Serving);
        }
    }}Ok(())}.await;
    jobs.abort_all();
    let _ = api.delete_room(&created.room_id).await;
    result
}
pub async fn join(
    mut cfg: crate::config::Config,
    room: String,
    listen: Option<String>,
    selection: Option<u16>,
    key: Option<String>,
) -> anyhow::Result<()> {
    let _screen = crate::terminal::monitor(true);
    let _view = ViewGuard;
    let mut attempt = 0;
    loop {
        let api = crate::signaling::SignalingClient::new_with_password(
            &cfg.signaling_addr,
            cfg.password.as_deref(),
        );
        let info = api.get_room(&room).await?;
        validate_catalog(&info.services)?;
        let token = info
            .access_token
            .context("server does not support private service rooms; upgrade the server")?;
        crate::debuglog::protect(&token);
        cfg.password = Some(token.clone());
        let items: Vec<_> = info
            .services
            .into_iter()
            .filter(|s| selection.is_none_or(|id| s.id == id))
            .collect();
        ensure!(!items.is_empty(), "selected service was removed");
        let bindings = bind(&items, listen.as_deref()).await?;
        let lines = binding_lines(&bindings);
        for line in &lines {
            crate::ui_println!("{line}");
        }
        view(
            &room,
            crate::i18n::text(
                "Connecting · encrypted TCP relay",
                "正在连接 · 加密 TCP 中继",
            ),
            lines.clone(),
            crate::pet::Mood::Connecting,
        );
        let marker = UdpSocket::bind("127.0.0.1:0").await?;
        let uuid = cfg.uuid.clone().context("device ID missing")?;
        let _invite = crate::invite::activate_mode(&cfg, &room, key.as_deref(), "dev");
        let connect = async {
            let api = crate::signaling::SignalingClient::new_with_password(
                &cfg.signaling_addr,
                Some(&token),
            );
            api.join_room(
                &room,
                marker.local_addr()?,
                vec![],
                Some(uuid.clone()),
                None,
                vec![],
                None,
                Some(crate::config::device_name(cfg.name.as_deref())),
            )
            .await?;
            let (stream, _) = crate::p2p::relay::connect(
                cfg.relay_addr.parse()?,
                &room,
                crate::p2p::relay::RelayRole::Guest,
                Some(&token),
                true,
                Some(&uuid),
                None,
            )
            .await?;
            view(
                &room,
                crate::i18n::text(
                    "Waiting for publisher · encrypted TCP relay",
                    "等待发布者 · 加密 TCP 中继",
                ),
                lines,
                crate::pet::Mood::Connecting,
            );
            session(
                payload_stream(stream, key.as_deref()),
                vec![],
                bindings,
                Arc::new(Semaphore::new(64)),
            )
            .await
        };
        tokio::select! {r=connect=>{if let Err(e)=r{log::warn!(target:"services","connection ended: {e}");}},_=crate::terminal::ctrl_c()=>return Ok(())}
        attempt += 1;
        view(
            &room,
            crate::i18n::text(
                "Reconnecting; existing application connections are closed",
                "正在重连；原有应用连接已关闭",
            ),
            vec![],
            crate::pet::Mood::Reconnecting,
        );
        tokio::select! {_=tokio::time::sleep(Duration::from_secs(attempt.min(4)))=>{},_=crate::terminal::ctrl_c()=>return Ok(())}
    }
}

#[cfg(test)]
mod control_tests {
    use super::*;
    #[test]
    fn removing_a_service_never_reuses_its_authorized_id() {
        let target = published(&["3000".into()], &[], None).unwrap();
        let mut control = Control {
            next_id: 2,
            server: String::new(),
            room: String::new(),
            password: String::new(),
            owner: String::new(),
            targets: vec![(target[0].info.clone(), target[0].target.to_string())],
        };
        apply_change(&mut control, target.clone(), None).unwrap();
        assert_eq!(control.targets[1].0.id, 2);
        apply_change(&mut control, vec![], Some(2)).unwrap();
        apply_change(&mut control, target, None).unwrap();
        assert_eq!(control.targets[1].0.id, 3);
    }
}
