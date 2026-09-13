# Commands and keyboard controls

## Background supervision, durable spaces, and status (development branch)

Save a connection using `frp-sh profile` first. Here `friends` is an existing profile; the job directory must exist. Use another terminal for control and status commands.

```sh
frp-sh agent configure --profile friends --job ./agent.toml
frp-sh agent run --job ./agent.toml
frp-sh status
frp-sh status --json
frp-sh agent stop --job ./agent.toml
frp-sh agent start --job ./agent.toml
```

`run` is a long-running supervisor without terminal UI. `agent install --job PATH` installs an OS service: a restricted virtual account on Windows, systemd on Linux, and a LaunchDaemon on macOS. Administrator rights are only requested during installation. Afterwards, use `frp-sh start`, `frp-sh stop`, and `frp-sh status` for the client; add `--server` for the server. Use `frp-sh logs` for separate diagnostics.

`status` queries processes the current account is allowed to manage. On Windows it validates the service process identity before showing cross-account service status. Exit codes: 0 means running, 1 unavailable/session error, 2 connection unverified, 3 no visible process, and 4 permission denied.

### Durable spaces

The server persists durable spaces in SQLite. The owner leaving does not remove other members' sessions. Invitations contain no server password: they carry a random single-use token valid for 15 minutes by default. Space connections require HTTPS/WSS.

```sh
frp-sh space create friends
frp-sh space invite SPACE_ID
# Joining device: redeem and attach in one step, without placing the invitation in shell history
frp-sh space join --stdin
```

Each member receives a stable `10.66.0.x` virtual address and a ten-minute access lease. The relay forwards encrypted frames only between authenticated members of the same space. Removing a member, replacing a session, expiry, or closing a session invalidates the prior relay immediately. `space join` combines redemption and attachment; use `space redeem` to register without attaching, and `space connect SPACE_ID` to reconnect a registered device. `space connect` is currently a foreground command; background space recovery and one-command install-and-join are still in development. Legacy `create` / `join` rooms and invitations remain a separate compatibility path.


## Interactive rooms and personal presets

Run `frp-sh` for the terminal app: create a default LAN room immediately, join, customize a room, manage presets or saved connections. `frp-sh preset` opens room presets (shortcut `7`); `frp-sh profile` opens saved connections (shortcut `2`).

```sh
frp-sh create
frp-sh 3856
frp-sh create game
frp-sh create dev
frp-sh create --preset my-room --mtu 1280
```

The new `create game` and `create dev` entry points use the same LAN foundation. Built-in scenes currently share safe network defaults; they do not discover games, expose the physical LAN or automatically publish development services. Legacy `game create` and `dev create` service commands remain compatible.

Custom creation: Tab changes fields, Enter creates, Ctrl+S saves a preset. Presets: N adds, E edits/renames, C duplicates, Delete removes, Enter uses. Built-ins are read-only and can be copied. Resolution order is defaults, saved preset, explicit arguments. Temporary edits do not overwrite presets. Presets store lifetime, MTU, probe spread, relay selection, prefix and scene, never passphrases, room IDs or invitations.

The join page accepts a room ID or invitation; Tab selects an optional extra payload passphrase. `--key` remains an additional shared encryption passphrase, not a server login password. In a room, I opens invitations, G opens separate logs, Q/Esc requests leaving, Enter confirms and Esc cancels. Changing room pages keeps connections alive.

Use `frp-sh --help`, `frp-sh game --help`, `frp-sh dev --help` and `frp-sh profile --help` for the full options supported by your installed binary.

## Terminal app and Profiles

Run `frp-sh`, then press `2` for **Profiles**. Alternatively run `frp-sh profile`; scripts can use `frp-sh --plain profile list`.

Use `N` to add, `E` to edit, `D` to set the default, `Enter` to connect and `Delete` to remove. `G` opens separate logs; `Tab` changes pages. Importing an invitation saves a Profile automatically.

Configure your server in Settings. New invitations contain room credentials, not the server administrator password. Share them privately.

## Game and LAN networking

```sh
frp-sh game create
frp-sh game join 1234
frp-sh lan join 1234
```

Open your game's LAN world, then connect using the host's virtual IP. frp-sh does not require a game port in this mode, though the game itself may require one. Automatic discovery is not guaranteed for every game.

Physical LAN sharing is off by default. `frp-sh join 1234` joins LAN rooms directly; the old `--network` flag remains compatible. Legacy service rooms still expose only their published services.

## Game servers and development services

```sh
frp-sh game create --kind server --tcp 25565
frp-sh game create --kind server --udp 19132
frp-sh dev create --service http://127.0.0.1:3000 --label Web
frp-sh dev create --tcp 3000 --tcp 8080 --udp 9000
frp-sh dev join 1234
frp-sh join 1234
```

These ports are examples, not defaults. Missing creation parameters prompt in an interactive terminal and fail immediately in scripts or JSON mode. A room supports up to 16 host-published services, 32 guests (33 devices including the host), 64 simultaneous streams per member and 256 streams across the host.

Service rooms use encrypted TCP relay transport. TCP, WebSocket and SSH connections run concurrently. UDP datagrams and source flows remain separate, but TCP relay can add ordered-delivery latency. UDP is marked unverified until traffic arrives. Use a Game/LAN room for direct UDP networking.

Guest listeners bind to loopback. If the suggested port is busy, an available port is allocated; use the address actually shown. `C` copies addresses, `I` opens invitations and install-and-join commands, and `G` opens logs.

```sh
frp-sh dev join 1234 --service 1 --listen 4000
# In another terminal on the publishing host:
frp-sh dev add --tcp 8081 --label API
frp-sh dev remove 2
frp-sh dev revoke
```

Service changes close existing application streams and reconnect without replaying requests. To close the last service, exit the host session rather than leaving an empty room. `dev revoke` or `R` on the host revokes current invitations and established connections. Copy a new invitation to admit members again. An optional custom `--key` must match at both ends and is included in invitations.

## Self-hosted server

Use a full server release asset or `cargo build --release`; the normal client download excludes `serve`.

```sh
./frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 --password "$FRPSH_SERVE_PASSWORD"
```

Public listeners require a nonempty password. Allow TCP/UDP 8080 and TCP 8081, and use HTTPS for signaling credentials. Optional built-in TURN only connects live allocations on the same instance; it cannot proxy arbitrary UDP targets.
