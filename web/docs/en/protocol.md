# Protocol v3

This is an implementation overview for 0.5.4, not a complete independent wire-format specification. Compatibility work should also follow the version-tagged sources: `src/signaling/mod.rs`, `src/signaling/server.rs`, `src/p2p/relay.rs`, `src/p2p/enc.rs`, `src/p2p/stream.rs`, and `src/services.rs`.

## HTTP signaling

`GET /health` returns `ok`. `GET /version` returns application version, protocol number, authentication status and, from 0.5.4, configured `limits`. These two routes do not require authentication.

Other routes use `X-Frp-Sh-Token`: the server password or an applicable room access token. Room access tokens cannot authorize room creation. Owner mutations additionally require `X-Frp-Sh-Room-Token`.

| Route | Purpose |
| --- | --- |
| `POST /room/create` | Register a room; returns `room_id`, `host_addr`, `owner_token` |
| `POST /room/{id}/join` | Register/update a visitor; returns assigned IP and display name |
| `GET /room/{id}` | Room snapshot, including guest list and published services |
| `POST /room/{id}/refresh` | Owner address/candidate refresh |
| `POST /room/{id}/secure` | Owner creates/rotates scoped room access credentials |
| `POST /room/{id}/services` | Owner updates the service catalog |
| `DELETE /room/{id}` | Owner deletes the room |

Create requests include `prefix`, `ttl` and `addr`; optional adapter, candidate and version fields are defined in the request structs. Default room codes have four digits; prefixes are optional. `visitor_id` is the stable reconnect identifier. Room-scoped tokens and owner tokens are distinct and must not be logged.

Errors include 400 invalid metadata, 401 authentication failure, 403 owner permission failure, 404 missing/expired room, 409 address conflict, and 429 admission capacity reached. Counts include owners; reconnecting with the same registered identity reuses a slot. HTTP itself is not encrypted; deploy HTTPS.

## UDP discovery and transport

Public discovery uses `ECHO <token>` and `ADDR <token> <ip>:<port>`. Punching exchanges `PUNCH <token>` and `ACK <token>`. The reliable UDP stream uses FRS1 framing. See `src/p2p/stream.rs` for sequence, acknowledgement, retransmission and framing rules. Service multiplexing is a separate format in `src/services.rs`; it must not be implemented as the legacy CNEW sequential forwarding protocol.

## TCP relay

For a secured room, the client first sends `R3 <room_id>\n` so the server selects the room access credential, then establishes the credential-derived encrypted stream. Inside that stream:

```text
HELLO2 <room_id> <HOST|GUEST> <visitor_id> <owner_token-or-dash>\r\n
```

Hosts supply the owner token; guests supply `-`. The matching visitor ID identifies a pairing slot. `WAIT` means registered and waiting; it does not mean the peer is already connected. `OK` indicates pairing. Pending pair slots expire after 15 seconds. Invalid authentication/handshakes may close the connection; do not depend on an old fixed `ERROR` vocabulary. Consult the relay client/server sources for encryption framing and shutdown behavior.

## TURN

The implementation supports a UDP subset of STUN/TURN, with authenticated allocation and permission operations. Built-in TURN destinations are restricted to active local relay endpoints. TURN authentication is not payload end-to-end encryption. `--relay` selects TCP and skips TURN.
