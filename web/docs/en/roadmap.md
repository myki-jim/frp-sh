# Roadmap

## Available in 0.6.0

- Keyboard-operated terminal app, profiles, personal room presets and invitations.
- LAN by default; game/dev parameter presets and explicit TCP/UDP service publishing.
- Concurrent service multiplexing with bounded per-member and per-room streams.
- Installation-time privileged helper setup and ordinary-user room operations.
- Separate log pages, files and CLI access.
- Configurable server room/member registration limits.
- SQLite-backed durable spaces, device identities, 15-minute invitations, and encrypted member relay.
- Native Windows SCM, Linux systemd, and macOS LaunchDaemon supervision entry points.
- DNS ownership verification, custom-domain HTTP publishing, and On-Demand TLS on port 18443.

Web panels were removed in 0.4.0. Terminal pets were removed in 0.5.3.

## Further work

- Measure capacity under concurrent joins, reconnects and relay load.
- Reduce signaling polling through adaptive intervals or event notifications.
- Relay bandwidth fairness, rate limits and traffic budget visibility.
- Reliable member departure/expiry semantics before treating quotas as live online counts.
- Broader real-device and network testing, including OpenWrt.
- Per-domain bandwidth budgets, rate limits, WebSocket support, and path routing.

These are development directions, not shipped capabilities or delivery dates.
