# Versioning Policy

The current release is **0.5.4**, signaling protocol **v3**, helper protocol **v2**. The project remains in 0.x development; incompatible protocol changes require coordinated upgrades.

Use `frp-sh --version` to inspect your client and `GET /version` to inspect the server. Protocol mismatches are rejected; different application versions with the same protocol may communicate, with a warning.

## Updates

Run `frp-sh update` explicitly to check for updates and display installation instructions. Normal startup does not automatically check or install updates. Re-run the installer to upgrade the client and helper together. Installation/maintenance may require administrator authorization; normal room operations run as an ordinary user.

Deploy the full server release asset separately. Pin versions for reproducible deployments, retain a backup, and read release notes before upgrading. Restarting the server clears its in-memory room registry.

## 0.5.4

Server admission limits are configurable through `--max-rooms`, `--max-members`, and `--max-total-members`. Earlier 0.5.3 binaries do not recognize these flags. This is a compatible addition; signaling remains v3.
