# Roadmap

The current version focuses on "simple, robust, good enough". Planned capabilities, in priority order.

## Near term

- [ ] **Relay ↔ direct seamless switching**: while on a relay (TURN / private TCP),
  periodically re-evaluate and upgrade to direct as soon as it recovers
  (UDP-based data plane makes switching cheap)
- [ ] **TURN provider discovery**: the signaling server publishes an available TURN
  list; clients pick automatically
- [ ] **Concurrent multi-connection**: carry multiple local TCP connections over one tunnel simultaneously (currently sequential)
- [ ] **WebSocket signaling**: instant guest-join notification to the host instead of polling (lower direct-link latency)
- [ ] **UPnP automatic port mapping**: an additional option before punching, when the router supports it

## Mid term

- [ ] **x25519 key exchange**: automatic per-session key agreement (replacing manual `--key`)
- [ ] **Encrypted relay path**: end-to-end confidentiality on the private TCP relay path
  (currently the TCP relay only has password-grade stream encryption; `--key` applies to
  the UDP data plane: direct and TURN)
- [ ] **Adaptive transport parameters**: auto-tune window, retransmit interval, and MTU from loss/latency

## Long term

- [ ] **Platform polish**: Windows/macOS installers, autostart, tray icon
- [ ] **Relay cluster**: multi-server relay pool with nearest routing
- [ ] **Compatibility layer**: optional integration with existing protocol ecosystems

## Implemented (v0.2)

-  TURN relay (v0.2 main line):
  - Built-in TURN server (`serve --turn`, RFC 5766 UDP subset, auth reuses `--password`)
  - Client supports **multiple TURN providers** (`turn_providers`: built-in /
    self-hosted coturn / Cloudflare TURN, etc.)
  - **Automatic best-path selection**: parallel Allocate at connect time, pick
    the fastest reachable provider
  - **TURN fallback chain**: on punch failure (incl. `--relay`) → TURN relay
    (relay addresses exchanged via signaling) → private TCP as last resort
  - STUN public-address learning (`stun_addr`, free, no custom UDP probe needed)
  - TODO: in-session upgrade/downgrade (relay ↔ direct seamless switching),
    TURN provider discovery

## Implemented (v0.1)

-  Room-based signaling (REST + UDP probe + TTL expiry)
-  UDP hole punching (PUNCH/ACK simultaneous handshake + port spread)
-  FRS1 reliable stream (sliding window + retransmit + keepalive + FIN handshake)
-  ChaCha20-Poly1305 end-to-end encryption (`--key`)
-  Sequential multi-connection reuse (`--max-conns`)
-  TCP relay fallback (pairing + bidirectional copy + late-direct re-check)
-  Auto-reconnect with heartbeat liveness (stream-level 3s no-frame detection,
  tunable via `FRPSH_LIVENESS_MS`; exponential backoff + public-address refresh
  + punch window reopened on address change; short TCP keepalive on the relay path)
-  Multi-guest mesh (`lan`: 1 host + N guests fully interconnected; the host acts
  as a hub forwarding by destination IP; per-guest direct-first / relay-fallback
  links with UUID-keyed relay pairing)
-  Same-LAN auto-direct (advertises LAN addresses, dual-path punching)
-  Mesh mode (`lan` series: virtual-NIC whole-machine mesh, 10.66.0.0/24; guest can reach the host's LAN)
-  Per-device unique ID (UUID) + stable derived virtual IP + host IP pool (`--guest-ips`)
-  Server password auth (`serve --password`: 401 request checks + relay auth)
-  Relay traffic encryption (ChaCha20-Poly1305 stream, anti-eavesdropping)
-  Version governance: protocol conflict control (`/version`), startup update
  checks (skippable / big-gap forced), server & host version yellow/red hints
-  LAN exposure off by default (`--expose-lan` opts in)
-  Windows compatibility (ICMP poisoning, TUN/WinTun, auto UAC elevation, firewall auto-allow)
-  macOS utun compatibility (point-to-point route fix)
-  English CLI with colors, ASCII logo, interactive installer setup
-  End-to-end tests and real-NAT verification

## Contributing

PRs welcome. See [Development & Testing](./develop) and the [Protocol Spec](./protocol).
