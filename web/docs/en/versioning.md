# Versioning Policy

## Current stage: development (0.x)

frp-sh is in the **0.x development stage** (in semver, `major=0` means not yet stable):

- **Features and protocol may change**: before 1.0 we keep adding capabilities and
  revising designs; backward compatibility is not promised
- **Fast iteration**: 0.x ships almost weekly (currently v0.3.x); follow along
- **1.0 is the first stable release**: we only bump to 1.0 once the bar below is met

## The 1.0 bar

1. **Feature freeze**: core capabilities (rooms / punching / mesh / update
   governance) stop evolving every week
2. **Protocol freeze**: the wire protocol version becomes stable
3. **Cross-platform stability**: virtual NIC, firewall, and permission handling
   proven on Windows / macOS / Linux in real environments
4. **Complete docs**: CLI reference, FAQ, and deployment guides all up to date

## Version governance during development (the safety net)

0.x may change, but we keep changes controlled:

### Wire protocol version (PROTOCOL_VERSION)

- Every build carries a protocol version (currently 1)
- **Backward-compatible additions** (e.g. new optional fields) do not bump it
- **Breaking changes** (route changes, frame format, semantics) must bump it, with
  release notes
- Clients verify the protocol version against the signaling server before use:
  **mismatches refuse to run** (no confusing failures from mixing old and new)
- Servers expose version + protocol via `GET /version`

### Update checks

- Every run prints the current version and protocol (`frp-sh v0.3.7 (protocol v1)`)
- Startup checks for new versions (`frp.sh/latest-version.txt`, throttled to 6h)
- When a new version exists it asks `[Y/n]`:
  - **Smooth upgrade** (same major, minor diff ≤ 1) → can skip; prompted again next start
  - **Big gap** (different major, or minor diff ≥ 2) → cannot skip; update strongly recommended
- On connect, server / host versions are shown:
  - different but protocol-compatible → **yellow** notice
  - protocol conflict → **red** error and refusal
  - server unreachable → **red** error

## Your choice

| Need | Suggestion |
|------|------------|
| Try new features | Use the latest (`curl -fsSL https://frp.sh/install.sh \| sh` updates in place) |
| Stable deployment (server) | Pin a version and watch update notices; upgrade when the gap is big |

## Release cadence

- `0.x`: feature work and fixes, shipped whenever ready (currently v0.3.x: connection
  profiles / one-click join / panel logs / punch downgrade strategy)
- `1.0.0`: first stable release; semver applies from then on (`1.x` compatible,
  `2.0` breaking)
