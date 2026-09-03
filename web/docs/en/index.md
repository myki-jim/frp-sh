---
layout: home

hero:
  name: frp-sh
  text: Social P2P Hole-Punching Tunnel
  tagline: Host creates a room, friends join by code — UDP hole punching with automatic relay fallback. Pure Rust, single binary.
  actions:
    - theme: brand
      text: Quickstart
      link: /en/quickstart
    - theme: alt
      text: Architecture
      link: /en/architecture
    - theme: alt
      text: GitHub
      link: https://github.com/myki-jim/frp-sh

features:
  - title:  Room-based social networking
    details: A room code is all you need — no public IP or port forwarding
  - title:  UDP hole punching
    details: STUN-lite public probing + simultaneous PUNCH/ACK handshake, works through restricted cone NAT
  - title:  End-to-end encryption
    details: A matching --key passphrase enables ChaCha20-Poly1305 transport
  - title:  Relay fallback
    details: Falls back to TURN relay (optional) or server TCP relay when punching fails; one failed round downgrades for good, late-direct re-check heals asymmetric cases
  - title:  Connection profiles
    details: frp-sh profile saves server/password/room in one line for instant reconnect; the panel's one-click join dedupes automatically
  - title:  Web panels on both ends
    details: Server and client panels show rooms, link rates, topology and live runtime logs (/debug incremental pull)
  - title:  Built-in TURN
    details: serve --turn — the single binary provides standard RFC 5766 TURN; clients auto-pick the best among multiple providers
  - title:  Single static binary
    details: ~6 MB pure-Rust binary, no frp / libp2p / webrtc dependencies
---

::: warning Development stage (0.x)

frp-sh is currently in **development** (`v0.3.x`): **features and the protocol may
change before 1.0**, and backward compatibility is not promised. Versions ship
fast — keep up with updates, and if you deploy in production watch update notices
and big-gap warnings. See the [versioning policy](./versioning).

:::

## About the Name

We call ourselves `frp.sh`, but we have **nothing** to do with [FRP](https://github.com/fatedier/frp) — pure "domain squatting".
**F**ast Room Protocol · **R**eal-time P2P · **P**layful Shell.
**We don't penetrate your NAT. We delete the concept of NAT.** [（More on the name →）](./name)

## Install

One-line install / update (same link for both — re-run to update; the script detects your OS and architecture and downloads the latest release from GitHub):

::: code-group

```bash [Linux / macOS]
curl -fsSL https://frp.sh/install.sh | sh
```

```powershell [Windows (PowerShell)]
irm https://frp.sh/install.ps1 | iex
```

:::

After installing, run `frp-sh` and follow the interactive setup wizard to configure your signaling server.
