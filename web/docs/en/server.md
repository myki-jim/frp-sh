# Deploy the Signaling Server

The signaling server is the center of the system: room registration, address exchange, and fallback relay. It is stateless and extremely light (well under 2 MB RAM when idle); a 1-core 512 MB VPS is plenty.

## Quick start

```bash
./frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081
```

| Option | Default | Description |
|--------|---------|-------------|
| `--addr` | `0.0.0.0:8080` | HTTP REST listen address (UDP probe shares the port) |
| `--relay-addr` | `0.0.0.0:8081` | private TCP relay listen address (fallback when both punching and TURN fail) |
| `--password` | none | server password (signaling 401 check + relay auth encryption) |
| `--turn` | off | built-in TURN listen address (e.g. `0.0.0.0:3478`, RFC 5766 UDP subset) |
| `--external-ip` | listen address IP | public IP facing outbound (required when running TURN behind NAT/Docker) |

## systemd service (recommended)

Create `/etc/systemd/system/frp-sh.service`:

```ini
[Unit]
Description=frp-sh signaling server
After=network.target

[Service]
ExecStart=/opt/frpsh/target/release/frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
```

Enable and start:

```bash
systemctl daemon-reload
systemctl enable frp-sh
systemctl start frp-sh
systemctl status frp-sh
```

Verify the listeners:

```bash
ss -lntup | grep -E ':8080|:8081'
# You should see 8080/tcp, 8080/udp, and 8081/tcp
```

## Firewall

Open three ports:

| Port | Protocol | Purpose |
|------|----------|---------|
| 8080 | TCP | HTTP REST |
| 8080 | UDP | public address probe |
| 8081 | TCP | relay forwarding |

```bash
# ufw
ufw allow 8080/tcp
ufw allow 8080/udp
ufw allow 8081/tcp

# or iptables
iptables -I INPUT -p tcp --dport 8080 -j ACCEPT
iptables -I INPUT -p udp --dport 8080 -j ACCEPT
iptables -I INPUT -p tcp --dport 8081 -j ACCEPT
```

> Cloud security groups (Tencent Cloud, Alibaba Cloud, etc.) must also be opened, and **UDP 8080 is mandatory** — missing it makes client probing fail with `UDP echo timed out`.

## Built-in TURN relay (optional)

When punching fails, traffic goes over the server's private TCP relay by default. You can also enable the **built-in TURN server** (RFC 5766) so clients relay over UDP instead of TCP — better traversal and compatible with the standard STUN/TURN ecosystem:

```bash
./frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 \
  --password YOUR_PASSWORD --turn 0.0.0.0:3478 --external-ip YOUR_PUBLIC_IP
```

| Item | Value |
|------|-------|
| TURN username | `frp-sh` (fixed) |
| TURN password | same as `--password` |
| Realm | `frp.sh` |
| Client config | `turn_providers = ["turn://frp-sh:YOUR_PASSWORD@SERVER_IP:3478"]` |

> **Why `--external-ip` is needed**: the TURN server advertises relay addresses taken from the listen address's IP; behind NAT / Docker (e.g. listening on `0.0.0.0` picks up a private address like `172.17.0.x`), you must specify the public IP explicitly so clients can reach the relay ports.

**Firewall/security group**: TURN additionally needs `3478/udp` open (the STUN/TURN control port), plus the **relay port range** handed out to clients. Relay ports are assigned randomly per client (high ports); opening a UDP range (e.g. `50000-50100/udp`) or all inbound UDP is recommended.

```bash
ufw allow 3478/udp
ufw allow 50000:50100/udp
```

systemd example (with TURN):

```ini
ExecStart=/opt/frpsh/target/release/frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 --password YOUR_PASSWORD --turn 0.0.0.0:3478 --external-ip YOUR_PUBLIC_IP
```

Client verification (after configuring `turn_providers`):

```text
>>> UDP hole punching failed, trying TURN relay     # punching failed, auto-switched to TURN
```

You can also test connectivity directly with any standard TURN client (coturn's `turnutils_uclient`, etc.).

## Verify from a remote machine

```bash
# HTTP
curl http://SERVER-IP:8080/health
# → ok

# UDP probe (Linux: use nc or any UDP client)
# Expect: ADDR <token> <your-public-ip>:<port>
```

## Coexisting with other services

Other services on the server (e.g., ports 8000/8765) are unaffected; to move ports, change `--addr`/`--relay-addr` and update the client config accordingly.

## Multiple servers (optional)

The signaling protocol is plain HTTP plus simple UDP/TCP text protocols — you can implement a compatible server yourself (~200 lines) or run multiple instances, each with clients configured to point at it.

## Security notes

- The signaling HTTP layer has no built-in TLS: put Nginx/Caddy in front for production HTTPS termination
- The private TCP relay uses a password-derived key for ChaCha20-Poly1305 stream encryption once `--password` is set (see [Configuration](./config)); end-to-end confidentiality still needs `--key` on both ends (P2P direct links only, see [Advanced Usage](./advanced))
- Ports are intentionally public (the room code is the access credential); restrict by source IP in the security group if you're worried about scanners
