# Advanced Usage

## End-to-end encryption

A matching passphrase on both sides enables ChaCha20-Poly1305 encryption (see [Architecture](./architecture#authentication-and-encryption)):

```bash
# Host
frp-sh game create --service 127.0.0.1:25565 --key "our-passphrase"

# Guest
frp-sh game join game-a3f9c2 --listen 127.0.0.1:25565 --key "our-passphrase"
```

- A mismatch makes the session exit with `decryption failed (wrong --key?)`
- Service rooms apply an additional end-to-end encrypted stream when `--key` is supplied, including over TCP relay. Both peers must use the same key. Room-token transport encryption alone is not secrecy from the relay operator.
- The passphrase is passed on the command line — be aware of shell history; wrap it yourself in sensitive environments

## Concurrent services

Service rooms support up to 16 published services, 32 guests, 64 concurrent streams per guest and 256 per room. Use `frp-sh dev create --tcp 3000 --tcp 8080` and `frp-sh dev join 1234`. `--max-conns` is a legacy forwarding option, not a service-room concurrency setting. LAN rooms carry virtual-network traffic rather than a single sequential TCP connection.

## Force relay

```bash
frp-sh game create --relay
frp-sh game join game-xxxx --relay
```

- Skips UDP punching and TURN and forces TCP relay.
- For: known non-punchable NATs, server-side traffic logging, or quick link verification

## Configure TURN providers

By default a punch failure goes to the private TCP relay; with `turn_providers` configured, **TURN relay** is preferred (UDP, standard RFC 5766, interoperable with coturn/Cloudflare TURN, etc.). In `config.toml`:

```toml
# Single provider (built-in TURN server: serve --turn 0.0.0.0:3478)
turn_providers = ["turn://frp-sh:YOUR_PASSWORD@SERVER_IP:3478"]

# Multiple providers: parallel speed test, auto-picks the lowest RTT
turn_providers = [
  "turn://frp-sh:YOUR_PASSWORD@101.43.41.195:3478",   # your own VPS
  "turn://user:pass@turn.example.com:3478",           # third-party TURN
]
```

- Without `--relay`, punch failure can try available TURN providers, then TCP relay. `--relay` forces TCP.
- It's a global setting — no need to configure per room; the `frp-sh config` wizard doesn't ask about TURN yet, so edit the config by hand
- Deploying the built-in TURN relay: see [Server deployment](./server#built-in-turn-relay-optional)

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
