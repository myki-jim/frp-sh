# Roadmap

## Available in 0.5.4

- Keyboard-operated terminal app, profiles, personal room presets and invitations.
- LAN by default; game/dev parameter presets and explicit TCP/UDP service publishing.
- Concurrent service multiplexing with bounded per-member and per-room streams.
- Installation-time privileged helper setup and ordinary-user room operations.
- Separate log pages, files and CLI access.
- Configurable server room/member registration limits.

Web panels were removed in 0.4.0. Terminal pets were removed in 0.5.3.

## Further work

- Measure capacity under concurrent joins, reconnects and relay load.
- Reduce signaling polling through adaptive intervals or event notifications.
- Relay bandwidth fairness, rate limits and traffic budget visibility.
- Reliable member departure/expiry semantics before treating quotas as live online counts.
- Broader real-device and network testing, including OpenWrt.

These are development directions, not shipped capabilities or delivery dates.
