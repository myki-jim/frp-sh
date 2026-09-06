//! Authenticated bounded byte stream, protocol 2. Fresh mutual challenges
//! derive independent directional keys before application data is forwarded.
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::{
    io,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

pub fn key_from_password(pw: &str) -> [u8; 32] {
    Sha256::digest(pw.as_bytes()).into()
}
pub(crate) fn proof(key: &[u8; 32], label: &[u8], a: &[u8], b: &[u8]) -> [u8; 32] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("key length");
    mac.update(label);
    mac.update(a);
    mac.update(b);
    mac.finalize().into_bytes().into()
}
pub(crate) fn verify(key: &[u8; 32], label: &[u8], a: &[u8], b: &[u8], tag: &[u8]) -> bool {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("key length");
    mac.update(label);
    mac.update(a);
    mac.update(b);
    mac.verify_slice(tag).is_ok()
}
const BLOCK: usize = 16 * 1024;
#[derive(Default)]
struct Progress {
    accepted: u64,
    flushed: u64,
    closed: bool,
    error: Option<String>,
    waker: Option<Waker>,
}
impl Progress {
    fn wake(&mut self) {
        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }
}
pub struct EncStream {
    io: tokio::io::DuplexStream,
    task: tokio::task::JoinHandle<()>,
    progress: Arc<Mutex<Progress>>,
}
impl EncStream {
    pub fn new<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        mut inner: S,
        key: &[u8; 32],
    ) -> Self {
        let (io, mut bridge) = tokio::io::duplex(64 * 1024);
        let progress = Arc::new(Mutex::new(Progress::default()));
        let state = progress.clone();
        let key = *key;
        let task = tokio::spawn(async move {
            let result = run(&mut inner, &mut bridge, &key, &state).await;
            if let Err(e) = result {
                let mut s = state.lock().unwrap();
                s.error = Some(e.to_string());
                s.wake();
            }
        });
        Self { io, task, progress }
    }
}
impl Drop for EncStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}
fn nonce(counter: u64) -> Nonce {
    let mut n = [0; 12];
    n[4..].copy_from_slice(&counter.to_be_bytes());
    *Nonce::from_slice(&n)
}
async fn run<S: AsyncRead + AsyncWrite + Unpin>(
    inner: &mut S,
    bridge: &mut tokio::io::DuplexStream,
    key: &[u8; 32],
    state: &Arc<Mutex<Progress>>,
) -> io::Result<()> {
    let (send_key, recv_key) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let local: [u8; 32] = rand::random();
        let mut remote = [0; 32];
        let (mut hr, mut hw) = tokio::io::split(&mut *inner);
        tokio::try_join!(
            async {
                hw.write_all(&local).await?;
                hw.flush().await
            },
            async { hr.read_exact(&mut remote).await.map(|_| ()) }
        )?;
        if remote == local {
            return Err(invalid("reflected stream handshake"));
        }
        let confirm = proof(key, b"frpsh-v2-confirm", &local, &remote);
        let mut tag = [0; 32];
        tokio::try_join!(
            async {
                hw.write_all(&confirm).await?;
                hw.flush().await
            },
            async { hr.read_exact(&mut tag).await.map(|_| ()) }
        )?;
        if !verify(key, b"frpsh-v2-confirm", &remote, &local, &tag) {
            return Err(invalid("stream authentication failed"));
        }
        Ok((
            proof(key, b"frpsh-v2-key", &local, &remote),
            proof(key, b"frpsh-v2-key", &remote, &local),
        ))
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "stream authentication timeout"))??;
    let (mut nr, mut nw) = tokio::io::split(inner);
    let (mut ar, mut aw) = tokio::io::split(bridge);
    let send = async {
        let cipher = ChaCha20Poly1305::new((&send_key).into());
        let mut seq = 0u64;
        let mut buf = [0; BLOCK];
        loop {
            let n = ar.read(&mut buf).await?;
            let ct = cipher
                .encrypt(&nonce(seq), &buf[..n])
                .map_err(|_| invalid("encryption failed"))?;
            seq = seq
                .checked_add(1)
                .ok_or_else(|| invalid("stream sequence exhausted"))?;
            nw.write_u32(ct.len() as u32).await?;
            nw.write_all(&ct).await?;
            nw.flush().await?;
            {
                let mut s = state.lock().unwrap();
                s.flushed += n as u64;
                if n == 0 {
                    s.closed = true;
                }
                s.wake();
            }
            if n == 0 {
                nw.shutdown().await?;
                break;
            }
        }
        Ok::<(), io::Error>(())
    };
    let recv = async {
        let cipher = ChaCha20Poly1305::new((&recv_key).into());
        let mut seq = 0u64;
        loop {
            let n = nr.read_u32().await? as usize;
            if !(16..=BLOCK + 16).contains(&n) {
                return Err(invalid("invalid encrypted block length"));
            }
            let mut ct = vec![0; n];
            nr.read_exact(&mut ct).await?;
            let pt = cipher
                .decrypt(&nonce(seq), ct.as_slice())
                .map_err(|_| invalid("stream authentication failed"))?;
            seq = seq
                .checked_add(1)
                .ok_or_else(|| invalid("stream sequence exhausted"))?;
            if pt.is_empty() {
                aw.shutdown().await?;
                break;
            }
            aw.write_all(&pt).await?;
        }
        Ok::<(), io::Error>(())
    };
    tokio::try_join!(send, recv)?;
    Ok(())
}
impl AsyncRead for EncStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if let Some(e) = &self.progress.lock().unwrap().error {
            return Poll::Ready(Err(invalid(e)));
        }
        Pin::new(&mut self.io).poll_read(cx, buf)
    }
}
impl AsyncWrite for EncStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if let Some(e) = &self.progress.lock().unwrap().error {
            return Poll::Ready(Err(invalid(e)));
        }
        match Pin::new(&mut self.io).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => {
                self.progress.lock().unwrap().accepted += n as u64;
                Poll::Ready(Ok(n))
            }
            r => r,
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut s = self.progress.lock().unwrap();
        if let Some(e) = &s.error {
            return Poll::Ready(Err(invalid(e)));
        }
        if s.flushed == s.accepted {
            Poll::Ready(Ok(()))
        } else {
            s.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        std::task::ready!(Pin::new(&mut self.io).poll_shutdown(cx))?;
        let mut s = self.progress.lock().unwrap();
        if let Some(e) = &s.error {
            return Poll::Ready(Err(invalid(e)));
        }
        if s.closed {
            Poll::Ready(Ok(()))
        } else {
            s.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
