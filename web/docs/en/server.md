# Deploy the Signaling Server

The signaling server is the center of the system: room registration, address exchange, and fallback relay. It is stateless and extremely light (well under 2 MB RAM when idle); a 1-core 512 MB VPS is plenty.

## Quick start

```bash
./frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081
```

| Option | Default | Description |
|--------|---------|-------------|
| `--addr` | `0.0.0.0:8080` | HTTP REST listen address (UDP probe shares the port) |
| `--relay-addr` | `0.0.0.0:8081` | TCP relay listen address |

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
- Relay traffic is not encrypted: use `--key` on both ends when confidentiality matters (P2P direct links only, see [Advanced Usage](./advanced))
- Ports are intentionally public (the room code is the access credential); restrict by source IP in the security group if you're worried about scanners
