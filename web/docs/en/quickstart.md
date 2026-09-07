# Quickstart

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

Physical LAN sharing is off by default. The generic `frp-sh join 1234 --network` explicitly permits whole-device networking; joining services does not grant that permission.

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
