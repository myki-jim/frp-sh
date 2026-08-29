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
    details: A game-xxxxxx room code is all you need — no public IP or port forwarding
  - title:  UDP hole punching
    details: STUN-lite public probing + simultaneous PUNCH/ACK handshake, works through restricted cone NAT
  - title:  End-to-end encryption
    details: A matching --key passphrase enables ChaCha20-Poly1305 transport
  - title:  Relay fallback
    details: Automatic TCP relay when punching fails; late-direct re-check heals asymmetric cases
  - title:  Multi-connection reuse
    details: One session carries sequential TCP connections; reconnect in seconds
  - title:  Single static binary
    details: ~5 MB pure-Rust binary, no frp / libp2p / webrtc dependencies
---

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
