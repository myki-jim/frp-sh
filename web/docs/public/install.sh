#!/usr/bin/env sh
# frp-sh one-line installer / updater (Linux / macOS)
#
#   curl -fsSL https://frp.sh/install.sh | sh
#
# Env:
#   FRPSH_REPO         GitHub fallback repo (default myki-jim/frp-sh)
#   FRPSH_INSTALL_DIR  install dir (default /usr/local/bin)
#   FRPSH_SKIP_INIT=1  skip interactive first-run setup
set -e

REPO="${FRPSH_REPO:-myki-jim/frp-sh}"
DEST_DIR="${FRPSH_INSTALL_DIR:-/usr/local/bin}"
DEST="${DEST_DIR}/frp-sh"
# Download sources: Cloudflare official source first, GitHub Releases fallback
BASES="https://frp.sh/downloads https://github.com/${REPO}/releases/latest/download"

# ---- colors (only when stdout is a TTY) ----
if [ -t 1 ]; then
  GREEN='\033[32m'; CYAN='\033[36m'; YELLOW='\033[33m'; RED='\033[31m'; DIM='\033[2m'; BOLD='\033[1m'; RST='\033[0m'
else
  GREEN=''; CYAN=''; YELLOW=''; RED=''; DIM=''; BOLD=''; RST=''
fi
say()  { printf "${GREEN}%s${RST}\n" "$*"; }
info() { printf "${CYAN}%s${RST}\n" "$*"; }
warn() { printf "${YELLOW}%s${RST}\n" "$*"; }
fail() { printf "${RED}%s${RST}\n" "$*" >&2; exit 1; }
step() { printf "\n${BOLD}%s${RST}\n" "$*"; printf "${DIM}----------------------------------------${RST}\n"; }
human() { b=$1; if [ "$b" -ge 1048576 ]; then echo "$((b/1048576)) MB"; elif [ "$b" -ge 1024 ]; then echo "$((b/1024)) KB"; else echo "${b} B"; fi; }

# ---- banner ----
printf "${GREEN}"
printf '%s\n' \
  ' ______ _____  _____   _____ _    _ ' \
  '|  ____|  __ \|  __ \ / ____| |  | |' \
  '| |__  | |__) | |__) | (___ | |__| |' \
  '|  __| |  _  /|  ___/ \___ \|  __  |' \
  '| |    | | \ \| |     ____) | |  | |' \
  '|_|    |_|  \_\_|    |_____/|_|  |_|'
printf "${RST}"
echo ""

# ---- detect OS / architecture ----
uname_s="$(uname -s)"
case "${uname_s}" in
  Linux)  os="linux" ;;
  Darwin) os="macos" ;;
  *) fail "error: unsupported platform ${uname_s} (Windows: irm https://frp.sh/install.ps1 | iex)" ;;
esac
uname_m="$(uname -m)"
case "${os}-${uname_m}" in
  linux-x86_64|linux-amd64)  asset="frp-sh-linux-x86_64" ;;
  linux-aarch64|linux-arm64) asset="frp-sh-linux-aarch64" ;;
  macos-x86_64|macos-amd64)  asset="frp-sh-macos-x86_64" ;;
  macos-arm64|macos-aarch64) asset="frp-sh-macos-arm64" ;;
  *) fail "error: unsupported platform/architecture ${os}/${uname_m}" ;;
esac

echo "  ${BOLD}frp-sh installer${RST}  (${os}/${uname_m})"
echo ""

# ---- 1/3 download ----
step "  [1/3] Downloading ${asset}"
TMP="$(mktemp)"
ok=0
for base in ${BASES}; do
  info "    trying ${base}/${asset} ..."
  if curl -fsSL "${base}/${asset}" -o "${TMP}"; then
    ok=1
    break
  fi
done
if [ "${ok}" != "1" ]; then
  rm -f "${TMP}"
  fail "error: download failed ${asset} (tried frp.sh and GitHub Releases; check your network)"
fi
say "    [OK] downloaded $(human "$(wc -c < "${TMP}" | tr -d ' ')")"
chmod +x "${TMP}"

# ---- 2/3 install ----
step "  [2/3] Installing to ${DEST}"
if [ -w "$(dirname "${DEST}")" ]; then
  mv -f "${TMP}" "${DEST}"
else
  info "    sudo required to write to ${DEST_DIR} ..."
  sudo mv -f "${TMP}" "${DEST}"
fi
VER="$("${DEST}" --version 2>/dev/null | head -1 || true)"
say "    [OK] installed: ${DEST}${VER:+ (${VER})}"

# ---- 3/3 first-run setup (interactive only) ----
if [ -t 0 ] && [ -z "${FRPSH_SKIP_INIT:-}" ]; then
  CFG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/frp-sh"
  CFG="${CFG_DIR}/config.toml"
  if [ ! -f "${CFG}" ]; then
    step "  [3/3] First-run setup"
    echo "    Which signaling server should frp-sh use?"
    echo "    (the public VPS running 'frp-sh serve'; relay is derived as host:8081)"
    printf "    ${CYAN}Signaling server address${RST} (e.g. 101.43.41.195:8080) [Enter to skip]: "
    SIG=""
    # read from the TTY so this works under `curl ... | sh` (piped stdin is the script)
    read -r SIG < /dev/tty || true
    if [ -n "${SIG}" ]; then
      case "${SIG}" in
        http://*|https://*) : ;;
        *) SIG="http://${SIG}" ;;
      esac
      HOST_ONLY="$(printf '%s' "${SIG}" | sed -E 's#^https?://##; s#:.*##')"
      mkdir -p "${CFG_DIR}"
      printf 'signaling_addr = "%s"\nrelay_addr = "%s:8081"\n' "${SIG}" "${HOST_ONLY}" > "${CFG}"
      say "    [OK] config saved: ${CFG}"
      say "    ready to go. Run 'frp-sh' to get started."
    else
      warn "    skipped. Run 'frp-sh config' later to set up your server."
    fi
  fi
fi

echo ""
say "  All done. Run 'frp-sh --help' to get started."
info "  tip: lan mesh mode needs root (creates a virtual NIC)."
