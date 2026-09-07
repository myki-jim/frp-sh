# Architecture

This overview describes 0.5.4. Wire protocol: v3; network helper: v2.

## Room workflows

`frp-sh create`, `create game`, and `create dev` create LAN rooms. Game/dev in this command order are parameter presets, not service discovery. The legacy `game create --tcp ...` and `dev create --tcp ...` commands publish explicit services.

LAN uses a virtual adapter, with separate device addresses. Physical LAN exposure is opt-in. The privileged helper handles adapter and route operations; the ordinary client handles connections. The server holds room registrations in memory, so restart loses rooms.

## Connection paths

Clients discover addresses, exchange candidates through signaling and attempt UDP punching. Available TURN can provide a UDP relay; TCP relay is the fallback. `--relay` skips punching and TURN and forces TCP. Exhausting direct probes does not by itself disable TURN.

Service rooms use a bounded, multiplexed TCP relay transport for explicit TCP/UDP services. UDP datagrams preserve boundaries, but TCP head-of-line blocking still applies. Up to 16 services, 32 guests, 64 streams per guest and 256 per room are supported.

## Authentication and encryption

Public signaling/relay listeners require a server password. Room invitations carry scoped room credentials, not the server administration password. Owner operations also require the owner token. Use HTTPS to protect HTTP signaling credentials.

Room-token relay transport encryption is not secrecy from the operator. An optional shared `--key` adds payload encryption; service rooms apply this on top of TCP relay. Invitations containing that key are sensitive. The built-in TURN server restricts destinations to its active relay endpoints.

## Resource limits

0.5.4 introduces operator-controlled admission limits; see [Server deployment](./server). They count registrations, not live people. They do not enforce bandwidth or traffic quotas.

The current host mesh loop polls signaling every 100 ms; the terminal room view polls every 2 seconds. These are implementation details, not timing guarantees. Room snapshots share a registry lock, so scale must be measured rather than inferred from RAM alone.

See [Protocol](./protocol) for current authentication and relay framing entry points.
