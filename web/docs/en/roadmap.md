# Roadmap

The current version focuses on "simple, robust, good enough". Planned capabilities, in priority order.

## Near term

- [ ] **Concurrent multi-connection**: carry multiple local TCP connections over one tunnel simultaneously (currently sequential)
- [ ] **WebSocket signaling**: instant guest-join notification to the host instead of polling (lower direct-link latency)
- [ ] **UPnP automatic port mapping**: an additional option before punching, when the router supports it

## Mid term

- [ ] **x25519 key exchange**: automatic per-session key agreement (replacing manual `--key`)
- [ ] **Encrypted relay**: confidentiality on the relay path too (currently `--key` applies to P2P direct only)
- [ ] **Multi-node rooms (>2 people)**: a central node broadcasts member addresses for mesh P2P
- [ ] **Adaptive transport parameters**: auto-tune window, retransmit interval, and MTU from loss/latency

## Long term

- [ ] **Platform polish**: Windows/macOS installers, autostart, tray icon
- [ ] **Relay cluster**: multi-server relay pool with nearest routing
- [ ] **Compatibility layer**: optional integration with existing protocol ecosystems

## Implemented (v0.1)

-  Room-based signaling (REST + UDP probe + TTL expiry)
-  UDP hole punching (PUNCH/ACK simultaneous handshake + port spread)
-  FRS1 reliable stream (sliding window + retransmit + keepalive + FIN handshake)
-  ChaCha20-Poly1305 end-to-end encryption (`--key`)
-  Sequential multi-connection reuse (`--max-conns`)
-  TCP relay fallback (pairing + bidirectional copy + late-direct re-check)
-  Auto-reconnect with heartbeat liveness (stream-level 10s no-frame detection;
  exponential backoff + public-address refresh + punch window reopened on address
  change; short TCP keepalive on the relay path)
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
