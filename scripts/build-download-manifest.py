"""Generate a version-pinned installer manifest; fail on any inconsistent asset."""
from pathlib import Path
import hashlib
import re
root = Path(__file__).resolve().parents[1] / "web/docs/public"
version = (root / "latest-version.txt").read_text().strip()
assert re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?", version)
lines = [version]
for checksum in sorted((root / "downloads").glob("*.sha256")):
    asset = checksum.with_suffix("")
    expected = checksum.read_text().split()[0].lower()
    actual = hashlib.sha256(asset.read_bytes()).hexdigest()
    assert actual == expected, f"Checksum mismatch: {asset.name}"
    lines.append(f"{actual}  {asset.name}")
assert len(lines) == 22, "Expected 21 release assets"
(root / "release-manifest.txt").write_text("\n".join(lines) + "\n", encoding="ascii")
