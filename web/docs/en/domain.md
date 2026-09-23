# Bring your own domain

frp-sh can publish a local HTTP service under a domain you own. Public requests reach the frp-sh gateway and travel through an outbound, device-authenticated WebSocket tunnel to `127.0.0.1`; no router port forwarding is required.

The public community gateway currently serves traffic on HTTPS port `18443`:

```text
https://app.example.com:18443
```

## 1. Create a binding

```sh
frp-sh domain --server https://control.test.frp.sh:18443 bind app.example.com
```

The command returns one-time `txt_name`, `txt_value`, `cname_target`, and `https_port` fields. Add the exact records it prints at your DNS provider:

```text
<txt_name>       TXT    <txt_value>
app.example.com  CNAME  edge.test.frp.sh
```

Do not copy a verification value from an example. Every `bind` generates a new name and value that expire after 15 minutes. For an apex domain that cannot use a normal CNAME, use your provider's CNAME flattening, ALIAS, or ANAME support, or bind a subdomain.

## 2. Verify ownership

After the TXT record is visible through public DNS, run:

```sh
frp-sh domain --server https://control.test.frp.sh:18443 verify app.example.com
frp-sh domain --server https://control.test.frp.sh:18443 status app.example.com
```

When status shows `"verified": true`, the TXT record may be removed; keep the CNAME in place. The binding belongs to the local device key. Move that key when moving devices, or unbind from the original device first.

## 3. Publish the local service

For a site listening on `127.0.0.1:3000`:

```sh
frp-sh domain --server https://control.test.frp.sh:18443 publish app.example.com --target 127.0.0.1:3000
```

This command stays in the foreground. Run it under a system service or the existing agent supervisor for startup persistence. When the publisher exits, the gateway returns `502 publisher_offline` while retaining the binding.

Unbinding immediately disconnects the publisher and removes certificate authorization:

```sh
frp-sh domain --server https://control.test.frp.sh:18443 unbind app.example.com
```

## Security boundaries and limits

- Only regular ASCII/punycode hostnames are accepted. IP addresses, wildcards, and reserved internal suffixes are rejected.
- The local target must be an explicit-port `http://127.0.0.1`, `http://localhost`, or `http://[::1]` endpoint. It cannot be used as an arbitrary private-network proxy.
- The first release forwards regular HTTP requests. Public WebSocket, arbitrary TCP, CONNECT, and UDP are not supported.
- Request bodies are limited to 1 MiB, response bodies to 4 MiB, and total request time to 30 seconds.
- The gateway rewrites `Host`, `X-Forwarded-Host`, and `X-Forwarded-Proto`, and discards client forwarding and hop-by-hop headers.
- Caddy issues certificates only when the server `allow` endpoint confirms a verified binding. Public port `443` must be reachable for ACME TLS-ALPN; application traffic uses `18443`.
- Port `80` can be intercepted by ICP filtering on some mainland China networks. Share the explicit `https://…:18443` URL instead of relying on an HTTP redirect.

See [Deploy the server](./server#custom-domain-ingress) for self-hosting options.
