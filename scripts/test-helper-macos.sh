#!/bin/sh
set -eu
[ "${GITHUB_ACTIONS:-}" = true ] && [ "$(uname -s)" = Darwin ] || exit 1
[ "$(id -u)" != 0 ] || exit 1
root="$RUNNER_TEMP/frpsh-helper"
mkdir -p "$root"
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
sudo -u nobody "$root/frp-sh" --json doctor && exit 1
exit 0
