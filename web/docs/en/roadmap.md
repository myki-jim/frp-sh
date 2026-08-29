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
-  Windows compatibility (ICMP poisoning handling)
-  End-to-end tests and real-NAT verification

## Contributing

PRs welcome. See [Development & Testing](./develop) and the [Protocol Spec](./protocol).
