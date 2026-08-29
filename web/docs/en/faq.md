# Troubleshooting FAQ

## Connection issues

### Guest: `room not found or expired: xxx`

- Wrong room code (case-sensitive)
- Room expired (default 12h) or closed by the host's Ctrl-C
- Ask the host to run `game create` again for a fresh code

### Guest: `UDP echo timed out`

The client cannot reach the server's UDP probe port:

- Check `8080/udp` is opened (cloud security group **and** OS firewall)
- Check `signaling_addr` points at the right server
- Verify with `nc -u SERVER-IP 8080` or any UDP client sending `ECHO test`

### Stuck at `UDP hole punching failed, falling back to relay ...`

Punch failure is normal for strict NATs and **does not break usage** — it falls back to relay automatically. To get direct:

- Try a larger `--spread` (symmetric NAT)
- Make sure UDP outbound works on both sides
- If both ends are behind the same NAT/LAN, public-IP punching is a hairpin case and may fail — use `--relay`

### Direct link established but no data flows

- Confirm the host's `--service` process is **running** and listening on the right port
- Confirm the guest's `--listen` port is not occupied
- Run with `--verbose` and check whether frames are flowing

### `decryption failed (wrong --key?)`

The two sides' `--key` passphrases differ (or one side omitted it). Align them and recreate the room.

## Deployment issues

### Server: port already in use

```text
AddrInUse
```

- `8080` or `8081` is taken by another process; change ports:
  ```bash
  frp-sh serve --addr 0.0.0.0:9000 --relay-addr 0.0.0.0:9001
  ```
- Update the client config accordingly (`http://IP:9000`, `IP:9001`)

### Windows: quick restart fails with a socket permission error

After a TCP listener closes, Windows briefly holds the port in a transitional state; immediately re-binding UDP on the same port can fail:

- Wait 2–3 seconds before restarting
- Or use a separate UDP probe port (`signaling_udp` + a dedicated server listener)

## Performance & stability

### Slow transfers

- The reliable stream window is 32 × 1200 B ≈ 38 KB in flight; high-latency links are throughput-limited
- Relay mode is capped by the server's egress
- Direct punching is generally faster (one less hop)

### Connection dies after long idle

NAT mappings expire (usually 30–120 s). frp-sh sends keepalive ACKs every second, so this normally doesn't happen; if your NAT has an extremely short timeout, keep a trickle of data flowing.

### Frames dropped under heavy load

The read buffer caps at 1 MB; beyond that frames are dropped and recovered by retransmit (go-back-N flow control). Expected behavior — correctness is unaffected.

## Security questions

### Is relay mode safe?

Relay traffic is **plaintext** (it passes through your signaling server). For confidentiality:

- Prefer direct punching + `--key`
- Or wrap the path to the server yourself (e.g., WireGuard)

### Can room codes be guessed?

The code body is 6 hex chars (~16M combinations) with a 12h TTL; brute-forcing means hitting the server millions of times in a short window — low risk but nonzero. For higher assurance:

- Use a longer custom prefix: `frp-sh game create --prefix my-long-room-2024`
- Add `--key` (even if the code leaks, the traffic stays encrypted)

## Other

### Both sides are on the same WiFi — how do we get a LAN direct link?

No action needed — it's automatic. The host advertises its LAN addresses when creating
a room; the guest punches at both the public and LAN addresses. On the same subnet you
get `本地局域网直连 (LAN direct)` within seconds — traffic stays entirely on the LAN,
no server involved, lowest latency (great for gaming).

### Can the guest reach other devices on the host's LAN (NAS/printer)?

Yes — use the **`lan` series** (mesh, Tailscale-like). The host's `frp-sh lan create`
advertises its LAN subnets (e.g. `192.168.1.0/24`) and enables IPv4 forwarding; the
guest's `frp-sh lan join` automatically adds routes via its virtual NIC, then can
ping / access devices on the host's LAN. If both sides are on the same subnet, that
subnet is skipped automatically to avoid route conflicts.

`game` / `dev` are pure port forwarding and do not provide access to the peer's LAN.

### Do Windows / macOS need admin rights for `lan` mode?

Yes. Creating the virtual NIC, setting IPs, and adding routes are privileged
operations. To avoid manually right-clicking "Run as administrator":

- **Windows**: just run `frp-sh lan create/join` — when the program detects
  insufficient rights it pops a UAC elevation prompt automatically; click "Yes"
  once and it continues as admin (a UAC confirmation appears on each run)
- **macOS / Linux**: run `sudo frp-sh lan create/join`
- Windows `wintun.dll` is downloaded automatically by the installer
  (`irm https://frp.sh/install.ps1 | iex`)

`game` / `dev` are pure port forwarding and need **no** admin rights.

### Why is the default port 25565? Is that Minecraft's?

Yes — 25565 is the default Minecraft (Java Edition) server port. frp-sh was designed for "play with friends" scenarios, so both the host service and the guest listen default to 25565 for out-of-the-box server hosting.

Any port works: the host sets `--service` to its service address, the guest sets `--listen` to its local port. For example, a web service on port 3000:

```bash
frp-sh game create --service 127.0.0.1:3000
frp-sh game join game-a3f9c2 --listen 127.0.0.1:3000
```

### `invalid room id` rules

Format: `prefix-6hex` (e.g., `game-a3f9c2`). `--prefix` keeps only lowercase alphanumerics and `-_`, max 16 chars.

### How does a session end?

By default sessions **reconnect automatically** (backoff from 2s, capped at 15s), so link jitter won't end them. A session truly ends when:

- Either side presses `Ctrl-C` (the host deletes the room on exit)
- The room expires or is deleted (both sides end automatically)
- `--max-conns` is exhausted — the current round ends, then it reconnects and waits for the next round

### IPv6 support?

Signaling, relay, and punching all use standard `SocketAddr` and support IPv6 (`[::1]:8080` form in config). Punching behavior depends on the actual network.

### Can it run in Docker?

Yes — the static binary drops straight into an image:

```dockerfile
FROM ubuntu:24.04
COPY frp-sh /usr/local/bin/frp-sh
EXPOSE 8080/tcp 8080/udp 8081/tcp
CMD ["frp-sh", "serve", "--addr", "0.0.0.0:8080", "--relay-addr", "0.0.0.0:8081"]
```

## Feedback

If something is not covered, report it with `frp-sh --verbose` output and a description of the network environment.
