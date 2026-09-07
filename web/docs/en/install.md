# Installation and maintenance (0.5.4)

Installers prefer the website downloads and automatically fall back to GitHub. A version manifest pins SHA-256 hashes for both the client and helper to prevent mixed releases. Windows upgrades reuse an existing signature-verified Wintun driver. The website uses a global CDN, not a mainland-China CDN; performance depends on the local network.

LAN operations use an installed system network helper. Authorize installation once, then run the client from your ordinary account: creating, joining and reconnecting do not request UAC or sudo. Explicit TCP/UDP service forwarding does not require this helper; Game virtual LAN does.

> 0.5.0 upgrades signaling protocol v3 and the network helper. Reinstall on both devices and update the server.

## Install

Windows PowerShell:

```powershell
irm https://frp.sh/install.ps1 | iex
```

Linux/macOS:

```sh
curl -fsSL https://frp.sh/install.sh | sh
```

The installer retrieves a matching client/helper pair and checks SHA-256 files. Windows also verifies the Wintun Authenticode signature. Checksums verify download integrity; they are not independent release signatures.

| Platform | Protected installation | Service |
| --- | --- | --- |
| Windows | `%ProgramFiles%\frp-sh` | `FrpShNetwork` |
| Linux | `/usr/local/lib/frp-sh` | `frp-sh-network.service` or procd |
| macOS | `/usr/local/lib/frp-sh` | `com.frpsh.network` |

Only administrators can change installed executables. IPC permits the original installer user's SID or UID. Direct root installation requires `FRPSH_INSTALL_UID`. Linux needs iproute2 and systemd/procd. OpenWrt hardware validation remains pending.

## Verify as an ordinary user

```sh
frp-sh doctor
frp-sh doctor --network-test
frp-sh --lang en lan create
```

The network test creates and closes a temporary adapter; stop active LAN sessions first. Missing helpers produce an actionable error and never launch elevation.

## Updates and removal

`frp-sh update` checks for releases and shows instructions. Rerun the installer to update; authorization is part of installation maintenance. Signed, unattended helper updates are not implemented. Stop sessions before updating. The installer keeps the previous executable pair for recovery.

An administrator can uninstall by stopping and deleting the service above, removing its installation directory and PATH entry. Preserve user configuration and logs unless explicitly deleting them. Automated removal and platform upgrade rollback remain release validation items.

## Build

```sh
cargo build --locked --release --bins
cargo build --locked --release --no-default-features --bin frp-sh
```

The installer selects the lean client. Full Release assets remain available for server deployments. Default builds include `serve`. Client-only builds exclude the server and built-in TURN server. LAN still needs a matching `frp-sh-net` helper, and Windows needs Wintun in the protected installation directory.
