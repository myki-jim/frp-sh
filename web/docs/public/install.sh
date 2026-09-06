#!/bin/sh
# frp-sh 0.4: installation authorization, ordinary-user runtime.
set -eu
lang="${FRPSH_LANG:-${LC_ALL:-${LANG:-en}}}"
say() { case "$lang" in zh*) printf '%s\n' "$2";; *) printf '%s\n' "$1";; esac; }
if [ "$(id -u)" != 0 ]; then
    say "Installing the network helper requires one administrator approval." "安装网络辅助服务需要一次管理员授权。"
    bootstrap="$(mktemp)"
    trap 'rm -f "$bootstrap"' EXIT HUP INT TERM
    curl -fL --proto '=https' --tlsv1.2 https://frp.sh/install.sh -o "$bootstrap"
    sudo env FRPSH_INSTALL_UID="$(id -u)" FRPSH_LANG="$lang" sh "$bootstrap"
    say "Installed. LAN sessions do not need sudo." "安装完成，LAN 会话无需 sudo。"
    exit 0
fi
owner="${FRPSH_INSTALL_UID:-${SUDO_UID:-}}"
case "$owner" in ''|*[!0-9]*) say "Set FRPSH_INSTALL_UID to the account that will use frp-sh." "请用 FRPSH_INSTALL_UID 指定使用 frp-sh 的账户 UID。"; exit 1;; esac
id "$owner" >/dev/null 2>&1 || { printf 'Unknown user ID: %s\n' "$owner"; exit 1; }
os="$(uname -s)"; arch="$(uname -m)"
case "$arch" in x86_64|amd64) arch=x86_64;; aarch64|arm64) arch=aarch64;; *) printf 'Unsupported architecture: %s\n' "$arch"; exit 1;; esac
case "$os" in
    Darwin) suffix="macos-$arch"; manager=launchd;;
    Linux)
        suffix="linux-$arch"
        if ldd --version 2>&1 | grep -qi musl || [ -f /etc/openwrt_release ]; then suffix="$suffix-musl"; fi
        if [ -d /run/systemd/system ]; then manager=systemd
        elif [ -x /sbin/procd ]; then manager=procd
        else printf 'No supported service manager (systemd/procd).\n'; exit 1; fi
        command -v ip >/dev/null || { printf 'Install iproute2 first.\n'; exit 1; }
        ;;
    *) printf 'Unsupported OS: %s\n' "$os"; exit 1;;
esac
dest=/usr/local/lib/frp-sh
umask 022
mkdir -p "$dest" /usr/local/bin /etc/frp-sh
chmod 755 "$dest" /etc/frp-sh
stage="$(mktemp -d "$dest/staging.XXXXXXXX")"
trap 'rm -rf "$stage"' EXIT HUP INT TERM
base=https://github.com/myki-jim/frp-sh/releases/latest/download
fetch() {
    asset="$1"; out="$2"
    curl -fL --proto '=https' --tlsv1.2 "$base/$asset" -o "$out"
    curl -fL --proto '=https' --tlsv1.2 "$base/$asset.sha256" -o "$out.sha256"
    expected="$(awk '{print $1}' "$out.sha256")"
    case "$expected" in *[!a-fA-F0-9]*|'') printf 'Invalid checksum\n'; exit 1;; esac
    [ "${#expected}" = 64 ] || exit 1
    if command -v sha256sum >/dev/null; then actual="$(sha256sum "$out" | awk '{print $1}')"
    else actual="$(shasum -a 256 "$out" | awk '{print $1}')"; fi
    [ "$actual" = "$expected" ] || { printf 'Checksum mismatch: %s\n' "$asset"; exit 1; }
    chmod 755 "$out"
}
say "Downloading and verifying client and network helper..." "正在下载并校验客户端和网络辅助程序…"
fetch "frp-sh-$suffix" "$stage/frp-sh"
fetch "frp-sh-net-$suffix" "$stage/frp-sh-net"
case "$manager" in
    systemd) systemctl stop frp-sh-network.service 2>/dev/null || true;;
    launchd) launchctl bootout system/com.frpsh.network 2>/dev/null || true;;
    procd) /etc/init.d/frp-sh-network stop 2>/dev/null || true;;
esac
for name in frp-sh frp-sh-net; do
    [ ! -f "$dest/$name" ] || cp -p "$dest/$name" "$dest/$name.previous"
    mv -f "$stage/$name" "$dest/$name"
done
printf 'allowed_uid = %s\n' "$owner" > /etc/frp-sh/helper.toml
chmod 600 /etc/frp-sh/helper.toml
ln -sf "$dest/frp-sh" /usr/local/bin/frp-sh
case "$manager" in
systemd)
    cat > /etc/systemd/system/frp-sh-network.service <<'UNIT'
[Unit]
Description=frp-sh restricted network helper
After=network.target
[Service]
Type=simple
ExecStart=/usr/local/lib/frp-sh/frp-sh-net
Restart=on-failure
RestartSec=3
User=root
RuntimeDirectory=frp-sh
RuntimeDirectoryMode=0755
NoNewPrivileges=true
CapabilityBoundingSet=CAP_NET_ADMIN CAP_CHOWN
ProtectHome=true
PrivateTmp=true
RestrictAddressFamilies=AF_UNIX AF_INET AF_NETLINK
DevicePolicy=closed
DeviceAllow=/dev/net/tun rw
[Install]
WantedBy=multi-user.target
UNIT
    systemctl daemon-reload
    systemctl enable --now frp-sh-network.service
    systemctl is-active --quiet frp-sh-network.service
    ;;
launchd)
    cat > /Library/LaunchDaemons/com.frpsh.network.plist <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>com.frpsh.network</string>
<key>ProgramArguments</key><array><string>/usr/local/lib/frp-sh/frp-sh-net</string></array>
<key>RunAtLoad</key><true/><key>KeepAlive</key><true/>
</dict></plist>
PLIST
    chmod 644 /Library/LaunchDaemons/com.frpsh.network.plist
    launchctl bootstrap system /Library/LaunchDaemons/com.frpsh.network.plist
    ;;
procd)
    cat > /etc/init.d/frp-sh-network <<'INIT'
#!/bin/sh /etc/rc.common
START=95
USE_PROCD=1
start_service() {
    procd_open_instance
    procd_set_param command /usr/local/lib/frp-sh/frp-sh-net
    procd_set_param respawn
    procd_close_instance
}
INIT
    chmod 755 /etc/init.d/frp-sh-network
    /etc/init.d/frp-sh-network enable
    /etc/init.d/frp-sh-network start
    ;;
esac
say "Installed. Run frp-sh from your normal account." "安装完成。请使用普通账户运行 frp-sh。"
