# Installation & Building

## Requirements

- **Rust**: 1.70+ (install the latest stable via rustup)
- **OS**: Windows / Linux / macOS (UDP behavior is largely the same; Windows WSAECONNRESET quirks are handled)

## Option 1: Build from source (recommended)

```bash
git clone <your-repo-url> frp-sh
cd frp-sh

# Debug build
cargo build

# Release build (recommended for distribution)
cargo build --release
```

Artifacts:

```text
target/release/frp-sh.exe   # Windows
target/release/frp-sh       # Linux / macOS
```

The release binary is about **5.4 MB** (stripped + LTO), a single file with no runtime dependencies.

## Option 2: Build without a repo clone

Copy `src/`, `Cargo.toml`, and `Cargo.lock` into any directory and build there.

## Install to PATH

```bash
# Linux / macOS
sudo cp target/release/frp-sh /usr/local/bin/frp-sh

# Windows
copy target\release\frp-sh.exe %USERPROFILE%\bin\
```

Now `frp-sh` is available directly.

## Verify

```bash
frp-sh --help
frp-sh game --help
frp-sh serve --help
```

## Building on a server (no local Rust)

The server only needs the signaling binary; build it directly on the box:

```bash
# Install Rust (~1 min)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
export PATH="$HOME/.cargo/bin:$PATH"

# Upload the code, then build
cd /opt/frpsh
cargo build --release
```

A full build takes about 2.5 minutes on a 2-core VPS.

## Platform notes

| Platform | Notes |
|----------|-------|
| Windows | 10054 (ICMP poisoning) false errors on send/recv are handled |
| Linux | nothing extra needed |
| macOS | UDP behavior matches Linux |
