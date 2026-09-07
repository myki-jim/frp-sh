# Quickstart

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

0.5.0 uses protocol v3. Upgrade the server, clients and installed network helper together, then create new rooms. Older versions report a protocol conflict.

## Install or update

```powershell
irm https://frp.sh/install.ps1 | iex
```

```sh
curl -fsSL https://frp.sh/install.sh | sh
```

Administrator access is needed only for installation. Run daily sessions in an ordinary terminal. Service mode does not create a virtual adapter.

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

These ports are examples, not defaults. Missing creation parameters prompt in an interactive terminal and fail immediately in scripts or JSON mode. A room supports up to 16 host-published services, 32 members, 64 simultaneous streams per member and 256 streams across the host.

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
