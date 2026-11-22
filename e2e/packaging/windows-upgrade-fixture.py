#!/usr/bin/env python3
"""Construct an explicitly non-release later-build NSIS fixture from the same CLI lock."""
import argparse
from pathlib import Path
import re
import subprocess
import sys

p = argparse.ArgumentParser()
p.add_argument("--target", required=True)
p.add_argument("--build-number", required=True, type=int)
a = p.parse_args()
root = Path(__file__).resolve().parents[2]
out = root / "dist/windows-upgrade-fixture"
out.mkdir(parents=True)
metadata = out / "products.toml"
data = (root / "release/products.toml").read_text()
head, desktop = data.split("[products.desktop]", 1)
body, tail = desktop.split("[products.android]", 1)
body, replacements = re.subn(
    r"(?m)^build_number = [1-9][0-9]*$",
    f"build_number = {a.build_number + 1}",
    body,
)
if replacements != 1:
    raise ValueError("expected one Desktop build number")
metadata.write_text(head + "[products.desktop]" + body + "[products.android]" + tail)
for argv in [
    [
        "stage",
        "--metadata",
        metadata,
        "--target",
        a.target,
        "--cli-manifest",
        root / "dist/cli-input/release-manifest.json",
        "--output",
        out / "stage",
        "--development",
    ],
    [
        "package",
        "--stage",
        out / "stage",
        "--format",
        "setup.exe",
        "--output",
        out / "packages",
    ],
]:
    subprocess.run(
        [sys.executable, str(root / "tools/package/desktop.py"), *map(str, argv)],
        cwd=root,
        check=True,
    )
