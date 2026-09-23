# Deploy the Signaling Server
> 0.5.0: public listeners require a nonempty password. For the systemd example, create `/etc/frp-sh/server.env` readable only by root with `FRPSH_SERVE_PASSWORD` set. Use HTTPS for signaling.


The server keeps room registrations in memory, exchanges peer addresses, and provides fallback relaying. Restarting clears the registry. Size CPU, memory and public bandwidth from active rooms, polling, relay traffic and measured load; no fixed idle-memory or user-capacity guarantee is provided.

## Quick start

```bash
./frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 --password "$FRPSH_SERVE_PASSWORD"
```

## Durable spaces and member relay

To enable durable spaces, configure a server password, a private SQLite path, and the public HTTPS origin seen by users. `--spaces-origin` must be that public origin. A reverse proxy must pass WebSocket Upgrade so members can connect to `wss://…/spaces/v1/<SPACE_ID>/relay`.

```bash
./frp-sh serve \
  --addr 127.0.0.1:8080 \
  --relay-addr 0.0.0.0:8081 \
  --password "$FRPSH_SERVE_PASSWORD" \
  --spaces-db /var/lib/frp-sh/spaces.sqlite \
  --spaces-origin https://frp.sh \
  --invite-ttl 900 \
  --invite-max-ttl 86400
```

The SQLite file and its parent directory should only be accessible to the service account. Invitation tokens are stored only as digests. The default lifetime is 900 seconds; the server can set a default and maximum from one second through 24 hours.

## Custom-domain ingress

The domain control plane and HTTP ingress use a separate SQLite database and loopback listener:

```bash
./frp-sh serve \
  --addr 127.0.0.1:8080 \
  --relay-addr 0.0.0.0:8081 \
  --password "$FRPSH_SERVE_PASSWORD" \
  --domains-db /var/lib/frp-sh/domains.sqlite \
  --domains-origin https://control.test.frp.sh:18443 \
  --ingress-addr 127.0.0.1:8082 \
  --ingress-cname edge.test.frp.sh \
  --ingress-https-port 18443
```

| Option | Purpose |
| --- | --- |
| `--domains-db` | Domain ownership and verification database; enables the domain API |
| `--domains-origin` | Public HTTPS control origin bound into device signatures |
| `--ingress-addr` | HTTP ingress used only by the TLS gateway; keep it on loopback |
| `--ingress-cname` | Stable DNS target returned by `bind` |
| `--ingress-https-port` | Public HTTPS port returned by `bind` |

Caddy must proxy `/domains/v1/*` to port `8080` only for the control hostname, and user-domain traffic to `8082`. Configure On-Demand TLS with `ask http://127.0.0.1:8080/domains/v1/allow`; without the ask gate, arbitrary SNI values can consume certificate issuance limits. Keep the platform wildcard certificate and unknown user domains in separate TLS policies. A complete configuration is available at `ops/caddy/Caddyfile.example` in the repository.

Application traffic uses `18443`, while Caddy must also listen on public TCP `443` for ACME TLS-ALPN validation. First-time certificate issuance fails if a cloud security group blocks 443. Port `80` is optional and may be disabled.

Restrict the SQLite directory to the service account. TXT tokens are stored only as SHA-256 digests, and the Caddy ask endpoint accepts verified bindings only. The ingress has fixed request, response, timeout, and global concurrency bounds; community operators should still add bandwidth budgets, request-rate limits, and traffic monitoring at the gateway or cloud edge.

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
EnvironmentFile=/etc/frp-sh/server.env
ExecStart=/opt/frpsh/target/release/frp-sh serve --addr 0.0.0.0:8080 --relay-addr 0.0.0.0:8081 --password ${FRPSH_SERVE_PASSWORD}
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
| Client config | Set a private TURN address in `turn_providers` |

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

## Logs and troubleshooting

Use frp-sh logs path to locate per-process JSONL diagnostics and frp-sh logs tail --follow to read them. Files rotate at 5 MiB with three backups. There are no HTTP log endpoints. Set a stable HOME for service accounts.

## Coexisting with other services

Other services on the server (e.g., ports 8000/8765) are unaffected; to move ports, change `--addr`/`--relay-addr` and update the client config accordingly.

## Multiple servers (optional)

Implementations must follow protocol v3 authentication, framing and resource limits; see [Protocol](./protocol). Multiple instances can be deployed with separate client configurations.

## Security notes

- The signaling HTTP layer has no built-in TLS: put Nginx/Caddy in front for production HTTPS termination
- Relay transport encryption uses server/room credentials and is not secrecy from the relay operator. Service rooms support additional end-to-end payload encryption using a shared `--key`.
- A room code is an identifier, not an authorization credential. Protect invitations and server credentials; restrict listener exposure as appropriate.

## Community server capacity limits (0.5.4+)

Append these options to your existing `serve` command, for example:

```bash
--max-rooms 20 --max-members 8 --max-total-members 100
```

| Option | Default | Range / meaning |
| --- | --- | --- |
| `--max-rooms` | 1024 | 1–1024 unexpired rooms |
| `--max-members` | 33 | 2–33 registered devices per room, including the owner |
| `--max-total-members` | 33792 | 1–33792 memberships across the server, including all owners |

All three limits apply together and are enforced atomically by the server. Creation or admission beyond capacity returns HTTP 429. A registered device reconnecting with the same UUID reuses its slot even when full. The same device in different rooms consumes a membership in each room.

These are registration limits, not live online counts: temporary disconnects retain slots; deleting or expiring the room releases them. After editing systemd `ExecStart`, run `systemctl daemon-reload` and `systemctl restart frp-sh`. Restarting clears in-memory rooms, requiring recreation/rejoining. Inspect the `limits` field on `/version` to verify settings.

These controls do not provide bandwidth throttling or traffic quotas; relay bandwidth needs separate management.
