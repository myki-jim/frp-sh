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

### `invalid room id` rules

Format: `prefix-6hex` (e.g., `game-a3f9c2`). `--prefix` keeps only lowercase alphanumerics and `-_`, max 16 chars.

### How does a session end?

`Ctrl-C` on either side; or automatically when `--max-conns` is exhausted. The host deletes the room on exit.

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
