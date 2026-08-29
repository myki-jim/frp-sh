# Introduction

**frp-sh** is a "social P2P mesh" tool written in pure Rust. It reduces setting up
inner-network connectivity to three social steps:

```text
frp-sh lan create          # Host: generate a room code (mesh: whole-machine virtual NIC)
frp-sh lan join lan-xxxx   # Friend: join with the code; reach the host's machine and LAN
```

After joining, the two sides establish a **UDP hole-punched direct link** (P2P).
frp-sh ships three usage series:

| Series | Command | Use case |
|--------|---------|----------|
| **Mesh (recommended)** | `frp-sh lan` | Tailscale-like: virtual-NIC whole-machine mesh; reach the peer's machine and their whole LAN |
| **Game** | `frp-sh game` | multiplayer games like Minecraft, pure port forwarding (default 25565) |
| **Dev** | `frp-sh dev` | development, application-level port forwarding |

When NAT is too strict and punching fails, traffic automatically falls back to relay
through the signaling server.

## Key Features

| Feature | Description |
|---------|-------------|
| Room-based social networking | A `lan-xxxxxx` code is all a friend needs — no IP configuration |
| Virtual-NIC mesh | `lan` series: layer-2 tunnel into the peer's whole machine and LAN (Tailscale-like) |
| Same-LAN direct link | LAN addresses auto-advertised; instant direct link on the same WiFi, no server |
| UDP hole punching | STUN-lite public probing + simultaneous PUNCH/ACK handshake, works through restricted cone NAT |
| Port spread | `--spread` lightweight port prediction to improve symmetric-NAT hit rate |
| End-to-end encryption | `--key <passphrase>` enables ChaCha20-Poly1305 on both sides |
| Multi-connection reuse | One session sequentially carries multiple TCP connections |
| Relay fallback | Automatic TCP relay when punching fails, plus a late-direct re-check that heals asymmetric cases |
| Single binary | No frp / libp2p / webrtc dependencies; everything is self-contained |

## Use Cases

- **Whole-machine mesh**: form a virtual LAN between computers; reach SSH, file shares,
  printers on either side (`lan`)
- **LAN-style game multiplayer**: the host opens a room; friends join by code — no
  public IP, no port forwarding (`game`)
- **Remote access to a home computer**: expose SSH / remote desktop behind the tunnel
- **Dev collaboration**: temporarily forward a local service to a teammate (`dev`)

## Not a Fit For

- Anonymous public access (the room code is the access credential)
- Authentication beyond encryption (`--key` provides confidentiality, not identity)
- Strict symmetric NAT without a relay server (relay fallback guarantees availability)

## Architecture Overview

```mermaid
sequenceDiagram
    participant H as Host (lan create)
    participant S as Signaling Server
    participant G as Guest (lan join)
    H->>S: UDP probe → learn public address
    H->>S: POST /room/create → room code
    G->>S: GET /room/{id} → host address
    G->>S: UDP probe → learn public address
    G->>S: POST /room/{id}/join → register guest address
    par Simultaneous punching
        G->>H: PUNCH packets (repeated, ±spread)
        H->>G: ACK packets
        H->>G: PUNCH packets
        G->>H: ACK packets
    end
    Note over G,H: Direct link → FRS1 reliable stream → CNEW tunnel framing
    alt Punch timeout
        G->>S: Relay HELLO GUEST
        H->>S: Relay HELLO HOST
        S-->>G,H: Paired, server copies both ways
    end
```

## Project Info

- Language: Rust (2021 edition), Tokio async runtime
- Dependencies: tokio / clap / axum / reqwest / serde / chacha20poly1305 / sha2
- License: MIT
