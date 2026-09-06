#!/bin/sh
set -eu
[ "${GITHUB_ACTIONS:-}" = true ] && [ "$(uname -s)" = Darwin ] || exit 1
[ "$(id -u)" != 0 ] || exit 1
root="$(mktemp -d /private/tmp/frpsh-helper.XXXXXXXX)"
chmod 755 "$root"
cp target/debug/frp-sh target/debug/frp-sh-net "$root/"
sudo mkdir -p /etc/frp-sh
printf 'allowed_uid = %s\n' "$(id -u)" | sudo tee /etc/frp-sh/helper.toml >/dev/null
sudo chmod 600 /etc/frp-sh/helper.toml
sudo "$root/frp-sh-net" >"$root/helper.log" 2>&1 &
pid=$!
trap 'sudo kill "$pid" 2>/dev/null || true' EXIT INT TERM
sleep 2
"$root/frp-sh" --json doctor --network-test
sleep 1
sudo -u nobody "$root/frp-sh" --version >/dev/null
if sudo -u nobody "$root/frp-sh" --json doctor >"$root/outsider.json" 2>&1; then exit 1; fi
grep 'E_RUNTIME' "$root/outsider.json" >/dev/null
exit 0
