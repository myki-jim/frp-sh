# Quickstart

From binary to your first tunnel in about five minutes. You need three things:

1. A server with a public IP (any VPS)
2. The host computer (the one running the game/service)
3. The guest computer (a friend's machine)

## Step 1: Deploy the signaling server

On the public server:

```bash
# Build (slow the first time)
cargo build --release

# Start the signaling server
./target/release/frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081
```

Three services come up together:

- **HTTP REST** (8080/tcp): room registration and lookup
- **UDP public probe** (8080/udp): clients learn their public address
- **TCP relay** (8081/tcp): data forwarding when punching fails

> Open `8080/tcp`, `8080/udp`, and `8081/tcp` in both the cloud security group and the OS firewall.

For a permanent setup see [Deploy the Server](./server).

## Step 2: Configure the client

Both host and guest need a config pointing at that server (the built-in default targets `127.0.0.1` for local testing only):

```toml
# config/server.toml
signaling_addr = "http://YOUR-SERVER-IP:8080"   # HTTP signaling
relay_addr     = "YOUR-SERVER-IP:8081"          # TCP relay
```

Use it with `--config`:

```bash
frp-sh --config config/server.toml lan create
```

## Step 3: Pick your series

frp-sh has three usage series. This tutorial leads with **mesh (`lan`)**, the most
powerful, most general mode:

| Series | Command | Use case |
|--------|---------|----------|
| **Mesh (recommended)** | `frp-sh lan create/join` | reach the peer's whole machine + their entire LAN (Tailscale-like) |
| **Game** | `frp-sh game create/join` | multiplayer games like Minecraft, pure port forwarding (default 25565) |
| **Dev** | `frp-sh dev create/join` | development, application-level port forwarding |

### Mesh: host creates a room (`lan`)

On the host machine:

```bash
frp-sh lan create
```

Example output:

```text
  Room created : lan-a3f9c2
  Signaling    : http://YOUR-SERVER-IP:8080
  Your ID      : 123e4567-e89b-12d3-a456-426614174000
  LAN addrs    : 192.168.1.5:51234
  Vnet IP      : 10.66.0.1（对端可 ping/直连此 IP）
  Mode         : LAN mesh (virtual NIC)
  LAN subnets  : 192.168.1.0/24（访客加入后可访问）
  Waiting for a guest to join ...
```

Send **`lan-a3f9c2`** to your friend.

### Mesh: guest joins (`lan`)

On the friend's machine:

```bash
frp-sh lan join lan-a3f9c2
```

The guest gets a stable virtual IP derived from its device ID (e.g. `10.66.0.42`).
After joining:

- `ping 10.66.0.1` / SSH / file sharing reach the host's whole machine
- routes are added automatically to reach the host's LAN devices (NAS, printers, etc.)
- root/admin is required (virtual NIC creation, routing changes)

```text
  Joined room : lan-a3f9c2
  Host address: YOUR-SERVER-IP:xxx
  Host vnet IP: 10.66.0.1
  Host LAN     : 192.168.1.0/24
  Mode         : LAN mesh (virtual NIC)
  Punching through NAT ...

>>> 本地局域网直连 (LAN direct) with 192.168.1.5:51234   ← same-WiFi instant link!
```

## Step 4: Game / Dev series (pure port forwarding)

If you only need to forward one port, use `game` (games) or `dev` (development):

```bash
# host: forward local Minecraft (default 25565)
frp-sh game create --service 127.0.0.1:25565

# guest: connecting to local 25565 reaches the host's game server
frp-sh game join game-a3f9c2 --listen 127.0.0.1:25565
```

```bash
# dev: share a local web service on 8080 with a teammate
frp-sh dev create --service 127.0.0.1:8080
frp-sh dev join dev-a3f9c2 --listen 127.0.0.1:8080
```

`game` / `dev` are pure port forwarding — no virtual NIC, no admin rights needed.

If punching fails, it falls back automatically:

```text
>>> UDP hole punching failed, falling back to relay ...
>>> relay connected, waiting for host ...
```

The tunnel still works — traffic just goes through the server (see [Architecture](./architecture)).

After a drop (network jitter, expired NAT mappings), both sides reconnect automatically:

```text
>>> 连接已断开，2 秒后自动重连（Ctrl-C 退出）...
```

No manual action needed; the retry backoff is 2s, 4s, 8s... capped at 15s.

## Minimal Commands

| Role | Minimal command | Notes |
|------|-----------------|-------|
| Mesh host | `frp-sh lan create` | virtual-NIC whole-machine mesh (needs root/admin) |
| Mesh guest | `frp-sh lan join lan-xxxxxx` | only the room code is required |
| Game host | `frp-sh game create` | pure port forwarding (default 25565) |
| Server | `frp-sh serve` | listens on 0.0.0.0:8080/8081 |

## Next Steps

- [CLI Reference](./cli) for all options
- [Advanced Usage](./advanced) for encryption, multi-connection, etc.
- [Troubleshooting FAQ](./faq) if you hit problems
