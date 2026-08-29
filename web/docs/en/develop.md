# Development & Testing

Project structure, testing, and design conventions for contributors.

## Project layout

```text
src/
├── main.rs            # entry: parse subcommands, dispatch to commands
├── lib.rs             # library entry (for integration tests)
├── cli.rs             # clap subcommand definitions
├── commands.rs        # create/join/serve orchestration (punch/relay/encryption wiring)
├── config.rs          # TOML config loading (signaling/relay/separate UDP probe)
├── error.rs           # thiserror error types (FrpError)
├── utils.rs           # room codes/tokens/timestamps/validation
├── room/state.rs      # session state (role, peer, relay flag)
├── signaling/
│   ├── client.rs      # REST client + UDP public probe
│   └── server.rs      # axum server + UDP echo + TCP relay pairing
└── p2p/
    ├── hole_punch.rs  # PUNCH/ACK simultaneous handshake + port spread + self-punch guard
    ├── stream.rs      # FRS1 reliable UDP stream (window/retransmit/keepalive/encryption)
    └── relay.rs       # relay client transport
tests/e2e.rs           # in-process signaling server end-to-end tests
config/default.toml    # default config (127.0.0.1)
docs/                  # this documentation site (source)
deploy/                # VPS deployment scripts (contains credentials — keep private)
```

## Local development

```bash
cargo build                # debug build
cargo run -- serve         # local signaling server
cargo run -- game create   # host (defaults to 127.0.0.1:8080)
cargo run -- game join game-xxxx
```

## Testing

```bash
cargo test
```

### Unit tests (10)

| Module | Coverage |
|--------|----------|
| `utils` | room code format, token length, prefix sanitization |
| `config` | defaults, TOML parsing, missing fields |
| `hole_punch` | PUNCH/ACK message parsing |
| `stream` | frame codec, 64KB roundtrip, 20% loss retransmit, encrypted roundtrip, bidirectional graceful close |

### End-to-end tests (5)

Tests boot a **full in-process signaling server** (HTTP + UDP echo + relay) and run real sessions:

| Test | Verifies |
|------|----------|
| `e2e_direct_hole_punch` | punch direct + single-connection tunnel echo roundtrip |
| `e2e_direct_multiconn` | 3 sequential connections with individual echoes in one session |
| `e2e_direct_encrypted` | `--key` encrypted tunnel roundtrip |
| `e2e_relay_fallback` | forced relay pairing + tunnel roundtrip |
| `room_lifecycle_api` | room create/query/join/delete API lifecycle |

The test server uses a **separate UDP port** (via `signaling_udp`), avoiding the Windows same-port TCP/UDP bind timing issue.

## Design conventions

- **One socket throughout**: public probing, punching, and the data stream share one UDP socket, so the NAT mapping stays consistent
- **select safety**: for streaming concurrency use "long-lived tasks + bounded channels"; never `read` a stream directly inside `select!` (the unselected branch's half-read data is lost)
- **Windows compatibility**: every UDP recv/send error path must consider WSAECONNRESET(10054) ICMP poisoning
- **Error layering**: library functions return `Result<T, FrpError>` (thiserror); the command layer aggregates with anyhow
- **Logging**: `log` macros + `env_logger`; `--verbose` prints frame-level debug info

## Known pitfalls

1. **Frame header size**: the FRS1 header is 15 bytes (magic4+flags1+seq4+ack4+len2); it was once mistyped as 16, dropping every frame
2. **Self-punching**: when two engines get adjacent ephemeral ports, spread targets include your own port; exclude by port
3. **One-shot ACK**: Windows ICMP poisoning can swallow the first ACK; retry it
4. **Test port conflicts**: after force-killing processes, same-port UDP binds can fail briefly; the test server uses a separate UDP port

## Release flow

```bash
cargo test                          # all green
cargo build --release               # artifact: target/release/frp-sh
cargo build --release --target x86_64-unknown-linux-musl   # optional static Linux build
```
