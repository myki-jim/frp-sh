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
frp-sh --config config/server.toml game create
```

## Step 3: Host creates a room

On the machine running the game/service:

```bash
frp-sh game create --service 127.0.0.1:25565
```

`--service` is the host's local service address (default `127.0.0.1:25565`). Example output:

```text
  Room created : game-a3f9c2
  Signaling    : http://YOUR-SERVER-IP:8080
  Local service: 127.0.0.1:25565
  Waiting for a guest to join ...
```

Send **`game-a3f9c2`** to your friend.

## Step 4: Guest joins

On the friend's machine:

```bash
frp-sh game join game-a3f9c2 --listen 127.0.0.1:25565
```

`--listen` is the local port the guest listens on (default `127.0.0.1:25565`). Example output:

```text
  Joined room : game-a3f9c2
  Host address: YOUR-SERVER-IP:xxx
  Local listen: 127.0.0.1:25565
  Punching through NAT ...

>>> P2P direct link established with YOUR-SERVER-IP:xxx   ← punch succeeded!
```

Now connecting to `127.0.0.1:25565` on the friend's machine reaches the host's `127.0.0.1:25565`.

If punching fails, it falls back automatically:

```text
>>> UDP hole punching failed, falling back to relay ...
>>> relay connected, waiting for host ...
```

The tunnel still works — traffic just goes through the server (see [Architecture](./architecture)).

## Minimal Commands

| Role | Minimal command | Notes |
|------|-----------------|-------|
| Host | `frp-sh game create` | every option has a default |
| Guest | `frp-sh game join game-xxxxxx` | only the room code is required |
| Server | `frp-sh serve` | listens on 0.0.0.0:8080/8081 |

## Next Steps

- [CLI Reference](./cli) for all options
- [Advanced Usage](./advanced) for encryption, multi-connection, etc.
- [Troubleshooting FAQ](./faq) if you hit problems
