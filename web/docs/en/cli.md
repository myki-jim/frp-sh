# CLI Reference

Every command supports `--help`. Global options come before the subcommand.

## Global options

| Option | Description |
|--------|-------------|
| `-c, --config <FILE>` | TOML config file path (see [Configuration](./config)) |
| `-v, --verbose` | debug logging (RUST_LOG=debug, includes frame-level traces) |

```bash
frp-sh --config config/server.toml --verbose game create
```

## `frp-sh serve` — start the signaling server

```bash
frp-sh serve [--addr <addr>] [--relay-addr <addr>]
```

| Option | Default | Description |
|--------|---------|-------------|
| `--addr` | `0.0.0.0:8080` | HTTP REST + UDP probe listen address |
| `--relay-addr` | `0.0.0.0:8081` | TCP relay listen address |

`Ctrl-C` shuts down gracefully.

## `frp-sh game create` — host creates a room

```bash
frp-sh game create [options]
```

| Option | Default | Description |
|--------|---------|-------------|
| `-p, --prefix` | `game` | room prefix (room code = `prefix-6hex`) |
| `-t, --ttl <sec>` | `43200` (12h) | room lifetime in seconds |
| `--service <addr>` | `127.0.0.1:25565` | host's local service address |
| `--relay` | off | skip punching, use relay directly |
| `--key <passphrase>` | none | end-to-end encryption (both sides must match) |
| `--max-conns <N>` | `0` (unlimited) | max connections per session |
| `--spread <N>` | `2` | punch port spread (±N ports) |

Examples:

```bash
# minimal
frp-sh game create

# full: encryption + 5-connection cap + wider spread
frp-sh game create --service 127.0.0.1:25565 --key mypass --max-conns 5 --spread 3
```

## `frp-sh game join <room_id>` — guest joins a room

```bash
frp-sh game join <ROOM_ID> [options]
```

| Option | Default | Description |
|--------|---------|-------------|
| `room_id` | (required) | room code, e.g. `game-a3f9c2`; format validated as `prefix-6hex` |
| `-r, --relay` | off | force relay mode (skip punching) |
| `--listen <addr>` | `127.0.0.1:25565` | local listen address for the guest |
| `--key <passphrase>` | none | encryption passphrase matching the host |
| `--max-conns <N>` | `0` (unlimited) | max connections per session |
| `--spread <N>` | `2` | punch port spread |

Examples:

```bash
frp-sh game join game-a3f9c2
frp-sh game join game-a3f9c2 --listen 127.0.0.1:30000 --key mypass
frp-sh game join game-a3f9c2 --relay     # force relay
```

## Session output meanings

| Output | Meaning |
|--------|---------|
| `Room created : game-a3f9c2` | host room ready |
| `>>> P2P direct link established with <addr>` | **punch succeeded**, P2P direct |
| `>>> UDP hole punching failed, falling back to relay ...` | punching failed, switching to relay |
| `>>> late P2P link established with <addr>` | direct link re-captured while waiting on relay |
| `connection N from <addr>` | guest side: a local connection entered the tunnel |
| `guest connection N, dialing local service ...` | host side: guest connected, dialing local service |
| `connection N closed` | a tunnel connection ended normally |
| `max connections (N) reached, ending session` | `--max-conns` exhausted, session ends |
| `session ended by peer` | the peer closed the session |

## Exiting

- **Host**: `Ctrl-C` ends the session and deletes the room
- **Guest**: `Ctrl-C` ends the session; or it ends automatically when `--max-conns` is exhausted

## Common errors

| Error | Cause & fix |
|-------|-------------|
| `invalid room id: xxx` | bad room format (needs `prefix-6hex`) |
| `room not found or expired: xxx` | room missing/expired; ask the host to create a new one |
| `UDP echo timed out` | server 8080/udp not opened, or wrong config address |
| `decryption failed (wrong --key?)` | the two sides' `--key` do not match |
| `relay rejected: ERROR ROOM_EXPIRED` | room expired, relay rejected |
| `cannot reach local service xxx` | host's local service not running or wrong address |
