#!/bin/sh
# frp-sh 0.4: installation authorization, ordinary-user runtime.
set -eu
lang="${FRPSH_LANG:-${LC_ALL:-${LANG:-en}}}"
say() { case "$lang" in zh*) printf '%s\n' "$2";; *) printf '%s\n' "$1";; esac; }
if [ "$(id -u)" != 0 ]; then
    say "Installing the network helper requires one administrator approval." "安装网络辅助服务需要一次管理员授权。"
    bootstrap="$(mktemp)"
    trap 'rm -f "$bootstrap"' EXIT HUP INT TERM
    curl -fL --proto '=https' --tlsv1.2 --connect-timeout 5 --max-time 30 --retry 1 https://frp.sh/install.sh -o "$bootstrap"
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
    Darwin) [ "$arch" != aarch64 ] || arch=arm64; suffix="macos-$arch"; manager=launchd;;
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
rollback=0
case "$manager" in
    systemd) unit=/etc/systemd/system/frp-sh-network.service;;
    launchd) unit=/Library/LaunchDaemons/com.frpsh.network.plist;;
    procd) unit=/etc/init.d/frp-sh-network;;
esac
stop_service() {
    case "$manager" in
        systemd) systemctl stop frp-sh-network.service 2>/dev/null || true;;
        launchd) launchctl bootout system/com.frpsh.network 2>/dev/null || true;;
        procd) /etc/init.d/frp-sh-network stop 2>/dev/null || true;;
    esac
}
cleanup() {
    result=$?
    trap - EXIT HUP INT TERM
    if [ "$rollback" = 1 ]; then
        stop_service
        for item in client helper policy unit; do
            case "$item" in client) path="$dest/frp-sh";; helper) path="$dest/frp-sh-net";; policy) path=/etc/frp-sh/helper.toml;; unit) path="$unit";; esac
            if [ -f "$stage/previous-$item" ]; then cp -p "$stage/previous-$item" "$path"; else rm -f "$path"; fi
        done
        if [ -f "$stage/previous-legacy" ]; then
            rm -f /usr/local/bin/frp-sh
            cp -p "$stage/previous-legacy" /usr/local/bin/frp-sh
        elif [ -f "$stage/previous-link" ]; then
            ln -sf "$(cat "$stage/previous-link")" /usr/local/bin/frp-sh
        else rm -f /usr/local/bin/frp-sh; fi
        if [ -f "$stage/previous-unit" ]; then
            case "$manager" in
                systemd) systemctl daemon-reload; systemctl start frp-sh-network.service || true;;
                launchd) launchctl bootstrap system "$unit" || true;;
                procd) /etc/init.d/frp-sh-network start || true;;
            esac
        fi
        say "Installation failed; the previous files were restored." "安装失败，已恢复原有文件。"
    fi
    case "$stage" in "$dest"/staging.*) rm -rf "$stage";; *) printf 'Unsafe staging path\n'; exit 1;; esac
    exit "$result"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
# One manifest pins both binaries even while the website publishes a new release.
manifest="$stage/release-manifest.txt"
site=https://frp.sh/downloads
if curl -fsSL --proto '=https' --proto-redir '=https' --tlsv1.2 --connect-timeout 5 --max-time 10 https://frp.sh/release-manifest.txt -o "$manifest"; then
    tag="v$(sed -n '1p' "$manifest" | tr -d '\r')"
else
    rm -f "$manifest"
    release_url="$(curl -fsSL --proto '=https' --proto-redir '=https' --tlsv1.2 --connect-timeout 5 --max-time 15 -o /dev/null -w '%{url_effective}' https://github.com/myki-jim/frp-sh/releases/latest)"
    case "$release_url" in https://github.com/myki-jim/frp-sh/releases/tag/v[0-9]*) ;; *) exit 1;; esac
    tag="${release_url##*/}"
fi
printf '%s\n' "$tag" | LC_ALL=C grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[A-Za-z0-9.-]+)?$' || { printf 'Invalid release version\n'; exit 1; }
base="https://github.com/myki-jim/frp-sh/releases/download/$tag"
fetch() {
    asset="$1"; out="$2"
    if [ -f "$manifest" ]; then
        expected="$(awk -v name="$asset" 'NR > 1 && $2 == name {print $1}' "$manifest")"
    else
        curl -fsSL --proto '=https' --proto-redir '=https' --tlsv1.2 --connect-timeout 5 --max-time 15 "$base/$asset.sha256" -o "$out.sha256" || return 1
        expected="$(awk '{print $1}' "$out.sha256")"
    fi
    case "$expected" in *[!a-fA-F0-9]*|'') printf 'Invalid checksum\n'; return 1;; esac
    [ "${#expected}" = 64 ] || return 1
    expected="$(printf '%s' "$expected" | tr 'A-F' 'a-f')"
    for source in "$site" "$base"; do
        if curl -fL --proto '=https' --proto-redir '=https' --tlsv1.2 --connect-timeout 5 --max-time 90 --speed-limit 1024 --speed-time 10 --retry 1 --retry-delay 1 "$source/$asset" -o "$out"; then
            if command -v sha256sum >/dev/null; then actual="$(sha256sum "$out" | awk '{print $1}')"
            else actual="$(shasum -a 256 "$out" | awk '{print $1}')"; fi
            if [ "$actual" = "$expected" ]; then chmod 755 "$out"; return 0; fi
        fi
        rm -f "$out"
        say "Download failed verification or timed out; trying another source..." "下载超时或校验未通过，正在切换下载源…"
    done
    printf 'Could not download verified asset: %s\n' "$asset"; return 1
}
say "Downloading and verifying client and network helper..." "正在下载并校验客户端和网络辅助程序…"
fetch "frp-sh-client-$suffix" "$stage/frp-sh"
fetch "frp-sh-net-$suffix" "$stage/frp-sh-net"
for item in client helper policy unit; do
    case "$item" in client) path="$dest/frp-sh";; helper) path="$dest/frp-sh-net";; policy) path=/etc/frp-sh/helper.toml;; unit) path="$unit";; esac
    [ ! -f "$path" ] || cp -p "$path" "$stage/previous-$item"
done
if [ -L /usr/local/bin/frp-sh ]; then readlink /usr/local/bin/frp-sh > "$stage/previous-link"
elif [ -f /usr/local/bin/frp-sh ]; then
    # The 0.3 installer placed its executable here directly.
    cp -p /usr/local/bin/frp-sh "$stage/previous-legacy"
    cp -p /usr/local/bin/frp-sh "$dest/frp-sh.previous"
elif [ -e /usr/local/bin/frp-sh ]; then printf 'Unexpected installation path type\n'; exit 1; fi
rollback=1
stop_service
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
rollback=0
say "Installed. Run frp-sh from your normal account." "安装完成。请使用普通账户运行 frp-sh。"
