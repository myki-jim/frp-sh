# CLI Reference

frp-sh ships three usage series, pick by scenario:

| Series | Command | Use case | Highlights |
|--------|---------|----------|------------|
| **Mesh** | `frp-sh lan` | general LAN/inner-network environments (Tailscale-like) | virtual-NIC whole-machine mesh; peers reach each other's machines and whole LAN |
| **Game** | `frp-sh game` | multiplayer games like Minecraft | pure port forwarding, default 25565, zero-config |
| **Dev** | `frp-sh dev` | development, arbitrary TCP services | application-level port forwarding |

- **lan is the most powerful, most general mode**: the virtual NIC is on by default;
  both sides build a layer-2 tunnel and can reach each other's whole machine
  (ping/SSH/file sharing), plus the peer's entire LAN (NAS, printers, etc.).
- **game / dev are pure port forwarding**: forward one local port to the peer;
  no mesh, no virtual NIC.

This document covers every command and every parameter. Every command supports
`--help`. Global options go before the subcommand, for example:

```bash
frp-sh --config config/server.toml --verbose lan create
```

---

## Global options

Global options apply to the whole `frp-sh` invocation and go before the subcommand
(`serve` / `game` / `dev` / `lan` / `config`).

### `-c, --config <FILE>`

**Purpose**: use a specific TOML config file instead of the defaults.

- When omitted, the lookup order is:
  1. the path from `-c`
  2. the platform default path (`%APPDATA%\frp-sh\config.toml` on Windows,
     `~/.config/frp-sh/config.toml` on Linux/macOS)
  3. built-in defaults (signaling `http://127.0.0.1:8080`, relay `127.0.0.1:8081`)
- Config file format is documented in [Configuration](./config)

**Examples**:

```bash
frp-sh -c config/server.toml lan create
frp-sh --config /etc/frp-sh.toml serve
```

### `-v, --verbose`

**Purpose**: enable debug logging (`RUST_LOG=debug`) with frame-level traces.

- Add it when troubleshooting punching, forwarding, or encryption
- Without it, only `info`-level logs are shown
- Logs go to `<config dir>/logs/frp-sh.log` (terminal stays quiet); the panel's Logs view shows them live

**Example**:

```bash
frp-sh --verbose lan join lan-a3f9c2
```

---

### `--punch-retries <N>`

**Purpose**: how many UDP hole-punch rounds to try before falling back to the
TCP relay (default **1**). After the limit, punching **and** TURN are skipped
entirely and every reconnect goes straight to the relay.

- Default 1: one failed punch permanently downgrades to relay — no repeated
  retries over the same failing path
- `--punch-retries 3`: tolerate 3 failed rounds before downgrading
- `--punch-retries 0`: never punch, always relay
- Unaffected: a direct link that drops after being established still skips
  punching on the next round automatically

**Example**:

```bash
frp-sh --punch-retries 3 lan join 7411
```

---

## `frp-sh profile` — manage connection profiles

Save "server + password + room + mode" as a named profile and reconnect with
one command. The server panel's "one-click client setup / join" commands use
these under the hood; **adding the same server+mode twice dedupes into one
profile** (setup first, then joining a room fills the room into the same
profile).

```bash
# Add a profile (name defaults to profile1, profile2, ...; relay derived as :8081)
frp-sh profile add --server http://101.43.41.195:8080 --room 7411 --password XXXX

# Server-only profile (no room) — same as the panel's "one-click client setup"
frp-sh profile add --server http://101.43.41.195:8080 --password XXXX --set-default

# List / show (passwords masked)
frp-sh profile list
frp-sh profile show profile1

# Edit: rename, change room, device name, etc. (fields merge)
frp-sh profile edit profile1 --rename jims-phone --room 7411 --device JimmyPhone

# Remove
frp-sh profile remove profile1

# Start a session from a profile (default profile if omitted; lan needs admin)
frp-sh profile run
frp-sh profile run profile1
```

- `--mode`: `lan` (virtual NIC mesh, default) / `dev` / `game` (port forwarding)
- `--set-default`: mark as the default profile (used by `profile run` without a name)
- Passwords are always masked in `list` / `show`; plaintext lives only in the local `config.toml`

---

## `frp-sh serve` — start the signaling server

Run it on a public VPS to provide room registration, UDP public-address probing,
TCP relay forwarding, and the optional built-in TURN relay.

```bash
frp-sh serve [--addr <addr>] [--relay-addr <addr>] [--udp-addr <addr>] [--password <passphrase>] [--turn <addr>] [--external-ip <IP>]
```

`Ctrl-C` shuts down gracefully.

### `--addr <addr>`

**Purpose**: listen address for HTTP REST + UDP public-address probing.

- **Default**: `0.0.0.0:8080`
- The REST API (room create/join/query) and the UDP probe share this port
- Cloud firewalls must open **both TCP and UDP** on this port (e.g. `8080/tcp`
  and `8080/udp`)

**Examples**:

```bash
frp-sh serve --addr 0.0.0.0:9000          # use port 9000 instead
frp-sh serve --addr 127.0.0.1:8080        # localhost only (debugging)
```

### `--relay-addr <addr>`

**Purpose**: TCP relay listen address (the server that forwards traffic when
punching fails).

- **Default**: `0.0.0.0:8081`
- Independent of `--addr`; can share the same host on a different port
- Open this port in the firewall (e.g. `8081/tcp`)

**Example**:

```bash
frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:9001
```

### `--udp-addr <addr>`

**Purpose**: separate UDP probe listen address (optional).

- **Default**: same port as `--addr` (clients' `signaling_udp` must match)
- Only needed when your cloud firewall cannot open TCP and UDP on the same port
- When used, clients must set `signaling_udp` in config (wizard step 3)

**Example**:

```bash
frp-sh serve --addr 0.0.0.0:8080 --udp-addr 0.0.0.0:8082
# client config: signaling_udp = "SERVER_IP:8082"
```

### `--password <passphrase>`

**Purpose**: server password (optional). When set:

- **Request auth**: every signaling request is checked (`X-Frp-Sh-Token`);
  missing/wrong → 401
- **Relay auth + encryption**: relay connections must carry the password and the
  channel is encrypted with ChaCha20-Poly1305
- Clients must set the same `password` in config (wizard step 4)
- Without it, behavior is identical to older versions (old clients keep working)

**Example**:

```bash
frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 --password YOUR_PASSWORD
```

### `--turn <addr>`

**Purpose**: enable the **built-in TURN server** (RFC 5766 UDP subset, optional).

- **Default**: off (no TURN; clients fall back to the private TCP relay when punching fails)
- Give it a listen address to enable it, e.g. `--turn 0.0.0.0:3478`
- Auth reuses `--password`: the TURN username is fixed to `frp-sh` and the password matches the server password
- On the client side, just put `turn://frp-sh:<password>@<SERVER_IP>:3478` in the config's
  `turn_providers`; punching failures then switch to TURN automatically (no `--relay` needed)
- When enabled, the firewall must additionally open the TURN port (`3478/udp`) plus the relay port range handed out to clients
- You can also skip the built-in TURN and configure the official frp.sh server or a self-hosted coturn as a provider on the client

**Example**:

```bash
frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 --password YOUR_PASSWORD --turn 0.0.0.0:3478
```

### `--external-ip <IP>`

**Purpose**: the server's public IP (optional, used with `--turn`).

- **Default**: the IP of the `--turn` listen address; when listening on `0.0.0.0`, the machine's first LAN address
- Behind NAT / Docker the default may be a private address (e.g. `172.17.0.6`); use this
  option to specify the public IP explicitly so TURN advertises the correct relay addresses
- Normally used together with `--turn`; meaningless without it

**Example**:

```bash
frp-sh serve --turn 0.0.0.0:3478 --external-ip 101.43.41.195
```

---

## `frp-sh lan create` — mesh: host creates a room

Mesh mode (Tailscale-like), virtual NIC enabled by default. Run it on the host
machine to create a room and wait for guests.

**Multi-guest full mesh**: one room supports 1 host + any number of guests online
at the same time. Every guest gets a direct tunnel to the host (relay fallback
when punching fails); the host acts as a hub forwarding traffic between guests —
all members' virtual IPs (10.66.0.x) can reach each other (e.g. `ping 10.66.0.2`
reaches any guest's whole machine).

```bash
frp-sh lan create [options]
```

### `-p, --prefix <prefix>`

**Purpose**: room-code prefix; the code looks like `<prefix>-6hex` (e.g.
`lan-a3f9c2`).

- **Default**: `lan`
- Only lowercase alphanumerics and `-_` are kept, max 16 chars; falls back to
  `lan` when empty
- A custom prefix makes the room easier to identify

**Examples**:

```bash
frp-sh lan create --prefix home
frp-sh lan create --prefix my-team
```

### `-t, --ttl <seconds>`

**Purpose**: room lifetime in seconds; the room expires and both sessions end
afterwards.

- **Default**: `43200` (12 hours)
- Guests joining after expiry get `room not found or expired`
- Increase for long sessions, decrease for quick tests

**Examples**:

```bash
frp-sh lan create --ttl 3600        # 1 hour
frp-sh lan create --ttl 86400       # 24 hours
```

### `--relay`

**Purpose**: skip UDP hole punching and use the relay directly.

- **Default**: off (auto punching with relay fallback)
- Use when NAT is too strict, punching is guaranteed to fail, or you want traffic
  to always go through the server
- Forced relay follows the fallback chain: **TURN relay** (when `turn_providers` is configured) → private TCP relay
- The private TCP relay has password-derived stream encryption when the server sets `--password`; for end-to-end confidentiality use `--key` (applies to the UDP data plane: direct and TURN paths)

**Example**:

```bash
frp-sh lan create --relay
```

### `--key <passphrase>`

**Purpose**: end-to-end encryption passphrase (ChaCha20-Poly1305); both sides
must use the same passphrase.

- **Default**: none (unencrypted)
- Provides confidentiality only, not identity; a leaked passphrase lets anyone decrypt
- A mismatch produces `decryption failed (wrong --key?)`

**Example**:

```bash
frp-sh lan create --key "our-passphrase"
```

### `--spread <N>`

**Purpose**: punch port spread — also punch the peer's port ±N.

- **Default**: `2`
- Some NATs map consecutive ports consecutively; spreading raises the hit rate
- Increase (e.g. `5`) for symmetric-NAT scenarios, at the cost of more datagrams

**Example**:

```bash
frp-sh lan create --spread 5
```

### `--ip <IP>`

**Purpose**: the host's virtual NIC IP (mesh subnet default 10.66.0.0/24).

- **Default**: `10.66.0.1`
- Must be in the same subnet as the guest's virtual IP
- Keep it fixed so friends can reach you long-term

**Examples**:

```bash
frp-sh lan create --ip 10.66.0.1
frp-sh lan create --ip 10.66.0.10
```

### `--netmask <mask>`

**Purpose**: virtual NIC netmask.

- **Default**: `255.255.255.0` (/24)
- Both sides must use the same mask
- Usually no need to change

**Example**:

```bash
frp-sh lan create --netmask 255.255.0.0
```

### `--mtu <N>`

**Purpose**: virtual NIC MTU (bytes).

- **Default**: `1400`
- The tunnel has ~100 bytes of framing overhead; 1400 avoids fragmentation
- Lower it (e.g. `1300`) if you see packet loss

**Example**:

```bash
frp-sh lan create --mtu 1300
```

### `--guest-ips <IP1,IP2,...>`

**Purpose**: reserved guest virtual-IP pool (host-managed assignment).

- **Default**: empty (guests use their UUID-derived IP, or `--ip`)
- Comma-separated IPs, e.g. `10.66.0.2,10.66.0.3,10.66.0.4`
- When a guest joins **without `--ip`**, addresses are handed out in join order;
  the **same device (UUID) reuses the same IP across reconnects**
- Great for teams where the host wants to manage the vnet addressing

**Example**:

```bash
# host: reserve 3 guest addresses
frp-sh lan create --guest-ips 10.66.0.2,10.66.0.3,10.66.0.4

# guest joins (no --ip needed; auto-assigned, prints Assigned IP)
frp-sh lan join lan-a3f9c2
```

### `--expose-lan`

**Purpose**: bring your **local LAN into the tunnel** (the peer can reach devices on
your LAN, such as NAS or printers).

- **Default**: off. By default only the virtual subnet (`10.66.0.0/24`) is reachable
  between peers — your real LAN is **not exposed**
- When enabled: your LAN subnets are advertised (`LAN subnets`), IPv4 forwarding is
  turned on, and the peer automatically adds routes via its virtual NIC so it can
  reach devices on your LAN
- Requires root/admin (routing changes, enabling forwarding)

**Example**:

```bash
# host: open your home LAN (e.g. 192.168.1.0/24) to the guest
frp-sh lan create --expose-lan

# after joining, the guest can reach devices on the host's LAN
frp-sh lan join lan-a3f9c2
```

> Not exposing by default is safer; when both sides want to reach each other's LAN,
> each side adds `--expose-lan`.

### Combined examples

```bash
# minimal
frp-sh lan create

# encrypted mesh + custom subnet
frp-sh lan create --key mypass --ip 10.66.0.1 --netmask 255.255.255.0
```

The host advertises its LAN subnets (`LAN subnets`); guests can reach the host's
whole LAN after joining. See [Local network topology support](#local-network-topology-support).

---

## `frp-sh lan join <room_id>` — mesh: guest joins a room

Run it on the friend's machine to join the host's mesh with the room code.

```bash
frp-sh lan join <ROOM_ID> [options]
```

### `room_id` (positional, required)

**Purpose**: the room code given by the host.

- Format: `prefix-6hex` (e.g. `lan-a3f9c2`); anything else gives `invalid room id`
- Missing/expired rooms give `room not found or expired`
- Case-sensitive

**Examples**:

```bash
frp-sh lan join lan-a3f9c2
frp-sh lan join home-3f9c2a
```

### `--relay`

**Purpose**: force relay mode (skip punching).

- **Default**: off (auto punching with relay fallback)
- If the host created with `--relay`, the guest should also use it (or let it
  fall back automatically)

**Example**:

```bash
frp-sh lan join lan-a3f9c2 --relay
```

### `--key <passphrase>`

**Purpose**: the encryption passphrase matching the host.

- **Default**: none
- Required if the host used `--key`; a mismatch gives `decryption failed (wrong --key?)`

**Example**:

```bash
frp-sh lan join lan-a3f9c2 --key "our-passphrase"
```

### `--spread <N>`

**Purpose**: punch port spread (same meaning as on the host side).

- **Default**: `2`
- Keep it consistent with the host

**Example**:

```bash
frp-sh lan join lan-a3f9c2 --spread 5
```

### `--ip <IP>`

**Purpose**: the guest's virtual NIC IP.

- **Default**: **derived stably** from your device ID (UUID), e.g. `10.66.0.42` —
  the same device gets the same IP every time
- When set manually, it must be in the host's subnet (e.g. host `10.66.0.1`,
  guest `10.66.0.2`)
- Usually unnecessary — the derived IP is already in the same subnet

**Example**:

```bash
frp-sh lan join lan-a3f9c2 --ip 10.66.0.2
```

### `--netmask <mask>`

**Purpose**: virtual NIC netmask.

- **Default**: `255.255.255.0` (/24)
- Must match the host

**Example**:

```bash
frp-sh lan join lan-a3f9c2 --netmask 255.255.0.0
```

### `--mtu <N>`

**Purpose**: virtual NIC MTU.

- **Default**: `1400`
- Must match the host, otherwise large packets may fail

**Example**:

```bash
frp-sh lan join lan-a3f9c2 --mtu 1300
```

### `--expose-lan`

**Purpose**: bring your **local LAN into the tunnel** on the guest side too, so the
host can reach devices on your LAN.

- **Default**: off. By default only the virtual subnet is reachable
- Same behavior as on the host side: your LAN subnets are advertised, IPv4
  forwarding is enabled, and the host adds routes to your LAN automatically
- Requires root/admin

**Example**:

```bash
frp-sh lan join lan-a3f9c2 --expose-lan
```

### Combined examples

```bash
frp-sh lan join lan-a3f9c2
frp-sh lan join lan-a3f9c2 --key mypass
frp-sh lan join lan-a3f9c2 --relay     # force relay
```

After joining, the guest gets a stable virtual IP (`Vnet IP`) and can ping / reach
the host's whole machine; routes for the host's LAN are added automatically (see
[Local network topology support](#local-network-topology-support)).

---

## `frp-sh game create` — game: host creates a room

Pure port forwarding for multiplayer games (no mesh). Run it on the machine
running the game server.

```bash
frp-sh game create [options]
```

### `-p, --prefix <prefix>`

**Purpose**: room-code prefix.

- **Default**: `game`
- Only lowercase alphanumerics and `-_`, max 16 chars

**Example**:

```bash
frp-sh game create --prefix mc
```

### `-t, --ttl <seconds>`

**Purpose**: room lifetime in seconds.

- **Default**: `43200` (12 hours)

**Example**:

```bash
frp-sh game create --ttl 86400
```

### `--service <addr>`

**Purpose**: the game server's local address; guest connections are forwarded
there once the tunnel is up.

- **Default**: `127.0.0.1:25565` (25565 is the Minecraft default port; use any port)
- Format: `IP:port`, usually `127.0.0.1`
- The service must already be listening, otherwise you get
  `cannot reach local service`

**Examples**:

```bash
frp-sh game create --service 127.0.0.1:25565   # Minecraft
frp-sh game create --service 127.0.0.1:7777    # other games (e.g. Palworld)
```

### `--relay`

**Purpose**: skip UDP hole punching and use the relay directly.

- **Default**: off (auto punching with relay fallback)

**Example**:

```bash
frp-sh game create --relay
```

### `--key <passphrase>`

**Purpose**: end-to-end encryption passphrase; both sides must match.

- **Default**: none (unencrypted)

**Example**:

```bash
frp-sh game create --key "our-passphrase"
```

### `--max-conns <N>`

**Purpose**: max connections accepted per session round; after that the round ends
and reconnects automatically.

- **Default**: `0` (unlimited)
- Connections reuse one tunnel **sequentially** (one at a time)
- Useful for sharing scenarios with a strict connection cap

**Example**:

```bash
frp-sh game create --max-conns 5
```

### `--spread <N>`

**Purpose**: punch port spread.

- **Default**: `2`
- Increase (e.g. `5`) for symmetric-NAT scenarios

**Example**:

```bash
frp-sh game create --spread 5
```

### Combined examples

```bash
# minimal
frp-sh game create

# encryption + 5-connection cap + wider spread
frp-sh game create --service 127.0.0.1:25565 --key mypass --max-conns 5 --spread 3
```

---

## `frp-sh game join <room_id>` — game: guest joins a room

```bash
frp-sh game join <ROOM_ID> [options]
```

### `room_id` (positional, required)

**Purpose**: the room code given by the host.

- Format: `prefix-6hex`, case-sensitive

**Example**:

```bash
frp-sh game join game-a3f9c2
```

### `--relay`

**Purpose**: force relay mode.

- **Default**: off

**Example**:

```bash
frp-sh game join game-a3f9c2 --relay
```

### `--listen <addr>`

**Purpose**: the guest's local listen address; the game client connects here and
traffic flows to the host's game server.

- **Default**: `127.0.0.1:25565` (25565 is the Minecraft default port; use any port)
- If the port is taken, pick a free one and connect players to it

**Examples**:

```bash
frp-sh game join game-a3f9c2 --listen 127.0.0.1:25565
frp-sh game join game-a3f9c2 --listen 127.0.0.1:30000
```

### `--key <passphrase>`

**Purpose**: the encryption passphrase matching the host.

- **Default**: none

**Example**:

```bash
frp-sh game join game-a3f9c2 --key "our-passphrase"
```

### `--max-conns <N>`

**Purpose**: max connections per session round.

- **Default**: `0` (unlimited)
- Independent of the host's `--max-conns`; whichever is hit first wins

**Example**:

```bash
frp-sh game join game-a3f9c2 --max-conns 3
```

### `--spread <N>`

**Purpose**: punch port spread.

- **Default**: `2`

**Example**:

```bash
frp-sh game join game-a3f9c2 --spread 5
```

### Combined examples

```bash
frp-sh game join game-a3f9c2
frp-sh game join game-a3f9c2 --listen 127.0.0.1:30000 --key mypass
frp-sh game join game-a3f9c2 --relay
```

---

## `frp-sh dev create` — dev: host creates a room

Application-level port forwarding for development (any TCP service), no mesh.
Same parameters as `game create`, with a different default prefix.

```bash
frp-sh dev create [options]
```

### `-p, --prefix <prefix>`

**Purpose**: room-code prefix.

- **Default**: `dev`

### `-t, --ttl <seconds>`

**Purpose**: room lifetime in seconds.

- **Default**: `43200` (12 hours)

### `--service <addr>`

**Purpose**: the local service address to forward.

- **Default**: `127.0.0.1:25565` (change to your service port, e.g. `127.0.0.1:8080`)
- The service must already be listening

**Examples**:

```bash
frp-sh dev create --service 127.0.0.1:8080     # forward a local web service
frp-sh dev create --service 127.0.0.1:5432     # forward a database port
```

### `--relay`

**Purpose**: skip punching, use relay directly.

- **Default**: off

### `--key <passphrase>`

**Purpose**: end-to-end encryption passphrase.

- **Default**: none

### `--max-conns <N>`

**Purpose**: max connections per session round.

- **Default**: `0` (unlimited)

### `--spread <N>`

**Purpose**: punch port spread.

- **Default**: `2`

### Combined examples

```bash
# share a local web service on 8080 with a teammate
frp-sh dev create --service 127.0.0.1:8080 --key devpass
```

---

## `frp-sh dev join <room_id>` — dev: guest joins a room

Same parameters as `game join`, with a different default prefix (`dev`).

```bash
frp-sh dev join <ROOM_ID> [options]
```

### `room_id` (positional, required)

**Purpose**: the room code given by the host.

### `--relay`

**Purpose**: force relay mode.

- **Default**: off

### `--listen <addr>`

**Purpose**: the guest's local listen address; programs/browsers connect here to
reach the host's service.

- **Default**: `127.0.0.1:25565` (change to your target port)

**Example**:

```bash
frp-sh dev join dev-a3f9c2 --listen 127.0.0.1:8080
```

### `--key <passphrase>`

**Purpose**: the encryption passphrase matching the host.

- **Default**: none

### `--max-conns <N>`

**Purpose**: max connections per session round.

- **Default**: `0` (unlimited)

### `--spread <N>`

**Purpose**: punch port spread.

- **Default**: `2`

### Combined examples

```bash
frp-sh dev join dev-a3f9c2 --listen 127.0.0.1:8080 --key devpass
```

---

## `frp-sh config` — interactive configuration wizard

Use it on first run or when switching signaling servers.

```bash
frp-sh config [--config <FILE>]
```

**Purpose**: interactively asks for and saves the signaling server, relay address,
and other settings (the wizard has 5 steps: signaling address → relay address →
separate UDP port → server password → optional STUN server).

- Without `--config`, saves to the platform default path (see `-c, --config`)
- Press Enter on any prompt to use the default value
- Running bare `frp-sh` with no config also enters the wizard

**Examples**:

```bash
frp-sh config
frp-sh config --config /etc/frp-sh.toml
```

---

## `frp-sh` (no subcommand)

Running `frp-sh` with no subcommand:

- **No config yet**: enters the configuration wizard
- **Config exists**: prints a summary of the current config and common commands

**Example**:

```bash
frp-sh
```

---

## Local network topology support

**One fixed virtual IP per device (VLAN-like)**: the mesh uses the `10.66.0.0/24`
virtual subnet by default (think of it as one VLAN). Each device gets its address
one of three ways, priority order:

1. **Explicit**: `--ip 10.66.0.5` (host or guest)
2. **Host-assigned**: the host reserves a pool with `--guest-ips`; guests take
   addresses in join order and **reuse the same IP across reconnects**
3. **UUID-derived**: with no `--ip` at all, the address is derived stably from the
   device ID (e.g. `10.66.0.42`) — the same device always gets the same IP

**VLANs (multiple subnets)**: different subnets = different VLANs. Any subnet works
via `--ip` + `--netmask`:

```bash
# subnet A (default): 10.66.0.0/24
frp-sh lan create --ip 10.66.0.1

# subnet B: 10.66.1.0/24 (another team)
frp-sh lan create --prefix team-b --ip 10.66.1.1 --netmask 255.255.255.0

# large subnet: 10.66.0.0/16
frp-sh lan create --ip 10.66.0.1 --netmask 255.255.0.0
```

> Note: everyone in one room shares one subnet (naturally reachable); to isolate,
> use separate rooms/subnets.

**Same-LAN auto-direct**: the host advertises all of its LAN addresses when creating
a room; the guest punches at both the public address and the LAN addresses
simultaneously. On the same WiFi/wired LAN a direct link is established in seconds
(`本地局域网直连 (LAN direct)` output) with no server in the path; otherwise the
public punch path is used, falling back to relay only if punching fails. This works
in all three series (lan / game / dev).

**Guest reaches the host's whole LAN (lan series; host needs `--expose-lan`)**: by
default **neither side's real LAN is exposed** — the tunnel only carries the virtual
subnet. Only a side that adds `--expose-lan` advertises its LAN subnets (e.g.
`192.168.1.0/24`); the peer automatically adds routes for those subnets via its
virtual NIC, so it can reach devices on that LAN (NAS, printers, other PCs):

```bash
# host (needs root/admin; enables IPv4 forwarding automatically)
frp-sh lan create --expose-lan
# → LAN subnets  : 192.168.1.0/24

# guest (adds routes automatically, no extra options needed)
frp-sh lan join lan-a3f9c2
# → 路由 192.168.1.0/24 → frp1 已添加
# now you can ping / access devices on the host's LAN
```

Both directions: if the guest also wants the host to reach its LAN, add
`--expose-lan` to `lan join` as well.

Notes:

- root/admin is required (creating the virtual NIC, routing changes, forwarding)
- if the guest's own LAN is on the same subnet as the host's (e.g. both
  `192.168.1.0/24`), that subnet is skipped automatically to avoid route conflicts
  (the output notes `跳过与本地同网段的房主子网`)
- you reach the host's **current** LAN; if the host changes networks, recreate the room
- game / dev are pure port forwarding and do not provide access to the peer's LAN

---

## Session output meanings

| Output | Meaning |
|--------|---------|
| `Room created : lan-a3f9c2` | host room ready |
| `Your ID      : <uuid>` | your device unique ID (stored in `%APPDATA%\frp-sh\identity`; derives your stable virtual IP) |
| `LAN addrs    : 192.168.1.5:51234` | host LAN addresses (same-LAN guests connect directly) |
| `LAN subnets  : 192.168.1.0/24` | host LAN subnets (reachable by guests in the lan series) |
| `Vnet IP      : 10.66.0.x` | your virtual NIC IP (lan series; friends can reach your whole machine long-term) |
| `>>> 本地局域网直连 (LAN direct) with <addr>` | **same-WiFi/LAN direct link** (no server involved, lowest latency) |
| `>>> P2P direct link established with <addr>` | **punch succeeded**, P2P direct |
| `>>> UDP hole punching failed, falling back to relay ...` | punching failed, switching to relay (tries TURN first when configured, then private TCP) |
| `>>> UDP hole punching failed, trying TURN relay` | punching failed, switching to TURN relay (when `turn_providers` is configured) |
| `>>> late P2P link established with <addr>` | direct link re-captured while waiting on relay |
| `connection N from <addr>` | guest side: a local connection entered the tunnel |
| `guest connection N, dialing local service ...` | host side: guest connected, dialing local service |
| `connection N closed` | a tunnel connection ended normally |
| `max connections (N) reached, ending session` | `--max-conns` exhausted, session ends |
| `session ended by peer` | the peer closed the session |
| `>>> 连接已断开，N 秒后自动重连...` | link dropped, auto-reconnecting with backoff (2s, 4s, 8s... capped at 15s) |

---

## Exiting

- **Auto-reconnect**: dropped links (network jitter, expired NAT mappings, service
  restarts) reconnect automatically
- **Host**: `Ctrl-C` ends the session and deletes the room; the session also ends
  when the room expires
- **Guest**: `Ctrl-C` ends the session; it also ends when the room is deleted or expires

---

## Common errors

| Error | Cause & fix |
|-------|-------------|
| `invalid room id: xxx` | bad room format (needs `prefix-6hex`) |
| `room not found or expired: xxx` | room missing/expired; ask the host to create a new one |
| `UDP echo timed out` | server 8080/udp not opened, or wrong config address |
| `decryption failed (wrong --key?)` | the two sides' `--key` do not match |
| `relay rejected: ERROR ROOM_EXPIRED` | room expired, relay rejected |
| `cannot reach local service xxx` | host's local service not running or wrong address |
| `创建 TUN 设备失败` | root/admin required; on Windows place `wintun.dll` next to the executable |
