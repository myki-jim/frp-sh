#!/usr/bin/env sh
# frp-sh one-line installer / updater (Linux / macOS)
#
# Usage:
#   curl -fsSL https://frp.sh/install.sh | sh
#
# Notes:
#   - auto-detects OS and architecture, downloads the latest release
#     (frp.sh official source first, GitHub Releases as fallback)
#   - re-running updates in place (overwrites the binary)
#   - Windows: use PowerShell: irm https://frp.sh/install.ps1 | iex
#
# Optional environment variables:
#   FRPSH_REPO=owner/repo     override the GitHub fallback repo (default myki-jim/frp-sh)
#   FRPSH_INSTALL_DIR=path    override the install directory (default /usr/local/bin)
set -e

REPO="${FRPSH_REPO:-myki-jim/frp-sh}"
# Download sources: Cloudflare official source first, GitHub Releases fallback
BASES="https://frp.sh/downloads https://github.com/${REPO}/releases/latest/download"
DEST_DIR="${FRPSH_INSTALL_DIR:-/usr/local/bin}"
DEST="${DEST_DIR}/frp-sh"

# Detect OS
uname_s="$(uname -s)"
case "${uname_s}" in
  Linux)  os="linux" ;;
  Darwin) os="macos" ;;
  *)
    echo "error: unsupported platform ${uname_s} (only Linux/macOS; Windows: run irm https://frp.sh/install.ps1 | iex)" >&2
    exit 1
    ;;
esac

# Detect architecture and pick the asset name (note: Linux uses aarch64, macOS uses arm64)
uname_m="$(uname -m)"
case "${os}-${uname_m}" in
  linux-x86_64 | linux-amd64)    asset="frp-sh-linux-x86_64" ;;
  linux-aarch64 | linux-arm64)   asset="frp-sh-linux-aarch64" ;;
  macos-x86_64 | macos-amd64)    asset="frp-sh-macos-x86_64" ;;
  macos-arm64 | macos-aarch64)   asset="frp-sh-macos-arm64" ;;
  *)
    echo "error: unsupported platform/architecture ${os}/${uname_m}" >&2
    exit 1
    ;;
esac

echo "==> frp-sh installer (${os}/${uname_m})"
echo "    installing to: ${DEST}"

TMP="$(mktemp)"
ok=0
for base in ${BASES}; do
  echo "    trying: ${base}/${asset}"
  if curl -fsSL "${base}/${asset}" -o "${TMP}"; then
    ok=1
    break
  fi
done
if [ "${ok}" != "1" ]; then
  rm -f "${TMP}"
  echo "error: download failed ${asset} (tried frp.sh and GitHub Releases; check your network)" >&2
  exit 1
fi
chmod +x "${TMP}"

if [ -w "$(dirname "${DEST}")" ]; then
  mv -f "${TMP}" "${DEST}"
else
  echo "==> sudo required to write to ${DEST_DIR} ..."
  sudo mv -f "${TMP}" "${DEST}"
fi

VER="$("${DEST}" --version 2>/dev/null | head -1 || true)"
echo "==> installed: ${DEST}${VER:+ (${VER})}"
echo "    run 'frp-sh --help' to get started; re-run this command to update."

# First-run initialization: ask for the signaling server and write the config.
# Only when running interactively (stdin is a TTY); `read < /dev/tty` works even
# under `curl | sh` (piped stdin is the script itself).
if [ -t 0 ] && [ -z "${FRPSH_SKIP_INIT:-}" ]; then
  CFG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/frp-sh"
  CFG="${CFG_DIR}/config.toml"
  if [ ! -f "${CFG}" ]; then
    echo ""
    echo "==> First run setup: which signaling server should frp-sh use?"
    echo "    (the public VPS running 'frp-sh serve'; relay is derived as host:8081)"
    printf '    Signaling server address (e.g. 101.43.41.195:8080) [Enter to skip]: '
    SIG=""
    read -r SIG < /dev/tty || true
    if [ -n "${SIG}" ]; then
      case "${SIG}" in
        http://*|https://*) : ;;
        *) SIG="http://${SIG}" ;;
      esac
      HOST_ONLY="$(printf '%s' "${SIG}" | sed -E 's#^https?://##; s#:.*##')"
      mkdir -p "${CFG_DIR}"
      printf 'signaling_addr = "%s"\nrelay_addr = "%s:8081"\n' "${SIG}" "${HOST_ONLY}" > "${CFG}"
      echo "    config saved: ${CFG}"
      echo "    ready to go. Run 'frp-sh' to get started."
    else
      echo "    skipped. Run 'frp-sh config' later to set up your server."
    fi
  fi
fi

echo "    note: lan mesh mode needs root (creates a virtual NIC)."
