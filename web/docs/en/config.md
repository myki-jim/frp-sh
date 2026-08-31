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
  >
  4. Server password (optional, fill in when the server runs with --password)
  > 
  5. STUN server (optional, e.g. stun.cloudflare.com:3478)
  > 
```

- The `http://` prefix is optional (auto-completed)
- The relay address defaults to the signaling host + `:8081`; just press Enter
- Step 4's password must match the server's `--password`; step 5's STUN gives more accurate public-address learning
- After saving, a quick connectivity check (`/health`) runs automatically
- The config is saved to the platform default location and loaded by every command afterwards:
  - Windows: `%APPDATA%\frp-sh\config.toml`
  - Linux/macOS: `~/.config/frp-sh/config.toml`
- Re-configure with `frp-sh config`, or use `--config <path>` for an explicit file

## Fields

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `signaling_addr` | no | `http://127.0.0.1:8080` | signaling server HTTP base URL |
| `relay_addr` | no | `127.0.0.1:8081` | relay TCP address (private TCP fallback when both punching and TURN fail) |
| `signaling_udp` | no | same port as HTTP | UDP probe address (when using a separate port) |
| `password` | no | none | server password (required when the server runs `serve --password`) |
| `stun_addr` | no | none | STUN server (e.g. `stun.cloudflare.com:3478`); public-address learning prefers STUN, falling back to the built-in UDP probe |
| `turn_providers` | no | empty | list of TURN providers (`turn://user:pass@host:port`); when punching fails, relays via the fastest one by RTT automatically |

> `uuid` (device unique ID) is not stored in the config file; it lives separately at `%APPDATA%\frp-sh\identity` and is used to derive the stable virtual IP (`lan` series) and UUID-keyed relay pairing.

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

## Example: TURN providers (automatic TURN when punching fails)

Once the server has built-in TURN enabled (`serve --turn 0.0.0.0:3478 --password YOUR_PASSWORD`), configure the client:

```toml
signaling_addr = "http://101.43.41.195:8080"
relay_addr     = "101.43.41.195:8081"
password       = "YOUR_PASSWORD"                          # must match the server
turn_providers = ["turn://frp-sh:YOUR_PASSWORD@101.43.41.195:3478"]
```

- You can configure **multiple** providers (built-in TURN / self-hosted coturn / Cloudflare TURN, etc.); the client benchmarks them in parallel and automatically picks the one with the lowest RTT
- When punching fails (including forced `--relay`), it tries in order: TURN relay → private TCP fallback
- The username is fixed to `frp-sh` (a built-in TURN server convention); self-hosted coturn can use a custom username/password

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

The private TCP relay endpoint used when punching fails (when `turn_providers` is configured, TURN takes precedence over it). After connecting, send `HELLO <room_id> <HOST|GUEST>` to pair.

### password

The server password. When the server runs with `serve --password <passphrase>`, clients must configure the same password; otherwise signaling requests get 401 and the relay is refused.

### stun_addr

Optional STUN server (e.g. `stun.cloudflare.com:3478`). Public-address learning (`learn_public_addr_auto`) **prefers STUN** (RFC 5389 Binding) and falls back to the built-in UDP probe (`ECHO`/`ADDR`) when STUN fails. The server's own `--addr` UDP probe still works as a fallback.

### turn_providers

List of TURN providers, each formatted as `turn://[user:pass@]host:port`. The client connects to all providers in parallel and benchmarks them with an Allocate request, picking the one with the lowest RTT; when punching fails it exchanges relay addresses over TURN to build the data plane (`create_permission` + FRS1 over TURN). If TURN is unavailable too, it falls back to the `relay_addr` private TCP relay.

## Precedence

```text
command-line options > config file > built-in defaults
```

Currently the `serve` listen addresses are controlled by command-line options only; client addresses come from the config file.
