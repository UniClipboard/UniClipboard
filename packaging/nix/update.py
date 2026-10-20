#!/usr/bin/env python3
"""Rewrite sources.json from a release's SHA256SUMS.txt.

usage: update.py <version> <SHA256SUMS.txt>

Verify the signature of SHA256SUMS.txt first (SECURITY.md has the command and the public key). This script only
converts the published hex digests of the Linux .deb files into the SRI hashes that Nix expects.
"""
import base64
import json
import pathlib
import re
import sys

DEB_ARCH = {"x86_64-linux": "amd64", "aarch64-linux": "arm64"}


def main() -> None:
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    version, sums_path = sys.argv[1].removeprefix("v"), pathlib.Path(sys.argv[2])
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
        hashes[system] = "sha256-" + base64.b64encode(bytes.fromhex(digests[asset])).decode()
    target = pathlib.Path(__file__).with_name("sources.json")
    target.write_text(json.dumps({"version": version, "hashes": hashes}, indent=2) + "\n")
    print(f"{target}: {version}")


if __name__ == "__main__":
    main()
