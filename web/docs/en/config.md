# Configuration

frp-sh uses a TOML config file (specified with `--config`). Without one, built-in defaults apply (pointing at `127.0.0.1`, for local testing only).

## First-run wizard

Running `frp-sh` with no arguments (or `frp-sh config`) opens an **interactive setup wizard**:

```text
  frp-sh setup wizard
  ==================
  1. Signaling server address (HTTP) [default http://127.0.0.1:8080]
  > http://101.43.41.195:8080
  2. Relay server address (TCP) [default 101.43.41.195:8081]
  >
  3. Separate UDP probe port? (y/N)
```

- The `http://` prefix is optional (auto-completed)
- The relay address defaults to the signaling host + `:8081`; just press Enter
- After saving, a quick connectivity check (`/health`) runs automatically
- The config is saved to the platform default location and loaded by every command afterwards:
  - Windows: `%APPDATA%\frp-sh\config.toml`
  - Linux/macOS: `~/.config/frp-sh/config.toml`
- Re-configure with `frp-sh config`, or use `--config <path>` for an explicit file

## Fields

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `signaling_addr` | no | `http://127.0.0.1:8080` | signaling server HTTP base URL |
| `relay_addr` | no | `127.0.0.1:8081` | relay TCP address |
| `signaling_udp` | no | same port as HTTP | UDP probe address (when using a separate port) |

## Example: connect to a remote server

```toml
# config/server.toml
signaling_addr = "http://101.43.41.195:8080"
relay_addr     = "101.43.41.195:8081"
```

Usage:

```bash
frp-sh --config config/server.toml game create
frp-sh --config config/server.toml game join game-a3f9c2
```

## Example: separate UDP probe port

Use this when a cloud firewall requires separate TCP/UDP ports:

```toml
signaling_addr = "http://101.43.41.195:8080"
relay_addr     = "101.43.41.195:8081"
signaling_udp  = "101.43.41.195:8082"   # UDP probe on 8082
```

The server side must then provide the UDP probe on `--addr 0.0.0.0:8082` (or a compatible endpoint).

## Field details

### signaling_addr

REST API base URL:

- `POST /room/create` — register a room
- `GET /room/{id}` — query a room
- `POST /room/{id}/join` — guest registration
- `DELETE /room/{id}` — close a room

### signaling_udp

UDP probe endpoint: the client sends `ECHO <token>`; the server replies `ADDR <token> <ip>:<port>`, which tells the client its NAT-mapped public address.

> Probing must use the **same socket as punching** (same local port), so the advertised address is the mapping that punching can actually use.

### relay_addr

The TCP relay endpoint used when punching fails. After connecting, send `HELLO <room_id> <HOST|GUEST>` to pair.

## Precedence

```text
command-line options > config file > built-in defaults
```

Currently the `serve` listen addresses are controlled by command-line options only; client addresses come from the config file.
