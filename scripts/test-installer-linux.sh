#!/bin/sh
# Exercise installation and rollback only inside an isolated test container.
set -eu
[ "${FRPSH_CONTAINER_TEST:-}" = 1 ] && [ -f /.dockerenv ] && [ "$(id -u)" = 0 ] || exit 1
fixtures="$(mktemp -d)"
mkdir -p "$fixtures/bin" /run/systemd/system /usr/local/bin
export FRPSH_FIXTURES="$fixtures"
printf 'new-client-v1\n' > "$fixtures/client"
printf 'new-helper-v1\n' > "$fixtures/helper"
cat > "$fixtures/bin/curl" <<'MOCK'
#!/bin/sh
set -eu
out=''
while [ "$#" -gt 0 ]; do
    case "$1" in -o) out="$2"; shift 2;; *) url="$1"; shift;; esac
done
case "$url" in
    */release-manifest.txt)
        printf '0.5.0\n' > "$out"
        for kind in client net; do
            case "$kind" in client) source="$FRPSH_FIXTURES/client";; net) source="$FRPSH_FIXTURES/helper";; esac
            if [ "${FAIL_CHECKSUM:-0}" = 1 ]; then hash="$(printf '%064d' 0)"; else hash="$(sha256sum "$source" | cut -d ' ' -f 1)"; fi
            printf '%s  frp-sh-%s-linux-x86_64\n' "$hash" "$kind" >> "$out"
        done
        exit 0;;
    */releases/latest) printf 'https://github.com/myki-jim/frp-sh/releases/tag/v0.4.0\n'; exit 0;;
    *frp-sh-client-*) source="$FRPSH_FIXTURES/client";;
    *frp-sh-net-*) source="$FRPSH_FIXTURES/helper";;
    *) exit 1;;
esac
case "$url" in
    *.sha256) if [ "${FAIL_CHECKSUM:-0}" = 1 ]; then printf '%064d\n' 0 > "$out"; else sha256sum "$source" > "$out"; fi;;
    *) cp "$source" "$out";;
esac
MOCK
cat > "$fixtures/bin/systemctl" <<'MOCK'
#!/bin/sh
if [ "$1" = is-active ] && [ "${FAIL_START:-0}" = 1 ]; then touch "$FRPSH_FIXTURES/failure-reached"; exit 1; fi
exit 0
MOCK
chmod 755 "$fixtures/bin/curl" "$fixtures/bin/systemctl"
printf '#!/bin/sh\nexit 0\n' > "$fixtures/bin/ip"
chmod 755 "$fixtures/bin/ip"
export PATH="$fixtures/bin:$PATH"
# Model the actual 0.3 single-file installation layout.
printf 'legacy-client\n' > /usr/local/bin/frp-sh
FRPSH_INSTALL_UID=65534 sh /work/web/docs/public/install.sh
cmp "$fixtures/client" /usr/local/lib/frp-sh/frp-sh
cmp "$fixtures/helper" /usr/local/lib/frp-sh/frp-sh-net
test -L /usr/local/bin/frp-sh
grep legacy-client /usr/local/lib/frp-sh/frp-sh.previous >/dev/null
cp /etc/frp-sh/helper.toml "$fixtures/policy"
cp "$fixtures/client" "$fixtures/expected-client"
cp "$fixtures/helper" "$fixtures/expected-helper"
printf 'new-client-v2\n' > "$fixtures/client"
printf 'new-helper-v2\n' > "$fixtures/helper"
if FAIL_START=1 FRPSH_INSTALL_UID=1 sh /work/web/docs/public/install.sh; then exit 1; fi
test -f "$fixtures/failure-reached"
cmp "$fixtures/expected-client" /usr/local/lib/frp-sh/frp-sh
cmp "$fixtures/expected-helper" /usr/local/lib/frp-sh/frp-sh-net
cmp "$fixtures/policy" /etc/frp-sh/helper.toml
test -L /usr/local/bin/frp-sh
if FAIL_CHECKSUM=1 FRPSH_INSTALL_UID=65534 sh /work/web/docs/public/install.sh; then exit 1; fi
cmp "$fixtures/expected-client" /usr/local/lib/frp-sh/frp-sh
printf 'PASS: legacy migration, paired rollback, policy rollback, checksum rejection\n'
