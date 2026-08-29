#!/usr/bin/env sh
# frp-sh 一键安装 / 更新脚本（Linux / macOS）
#
# 用法:
#   curl -fsSL https://frp.sh/install.sh | sh
#
# 说明:
#   - 自动检测系统与架构，从 GitHub Releases 下载最新版本
#   - 重复执行即为更新（覆盖安装）
#   - Windows 请使用 PowerShell: irm https://frp.sh/install.ps1 | iex
#
# 可选环境变量:
#   FRPSH_REPO=owner/repo   覆盖下载仓库（默认 frp-sh/frp-sh）
#   FRPSH_INSTALL_DIR=路径   覆盖安装目录（默认 /usr/local/bin）
set -e

REPO="${FRPSH_REPO:-myki-jim/frp-sh}"
BASE="https://github.com/${REPO}/releases/latest/download"
DEST_DIR="${FRPSH_INSTALL_DIR:-/usr/local/bin}"
DEST="${DEST_DIR}/frp-sh"

# 检测系统
uname_s="$(uname -s)"
case "${uname_s}" in
  Linux)  os="linux" ;;
  Darwin) os="macos" ;;
  *)
    echo "error: 不支持的平台 ${uname_s}（仅支持 Linux/macOS；Windows 请运行: irm https://frp.sh/install.ps1 | iex）" >&2
    exit 1
    ;;
esac

# 检测架构
uname_m="$(uname -m)"
case "${uname_m}" in
  x86_64 | amd64) arch="x86_64" ;;
  aarch64 | arm64) arch="aarch64" ;;
  *)
    echo "error: 不支持的架构 ${uname_m}" >&2
    exit 1
    ;;
esac

ASSET="frp-sh-${os}-${arch}"
URL="${BASE}/${ASSET}"

echo "==> frp-sh 安装器 (${os}/${arch})"
echo "    下载: ${URL}"
echo "    安装到: ${DEST}"

TMP="$(mktemp)"
if ! curl -fsSL "${URL}" -o "${TMP}"; then
  rm -f "${TMP}"
  echo "error: 下载失败 ${URL}（请确认网络与版本是否存在）" >&2
  exit 1
fi
chmod +x "${TMP}"

if [ -w "$(dirname "${DEST}")" ]; then
  mv -f "${TMP}" "${DEST}"
else
  echo "==> 需要 sudo 权限写入 ${DEST_DIR} ..."
  sudo mv -f "${TMP}" "${DEST}"
fi

VER="$("${DEST}" --version 2>/dev/null | head -1 || true)"
echo "==> 安装完成: ${DEST}${VER:+ (${VER})}"
echo "    运行 frp-sh --help 开始使用；重新执行本命令即可更新。"
