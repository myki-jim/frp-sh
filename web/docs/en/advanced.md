# Advanced Usage

## End-to-end encryption

A matching passphrase on both sides enables ChaCha20-Poly1305 encryption (see [Architecture](./architecture#encryption-optional)):

```bash
# Host
frp-sh game create --service 127.0.0.1:25565 --key "our-passphrase"

# Guest
frp-sh game join game-a3f9c2 --listen 127.0.0.1:25565 --key "our-passphrase"
```

- A mismatch makes the session exit with `decryption failed (wrong --key?)`
- Encryption applies to **P2P direct** data frames (relay traffic is plaintext; see security notes)
- The passphrase is passed on the command line — be aware of shell history; wrap it yourself in sensitive environments

## Multi-connection reuse

By default one session accepts **unlimited sequential connections** (reconnect, multiple logins — all reuse the same tunnel):

```bash
# Cap at 3 connections; the session ends when exhausted
frp-sh game create --max-conns 3
frp-sh game join game-xxxx --max-conns 3
```

Great for: game client reconnects, or sharing with a strict connection cap.

> Note: connections are **sequential** (one at a time; reconnect right after a disconnect). Concurrent multiplexing is on the [roadmap](./roadmap).

## Force relay

```bash
frp-sh game create --relay
frp-sh game join game-xxxx --relay
```

- Skips punching and goes straight through the server relay
- For: known non-punchable NATs, server-side traffic logging, or quick link verification

## Tuning the punch spread

```bash
# Widen the spread (try ±3~5 for symmetric NAT)
frp-sh game create --spread 5
frp-sh game join game-xxxx --spread 5
```

- Default `--spread 2`
- Bigger spread = higher hit rate but more wasted packets (ICMP noise is ignored)
- The two ends do not need to match

## Separate UDP probe port

When a cloud firewall requires separate TCP/UDP ports (see [Configuration](./config)):

```toml
signaling_addr = "http://101.43.41.195:8080"
relay_addr     = "101.43.41.195:8081"
signaling_udp  = "101.43.41.195:8082"
```

## Proxy environments (HTTP signaling via proxy)

The HTTP signaling client honors standard proxy environment variables:

```bash
# Linux
export HTTP_PROXY=http://127.0.0.1:7890
export HTTPS_PROXY=http://127.0.0.1:7890

# Windows PowerShell
$env:HTTP_PROXY = "http://127.0.0.1:7890"
$env:HTTPS_PROXY = "http://127.0.0.1:7890"
```

> Only HTTP signaling goes through the proxy; UDP probing/punching and the relay TCP connection stay direct. If UDP is blocked by the network, use `--relay` mode instead.

## Debugging

```bash
frp-sh --verbose game create
# frame-level logs: recv/send frame type, seq, ack, retransmits, etc.
```

Frame log example:

```text
DEBUG frp_sh::p2p::stream] send to 127.0.0.1:49278: len=19
DEBUG frp_sh::p2p::stream] recv kind=Data seq=1 ack=1 len=4
DEBUG frp_sh::p2p::stream] recv kind=Ack seq=4 ack=3 len=0
```

## Common host-service recipes

| Scenario | Host command | Guest command |
|----------|--------------|---------------|
| Minecraft | `create --service 127.0.0.1:25565` | `join <room> --listen 127.0.0.1:25565` |
| SSH | `create --service 127.0.0.1:22` | `join <room> --listen 127.0.0.1:2222` |
| RDP | `create --service 127.0.0.1:3389` | `join <room> --listen 127.0.0.1:3389` |
| Any web service | `create --service 127.0.0.1:8080` | `join <room> --listen 127.0.0.1:8080` |
