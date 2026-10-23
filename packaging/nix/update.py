#!/usr/bin/env python3
"""Rewrite sources.json from a release's SHA256SUMS.txt.

usage: update.py <version> <SHA256SUMS.txt> <deb-directory>

Verify the signature of SHA256SUMS.txt first (SECURITY.md has the command and the public key). This script only
verifies the downloaded debs and their GTK4 ABI before updating the Nix source pin.
"""
import base64
import hashlib
import json
import pathlib
import re
import sys
import subprocess
import tempfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[2] / 'apps/gui-go/e2e'))
from package_linux import require_gtk4_binary

DEB_ARCH = {"x86_64-linux": "amd64", "aarch64-linux": "arm64"}


def verified_sources(version, sums_path, deb_directory):
    """Return a GTK4 source pin only after checking both release payloads."""
    version = version.removeprefix("v")
    sums_path, deb_directory = pathlib.Path(sums_path), pathlib.Path(deb_directory)
    digests = {}
    for line in sums_path.read_text().splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  (\S+)", line.strip())
        if match:
            digests[match.group(2)] = match.group(1)
    hashes = {}
    for system, arch in DEB_ARCH.items():
        asset = f"UniClipboard_{version}_{arch}.deb"
        if asset not in digests:
            sys.exit(f"{asset} is not listed in {sums_path}")
        deb = deb_directory / asset
        if hashlib.sha256(deb.read_bytes()).hexdigest() != digests[asset]:
            sys.exit(f"{deb}: SHA-256 differs from the verified release manifest")
        with tempfile.TemporaryDirectory(prefix='uc-nix-abi-') as directory:
            subprocess.run(['dpkg-deb', '-x', str(deb), directory], check=True)
            require_gtk4_binary(pathlib.Path(directory) / 'usr/bin/uniclipboard')
        hashes[system] = "sha256-" + base64.b64encode(bytes.fromhex(digests[asset])).decode()
    return {"version": version, "hashes": hashes, "gtkMajor": 4}


def main() -> None:
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    sources = verified_sources(*sys.argv[1:])
    target = pathlib.Path(__file__).with_name("sources.json")
    target.write_text(json.dumps(sources, indent=2) + "\n")
    print(f"{target}: {sources['version']}")


if __name__ == "__main__":
    main()
