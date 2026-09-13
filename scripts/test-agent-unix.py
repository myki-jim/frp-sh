#!/usr/bin/env python3
"""Exercise a real system service on a disposable GitHub-hosted Unix runner."""
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import time
import uuid

assert os.environ.get("GITHUB_ACTIONS") == "true", "CI runners only"
assert os.getuid() != 0, "Run from the ordinary runner account"
system = platform.system()
assert system in ("Linux", "Darwin")
uid = os.getuid()
installed = Path("/usr/local/lib/frp-sh")
data = Path(f"/var/lib/frp-sh/{uid}/server")
owner = Path("/etc/frp-sh/server-owner")
unit = Path("/etc/systemd/system/frp-sh-server.service" if system == "Linux"
            else "/Library/LaunchDaemons/com.frpsh.server.plist")
for path in (installed, data.parent, owner, unit):
    assert not path.exists() and not path.is_symlink(), f"Refusing existing installation: {path}"


def run(*args, **kwargs):
    return subprocess.run(args, check=True, text=True, **kwargs)


def stop():
    command = ("/bin/systemctl", "disable", "--now", "frp-sh-server.service") if system == "Linux" else (
        "/bin/launchctl", "bootout", "system/com.frpsh.server")
    subprocess.run(("sudo", *command), check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def wait_phase(binary, phase):
    deadline = time.monotonic() + 35
    while time.monotonic() < deadline:
        result = subprocess.run((str(binary), "--json", "status"), text=True, capture_output=True)
        try:
            value = json.loads(result.stdout)
            services = value if isinstance(value, list) else value.get("processes", [])
            match = next((item for item in services if item.get("role") == "server_agent"), None)
            if match and match["lifecycle"]["phase"].lower() == phase.lower():
                assert match["lifecycle"]["starts_at_boot"] is True
                return match
        except (ValueError, KeyError, TypeError):
            pass
        time.sleep(0.5)
    raise AssertionError(f"Native server never reached {phase}")


with tempfile.TemporaryDirectory(prefix="frpsh-native-service-") as temporary:
    snapshot = Path(temporary) / "snapshot.json"
    job = (f'schema_version = 1\nenabled = true\nserver = true\nprofile = ""\n'
           f'config = "{data}/config.toml"\n')
    config = (f'uuid = "{uuid.uuid4()}"\n[server]\naddr = "127.0.0.1:0"\n'
              'relay_addr = "127.0.0.1:0"\nudp_addr = "127.0.0.1:0"\n')
    blob = json.dumps(dict(uid=uid, server=True, job=job, config=config)).encode()
    snapshot.write_bytes(blob)
    snapshot.chmod(0o600)
    try:
        run("sudo", "mkdir", "-p", str(installed))
        binary = installed / "frp-sh"
        run("sudo", "cp", "target/debug/frp-sh", str(binary))
        run("sudo", "chmod", "755", str(installed), str(binary))
        run("sudo", str(binary), "--plain", "agent", "install-elevated", "--snapshot", str(snapshot),
            "--digest", hashlib.sha256(blob).hexdigest())
        status = wait_phase(binary, "serving")
        actual_uid = run("ps", "-o", "uid=", "-p", str(status["process_id"]), capture_output=True).stdout.strip()
        assert int(actual_uid) == uid, "Supervisor must never run as root"
        for action, phase in (("stop", "stopped"), ("start", "serving")):
            run(str(binary), "--plain", "agent", action, "--job", str(data / "job.toml"))
            wait_phase(binary, phase)
        # SCM/launchd restart, while the ordinary user has no interactive app running.
        if system == "Linux":
            run("sudo", "/bin/systemctl", "restart", "frp-sh-server.service")
        else:
            run("sudo", "/bin/launchctl", "kickstart", "-k", "system/com.frpsh.server")
        wait_phase(binary, "serving")
        assert (data / "logs").is_dir(), "Service logs must use the installed data directory"
        print("Native non-root service, ordinary-user controls, restart, and logs passed")
    finally:
        stop()
        # Every target is a fixed, previously absent installation owned by this CI fixture.
        run("sudo", "rm", "-f", str(unit), str(owner))
        run("sudo", "rm", "-rf", str(installed), str(data.parent))
        if system == "Linux":
            run("sudo", "/bin/systemctl", "daemon-reload")
