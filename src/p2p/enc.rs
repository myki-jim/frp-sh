//! 中继流量加密：ChaCha20-Poly1305 逐块 AEAD 流。
//!
//! 服务器设置 `--password` 后，中继通道（客户端 ↔ 服务器）的流量以
//! 密码派生密钥加密，防止第三方嗅探（服务器持有密码，可解密转发；
//! 需要防服务器时请另加 `--key` 做端到端加密）。
//!
//! 帧格式：`[u32 BE len][ChaCha20-Poly1305 密文+tag]`，nonce 为
//! 12 字节（4 字节零 + 8 字节发送方计数器）。

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// 由口令派生 32 字节密钥（SHA-256）。
pub fn key_from_password(pw: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(pw.as_bytes());
    h.finalize().into()
}

/// 单块密文上限（1 MB），防止恶意长度头撑爆内存。
const MAX_BLOCK: usize = 1 << 20;

/// 加密流：包装任意 `AsyncRead + AsyncWrite`，按块加解密。
pub struct EncStream<S> {
    inner: S,
    cipher: ChaCha20Poly1305,
    enc_counter: u64,
    dec_counter: u64,
    // 读侧状态
    rd_header: [u8; 4],
    rd_header_len: usize,
    rd_body: Vec<u8>,
    rd_body_len: usize,
    rd_plain: Vec<u8>,
    rd_plain_pos: usize,
    // 写侧缓冲（待发送密文）
    wr_buf: Vec<u8>,
    wr_pos: usize,
}

impl<S: AsyncRead + AsyncWrite + Unpin> EncStream<S> {
    pub fn new(inner: S, key: &[u8; 32]) -> Self {
        let cipher = ChaCha20Poly1305::new(key.into());
        Self {
            inner,
            cipher,
            enc_counter: 0,
            dec_counter: 0,
            rd_header: [0; 4],
            rd_header_len: 0,
            rd_body: Vec::new(),
            rd_body_len: 0,
            rd_plain: Vec::new(),
            rd_plain_pos: 0,
            wr_buf: Vec::new(),
            wr_pos: 0,
        }
    }

    fn next_nonce(counter: u64) -> Nonce {
        let mut n = [0u8; 12];
        n[4..].copy_from_slice(&counter.to_be_bytes());
        *Nonce::from_slice(&n)
    }

    /// 将写缓冲中的密文尽量写入底层。
    fn flush_wr(&mut self, cx: &mut Context<'_>) -> io::Result<()> {
        while self.wr_pos < self.wr_buf.len() {
            match Pin::new(&mut self.inner).poll_write(cx, &self.wr_buf[self.wr_pos..]) {
                Poll::Pending => return Ok(()),
                Poll::Ready(Ok(0)) => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "write zero during flush",
                    ));
                }
                Poll::Ready(Ok(n)) => self.wr_pos += n,
                Poll::Ready(Err(e)) => return Err(e),
            }
        }
        self.wr_buf.clear();
        self.wr_pos = 0;
        Ok(())
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for EncStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // 通过 get_mut 取得 &mut Self 以便字段级拆分借用（Pin 的字段访问走 Deref 无法拆分）
        let this = self.as_mut().get_mut();
        loop {
            // 1) 已有解密数据 → 直接返回
            if this.rd_plain_pos < this.rd_plain.len() {
                let n = std::cmp::min(buf.remaining(), this.rd_plain.len() - this.rd_plain_pos);
                buf.put_slice(&this.rd_plain[this.rd_plain_pos..this.rd_plain_pos + n]);
                this.rd_plain_pos += n;
                if this.rd_plain_pos == this.rd_plain.len() {
                    this.rd_plain.clear();
                    this.rd_plain_pos = 0;
                }
                return Poll::Ready(Ok(()));
            }
            // 2) 读 4 字节长度头
            while this.rd_header_len < 4 {
                let mut hbuf = ReadBuf::new(&mut this.rd_header[this.rd_header_len..]);
                match Pin::new(&mut this.inner).poll_read(cx, &mut hbuf) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(())) => {
                        if hbuf.filled().is_empty() {
                            return Poll::Ready(Ok(())); // EOF
                        }
                        this.rd_header_len += hbuf.filled().len();
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                }
            }
            let len = u32::from_be_bytes(this.rd_header) as usize;
            if len > MAX_BLOCK {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "encrypted block too large",
                )));
            }
            // 3) 读密文
            this.rd_body.resize(len, 0);
            while this.rd_body_len < len {
                let mut bbuf = ReadBuf::new(&mut this.rd_body[this.rd_body_len..]);
                match Pin::new(&mut this.inner).poll_read(cx, &mut bbuf) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Ok(())) => {
                        if bbuf.filled().is_empty() {
                            return Poll::Ready(Ok(())); // EOF
                        }
                        this.rd_body_len += bbuf.filled().len();
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                }
            }
            // 4) 解密
            let pt = this
                .cipher
                .decrypt(&Self::next_nonce(this.dec_counter), &this.rd_body[..len])
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "relay decryption failed")
                })?;
            this.dec_counter += 1;
            this.rd_plain = pt;
            this.rd_plain_pos = 0;
            this.rd_header_len = 0;
            this.rd_body_len = 0;
            this.rd_body.clear();
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for EncStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.as_mut().get_mut();
        let ct = this
            .cipher
            .encrypt(&Self::next_nonce(this.enc_counter), buf)
            .map_err(|_| io::Error::other("relay encryption failed"))?;
        this.enc_counter += 1;
        this.wr_buf
            .extend_from_slice(&(ct.len() as u32).to_be_bytes());
        this.wr_buf.extend_from_slice(&ct);
        this.flush_wr(cx)?;
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        this.flush_wr(cx)?;
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        this.flush_wr(cx)?;
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}
