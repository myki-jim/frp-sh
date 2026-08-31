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

# ---- colors & symbols (only when stdout is a TTY) ----
if [ -t 1 ]; then
  GREEN='\033[32m'; CYAN='\033[36m'; YELLOW='\033[33m'; RED='\033[31m'; DIM='\033[2m'; BOLD='\033[1m'; RST='\033[0m'
  OKS='✓'; WARNS='⚠'; FAILS='✗'; STEPS='→'
else
  GREEN=''; CYAN=''; YELLOW=''; RED=''; DIM=''; BOLD=''; RST=''
  OKS='[ok]'; WARNS='[warn]'; FAILS='[error]'; STEPS='[step]'
fi
say()  { printf "  ${GREEN}%s %s${RST}\n" "$OKS" "$*"; }
info() { printf "  ${DIM}%s${RST}\n" "$*"; }
warn() { printf "  ${YELLOW}%s %s${RST}\n" "$WARNS" "$*"; }
fail() { printf "  ${RED}%s %s${RST}\n" "$FAILS" "$*" >&2; exit 1; }
step() { printf "\n${BOLD}${CYAN}%s %s${RST}\n" "$STEPS" "$*"; }
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

# ---- detect OS / architecture ----
uname_s="$(uname -s)"
case "${uname_s}" in
  Linux)  os="linux" ;;
  Darwin) os="macos" ;;
  *) fail "unsupported platform ${uname_s} (Windows: irm https://frp.sh/install.ps1 | iex)" ;;
esac
uname_m="$(uname -m)"
# musl libc（OpenWrt / Alpine / 静态系统）优先选静态 musl 构建（若存在）
musl=""
case "${os}" in
  linux)
    case "${uname_m}" in
      x86_64|aarch64|arm64)
        if ls /lib/ld-musl-* >/dev/null 2>&1; then
          musl="-musl"
        fi
        ;;
    esac
    ;;
esac
case "${os}-${uname_m}" in
  linux-x86_64|linux-amd64)  asset="frp-sh-linux-x86_64${musl}" ;;
  linux-aarch64|linux-arm64) asset="frp-sh-linux-aarch64${musl}" ;;
  macos-x86_64|macos-amd64)  asset="frp-sh-macos-x86_64" ;;
  macos-arm64|macos-aarch64) asset="frp-sh-macos-arm64" ;;
  *) fail "unsupported platform/architecture ${os}/${uname_m}" ;;
esac

echo ""
printf "  ${BOLD}frp-sh installer${RST} ${DIM}(${os}/${uname_m})${RST}\n"

# ---- download ----
step "Downloading ${asset}"
# Snap 版 curl 的沙箱会隔离 /tmp：脚本在宿主 /tmp 建的临时文件它写不进，
# 静默留下 0 字节文件。下载后校验最小体积（二进制约 6MB，阈值 1MB），
# 失败自动切换下载器与下载源。
TMP="$(mktemp)"
MIN_BYTES=1000000
ok=0
for base in ${BASES}; do
  info "source ${base}"
  if command -v curl >/dev/null 2>&1; then
    if curl -fsSL "${base}/${asset}" -o "${TMP}" 2>/dev/null &&
       [ "$(wc -c < "${TMP}" | tr -d ' ')" -ge "${MIN_BYTES}" ]; then
      ok=1
      break
    fi
  fi
  if command -v wget >/dev/null 2>&1; then
    if wget -qO "${TMP}" "${base}/${asset}" 2>/dev/null &&
       [ "$(wc -c < "${TMP}" | tr -d ' ')" -ge "${MIN_BYTES}" ]; then
      ok=1
      break
    fi
  fi
  rm -f "${TMP}"
done
if [ "${ok}" != "1" ]; then
  rm -f "${TMP}"
  case "$(command -v curl 2>/dev/null)" in
    /snap/*)
      fail "download failed. You are using the Snap curl - its sandbox breaks file
  downloads into /tmp (see https://github.com/boukendesho/curl-snap/issues/1).
  Fix: sudo apt install curl && hash -r, then rerun this installer."
      ;;
    *)
      fail "download failed ${asset} (tried frp.sh and GitHub Releases; check your network)"
      ;;
  esac
fi
say "downloaded $(human "$(wc -c < "${TMP}" | tr -d ' ')")"
chmod +x "${TMP}"

# ---- install ----
step "Installing to ${DEST}"
if [ -w "$(dirname "${DEST}")" ]; then
  mv -f "${TMP}" "${DEST}"
else
  info "sudo required to write to ${DEST_DIR}"
  sudo mv -f "${TMP}" "${DEST}"
fi
VER="$("${DEST}" --version 2>/dev/null | head -1 || true)"
if [ -z "${VER}" ]; then
  fail "installed binary is not runnable (empty or corrupted download). Remove it
  with: sudo rm -f ${DEST} - then rerun this installer."
fi
say "installed ${DEST} (${VER})"

# ---- first-run setup (interactive only) ----
if [ -t 0 ] && [ -z "${FRPSH_SKIP_INIT:-}" ]; then
  CFG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/frp-sh"
  CFG="${CFG_DIR}/config.toml"
  if [ ! -f "${CFG}" ]; then
    step "First-run setup"
    echo "  Which signaling server should frp-sh use?"
    echo "  (the public VPS running 'frp-sh serve'; relay is derived as host:8081)"
    printf "  ${CYAN}Signaling server address${RST} ${DIM}(e.g. 101.43.41.195:8080, Enter to skip)${RST}: "
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
      say "config saved: ${CFG}"
    else
      warn "skipped. Run 'frp-sh config' later to set up your server."
    fi
  fi
fi

# ---- done ----
echo ""
printf "  ${BOLD}${GREEN}All done.${RST} Next steps:\n"
printf "    ${DIM}frp-sh --help${RST}        see all commands\n"
printf "    ${DIM}frp-sh lan create${RST}    host a room (needs root: creates a virtual NIC)\n"
printf "    ${DIM}frp-sh lan join 1234${RST}  join a friend's room\n"
echo ""
