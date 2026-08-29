#!/usr/bin/env sh
# frp-sh 一键安装 / 更新脚本（Linux / macOS）
#
# 用法:
#   curl -fsSL https://frp.sh/install.sh | sh
#
# 说明:
#   - 自动检测系统与架构，下载最新版本（frp.sh 官方源优先，GitHub Releases 兜底）
#   - 重复执行即为更新（覆盖安装）
#   - Windows 请使用 PowerShell: irm https://frp.sh/install.ps1 | iex
#
# 可选环境变量:
#   FRPSH_REPO=owner/repo   覆盖 GitHub 兜底仓库（默认 myki-jim/frp-sh）
#   FRPSH_INSTALL_DIR=路径   覆盖安装目录（默认 /usr/local/bin）
set -e

REPO="${FRPSH_REPO:-myki-jim/frp-sh}"
# 下载源：Cloudflare 官方源优先（国内可达），GitHub Releases 兜底
BASES="https://frp.sh/downloads https://github.com/${REPO}/releases/latest/download"
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
# 检测架构并确定资产名（注意：Linux 用 aarch64，macOS 用 arm64）
case "${os}-${uname_m}" in
  linux-x86_64 | linux-amd64)    asset="frp-sh-linux-x86_64" ;;
  linux-aarch64 | linux-arm64)   asset="frp-sh-linux-aarch64" ;;
  macos-x86_64 | macos-amd64)    asset="frp-sh-macos-x86_64" ;;
  macos-arm64 | macos-aarch64)   asset="frp-sh-macos-arm64" ;;
  *)
    echo "error: 不支持的平台/架构 ${os}/${uname_m}" >&2
    exit 1
    ;;
esac

echo "==> frp-sh 安装器 (${os}/${uname_m})"
echo "    安装到: ${DEST}"

TMP="$(mktemp)"
ok=0
for base in ${BASES}; do
  echo "    尝试: ${base}/${asset}"
  if curl -fsSL "${base}/${asset}" -o "${TMP}"; then
    ok=1
    break
  fi
done
if [ "${ok}" != "1" ]; then
  rm -f "${TMP}"
  echo "error: 下载失败 ${asset}（已尝试 frp.sh 与 GitHub Releases，请检查网络）" >&2
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
