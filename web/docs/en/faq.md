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

Punch failure is normal (overly strict NAT) and **does not break usage** — it falls
back to relay automatically. Since v0.3.7 **one failed punch permanently downgrades**:
later rounds no longer retry punching or TURN and go straight to the TCP relay (no
more wasting ten-plus seconds retrying the same failing path). If you want a direct
link or more attempts:

- `--punch-retries 3`: downgrade only after 3 failed rounds (default 1);
  `--punch-retries 0` skips punching entirely
- Try a larger `--spread` (symmetric-NAT scenarios)
- Make sure UDP outbound works on both sides; on the same WiFi see [LAN direct](#both-sides-are-on-the-same-wifi-—how-do-we-get-a-lan-direct-link)
- If both ends are on the same LAN / behind the same NAT, public-IP punching is a
  hairpin case and the direct link may fail — staying downgraded on the relay is fine,
  or try the LAN-direct path first (LAN addresses are advertised automatically)
- TURN relay (configure `turn_providers`) is only tried in the first round before the
  downgrade; after downgrading it always uses the TCP relay

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

The read buffer caps at 1 MB; beyond that frames are dropped and recovered by retransmit (go-back-N flow control). Expected behavior —correctness is unaffected.

## Security questions

### Is relay mode safe?

Two layers:

- **Server password** (`serve --password`): the private TCP relay channel is
  stream-encrypted with ChaCha20-Poly1305 using a password-derived key — third
  parties (ISP/middleboxes) can't see the content (the server holds the
  password and can decrypt to forward); TURN relay uses standard long-term
  credential auth (prevents impersonation), but a standard TURN data plane is
  plaintext — pair it with `--key` when you need confidentiality
- **`--key` end-to-end**: P2P direct traffic is encrypted with a key derived from
  `--key` — **the server cannot decrypt it either** (even if the password leaks)

Combo: prefer `--key` for end-to-end secrecy on self-use/LAN direct links; when
punching fails and you go through relay, a server password at least keeps traffic
from being naked on the wire.

### Can the signaling server require a password (stop strangers creating rooms)?

Yes. Start the server with `--password`:

```bash
frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 --password YOUR_PASSWORD
```

Clients must then set the same password in their config (`frp-sh config` wizard
step 4, or add `password = "..."` to `config.toml`):

- **Request auth**: every signaling request is checked; missing/wrong → 401
- **Relay auth + encryption**: relay connections must carry the password, and the
  channel is encrypted
- Servers without a password behave exactly as before (old clients keep working)

### Can room codes be guessed?

The code body is 6 hex chars (~16M combinations) with a 12h TTL; brute-forcing means hitting the server millions of times in a short window —low risk but nonzero. For higher assurance:

- Use a longer custom prefix: `frp-sh game create --prefix my-long-room-2024`
- Add `--key` (even if the code leaks, the traffic stays encrypted)

## Other

### Both sides are on the same WiFi —how do we get a LAN direct link?

No action needed —it's automatic. The host advertises its LAN addresses when creating
a room; the guest punches at both the public and LAN addresses. On the same subnet you
get `本地局域网直连 (LAN direct)` within seconds —traffic stays entirely on the LAN,
no server involved, lowest latency (great for gaming).

### Can the guest reach other devices on the host's LAN (NAS/printer)?

Yes, but **it's not exposed by default**. Use the **`lan` series** (mesh,
Tailscale-like), and the host must add `--expose-lan` when creating the room:
`frp-sh lan create --expose-lan` advertises its LAN subnets (e.g. `192.168.1.0/24`)
and enables IPv4 forwarding; the guest's `frp-sh lan join` automatically adds routes
via its virtual NIC, then can ping / access devices on the host's LAN. If both sides
are on the same subnet, that subnet is skipped automatically to avoid route
conflicts.

The guest can also add `--expose-lan` (`frp-sh lan join <ROOM_ID> --expose-lan`) to
let the host reach devices on the guest's own LAN.

`game` / `dev` are pure port forwarding and do not provide access to the peer's LAN.

### Do Windows / macOS need admin rights for `lan` mode?

Yes. Creating the virtual NIC, setting IPs, and adding routes are privileged
operations. To avoid manually right-clicking "Run as administrator":

- **Windows**: just run `frp-sh lan create/join` —when the program detects
  insufficient rights it pops a UAC elevation prompt automatically; click "Yes"
  once and it continues as admin (a UAC confirmation appears on each run), and it auto-allows inbound traffic on the virtual NIC (otherwise the peer's ping/connections would be blocked by the Windows firewall)
- **macOS / Linux**: run `sudo frp-sh lan create/join`
- Windows `wintun.dll` is downloaded automatically by the installer
  (`irm https://frp.sh/install.ps1 | iex`)

`game` / `dev` are pure port forwarding and need **no** admin rights.

### Is each device's virtual IP fixed? How do I manage them (VLAN-like)?

Yes —**every device gets a fixed virtual IP**, one of three ways (priority order):

1. **Explicit**: `frp-sh lan join lan-xxx --ip 10.66.0.5`
2. **Host-assigned**: the host reserves a pool with
   `frp-sh lan create --guest-ips 10.66.0.2,10.66.0.3`; guests take addresses in
   join order and reuse the same IP across reconnects
3. **UUID-derived**: with nothing configured, the address derives stably from the
   device ID (e.g. `10.66.0.42`) —the same device always gets the same IP

The default subnet is `10.66.0.0/24` (one VLAN). For multiple VLANs, open separate
rooms on different subnets (`--ip` + `--netmask` support any subnet). Everyone in one
room shares one subnet and can reach each other directly.

### Why is the default port 25565? Is that Minecraft's?

Yes —25565 is the default Minecraft (Java Edition) server port. frp-sh was designed for "play with friends" scenarios, so both the host service and the guest listen default to 25565 for out-of-the-box server hosting.

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
- `--max-conns` is exhausted —the current round ends, then it reconnects and waits for the next round

### IPv6 support?

Signaling, relay, and punching all use standard `SocketAddr` and support IPv6 (`[::1]:8080` form in config). Punching behavior depends on the actual network.

### Can it run in Docker?

Yes —the static binary drops straight into an image:

```dockerfile
FROM ubuntu:24.04
COPY frp-sh /usr/local/bin/frp-sh
EXPOSE 8080/tcp 8080/udp 8081/tcp
CMD ["frp-sh", "serve", "--addr", "0.0.0.0:8080", "--relay-addr", "0.0.0.0:8081"]
```

## Feedback

If something is not covered, report it with `frp-sh --verbose` output and a description of the network environment.

