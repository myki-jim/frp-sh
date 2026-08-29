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
| `--service <addr>` | `127.0.0.1:25565` | host's local service address (25565 is the Minecraft default port; use any port) |
| `--relay` | off | skip punching, use relay directly |
| `--key <passphrase>` | none | end-to-end encryption (both sides must match) |
| `--max-conns <N>` | `0` (unlimited) | max connections per session |
| `--spread <N>` | `2` | punch port spread (±N ports) |
| `--tun` | off | virtual NIC mode (layer-2 tunnel; peer can reach your virtual IP) |
| `--tun-ip <IP>` | `10.66.0.1` | virtual NIC IP (stable address in the same subnet) |
| `--tun-netmask <mask>` | `255.255.255.0` | virtual NIC netmask |
| `--tun-mtu <N>` | `1400` | virtual NIC MTU |

Examples:

```bash
# minimal
frp-sh game create

# full: encryption + 5-connection cap + wider spread
frp-sh game create --service 127.0.0.1:25565 --key mypass --max-conns 5 --spread 3

# virtual NIC mode (friends can ping 10.66.0.1 to reach you)
frp-sh game create --tun
```

## `frp-sh game join <room_id>` — guest joins a room

```bash
frp-sh game join <ROOM_ID> [options]
```

| Option | Default | Description |
|--------|---------|-------------|
| `room_id` | (required) | room code, e.g. `game-a3f9c2`; format validated as `prefix-6hex` |
| `-r, --relay` | off | force relay mode (skip punching) |
| `--listen <addr>` | `127.0.0.1:25565` | local listen address for the guest (25565 is the Minecraft default port; use any port) |
| `--key <passphrase>` | none | encryption passphrase matching the host |
| `--max-conns <N>` | `0` (unlimited) | max connections per session |
| `--spread <N>` | `2` | punch port spread |
| `--tun` | off | virtual NIC mode (IP derived stably from your device ID, e.g. `10.66.0.42`) |
| `--tun-ip <IP>` | auto-derived | custom virtual NIC IP (same subnet as host) |
| `--tun-netmask <mask>` | `255.255.255.0` | virtual NIC netmask |
| `--tun-mtu <N>` | `1400` | virtual NIC MTU |

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
| `Your ID      : <uuid>` | your device unique ID (stored in `%APPDATA%\frp-sh\identity`; derives your stable virtual IP) |
| `LAN addrs    : 192.168.1.5:51234` | host LAN addresses (same-LAN guests connect directly) |
| `LAN subnets  : 192.168.1.0/24` | host LAN subnets (reachable by guests in `--tun` mode) |
| `Vnet IP      : 10.66.0.x` | your virtual NIC IP (friends can use this IP to reach you long-term) |
| `>>> 本地局域网直连 (LAN direct) with <addr>` | **same-WiFi/LAN direct link** (no server involved, lowest latency) |
| `>>> P2P direct link established with <addr>` | **punch succeeded**, P2P direct |
| `>>> UDP hole punching failed, falling back to relay ...` | punching failed, switching to relay |
| `>>> late P2P link established with <addr>` | direct link re-captured while waiting on relay |
| `connection N from <addr>` | guest side: a local connection entered the tunnel |
| `guest connection N, dialing local service ...` | host side: guest connected, dialing local service |
| `connection N closed` | a tunnel connection ended normally |
| `max connections (N) reached, ending session` | `--max-conns` exhausted, session ends |
| `session ended by peer` | the peer closed the session |
| `>>> 连接已断开，N 秒后自动重连...` | link dropped, auto-reconnecting with backoff (2s, 4s, 8s... capped at 15s) |

## Local network topology support

**Same-LAN auto-direct**: the host advertises all of its LAN addresses when creating a
room; the guest punches at both the public address and the LAN addresses simultaneously.
When both sides are on the same WiFi/wired LAN, a direct LAN link is established in
seconds (`LAN direct` output) with no server in the path; otherwise the public punch
path is used, falling back to relay only if punching fails.

**Guest reaches the host's whole LAN (`--tun`)**: the host advertises its LAN subnets
(e.g. `192.168.1.0/24`) in the room; in `--tun` mode the guest automatically adds
routes for those subnets via its virtual NIC, so it can reach other devices on the
host's LAN (NAS, printers, other PCs):

```bash
# host (needs root/admin; enables IPv4 forwarding automatically)
frp-sh game create --tun
# → LAN subnets  : 192.168.1.0/24

# guest (needs root/admin; adds routes automatically)
frp-sh game join game-a3f9c2 --tun
# → 路由 192.168.1.0/24 → frp1 已添加
# now you can ping / access devices on the host's LAN
```

Notes:

- root/admin is required (routing changes, forwarding)
- if the guest's own LAN is on the same subnet as the host's (e.g. both
  `192.168.1.0/24`), that subnet is skipped automatically to avoid route conflicts
  (the output notes `跳过与本地同网段的房主子网`)
- you reach the host's **current** LAN; if the host changes networks, recreate the room

## Exiting

- **Auto-reconnect**: dropped links (network jitter, expired NAT mappings, service restarts) reconnect automatically
- **Host**: `Ctrl-C` ends the session and deletes the room; the session also ends when the room expires
- **Guest**: `Ctrl-C` ends the session; it also ends when the room is deleted or expires

## Common errors

| Error | Cause & fix |
|-------|-------------|
| `invalid room id: xxx` | bad room format (needs `prefix-6hex`) |
| `room not found or expired: xxx` | room missing/expired; ask the host to create a new one |
| `UDP echo timed out` | server 8080/udp not opened, or wrong config address |
| `decryption failed (wrong --key?)` | the two sides' `--key` do not match |
| `relay rejected: ERROR ROOM_EXPIRED` | room expired, relay rejected |
| `cannot reach local service xxx` | host's local service not running or wrong address |
