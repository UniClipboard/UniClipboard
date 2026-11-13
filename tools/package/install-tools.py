#!/usr/bin/env python3
"""Install the pinned, hash-verified nFPM executable into a task-owned directory."""

import argparse
import hashlib
import io
import json
from pathlib import Path
import platform
import tarfile
from urllib.request import urlopen
import zipfile

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--output", required=True)
p.add_argument(
    "--tool", choices=["nfpm", "appimagetool", "appimage-runtime"], default="nfpm"
)
a = p.parse_args()
root = Path(__file__).resolve().parents[2]
lock = json.loads((root / "packaging/tools.json").read_text())[a.tool]
os_name = platform.system()
arch = {
    "aarch64": "arm64",
    "arm64": "arm64",
    "x86_64": "x86_64",
    "AMD64": "x86_64",
    "ARM64": "arm64",
}[platform.machine()]
name = f"nfpm_{lock['version']}_{os_name}_{arch}" + (
    ".zip" if os_name == "Windows" else ".tar.gz"
)
if a.tool == "nfpm":
    expected = lock["sha256"][name]
    url = f"https://github.com/goreleaser/nfpm/releases/download/v{lock['version']}/{name}"
else:
    if os_name != "Linux":
        raise ValueError("AppImage tools require Linux")
    asset = lock["assets"]["arm64" if arch == "arm64" else "amd64"]
    expected = asset["sha256"]
    url = asset["url"]
output = Path(a.output).resolve()
if output.exists():
    raise ValueError("tool output already exists")
with urlopen(url, timeout=60) as response:
    data = response.read(64 * 1024 * 1024 + 1)
if len(data) > 64 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != expected:
    raise ValueError("nFPM download digest mismatch")
if a.tool != "nfpm":
    executable = data
elif os_name == "Windows":
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        executable = z.read("nfpm.exe")
else:
    with tarfile.open(fileobj=io.BytesIO(data)) as t:
        entry = t.getmember("nfpm")
        if not entry.isfile():
            raise ValueError("invalid tool archive")
        executable = t.extractfile(entry).read()
output.parent.mkdir(parents=True, exist_ok=True)
with output.open("xb") as stream:
    stream.write(executable)
output.chmod(0o755)
print(output)
