#!/usr/bin/env python3
"""Repackage the candidate payload with the previous Go identity, using the maintained builders.

This is a generated transaction regression fixture, not a published historical artifact.
"""

import argparse
import json
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import package_linux as package

parser = argparse.ArgumentParser()
parser.add_argument("--kind", choices=["deb", "rpm"], required=True)
parser.add_argument("--arch", choices=["amd64", "arm64"], required=True)
parser.add_argument("--package", type=Path, required=True)
parser.add_argument("--version", help="Override the fixture version for transaction boundary checks")
parser.add_argument("--out", type=Path, required=True)
a = parser.parse_args()
a.out.mkdir(parents=True, exist_ok=False)
stage = a.out / "stage"
stage.mkdir()
if a.kind == "deb":
    version = package.run(["dpkg-deb", "-f", str(a.package), "Version"], capture=True)
    package.run(["dpkg-deb", "-x", str(a.package), str(stage)])
    built = package.build_deb(
        stage,
        a.out,
        a.version or version,
        a.arch,
        "legacy-go.deb",
        package_name=package.LEGACY_PACKAGE_NAME,
    )
else:
    version = package.run(
        ["rpm", "-qp", "--qf", "%{VERSION}", str(a.package)], capture=True
    )
    with subprocess.Popen(["rpm2cpio", str(a.package)], stdout=subprocess.PIPE) as proc:
        subprocess.run(
            ["cpio", "-idm", "--quiet"], stdin=proc.stdout, cwd=stage, check=True
        )
        proc.stdout.close()
        assert proc.wait() == 0
    built = package.build_rpm(
        stage,
        a.out,
        a.version or version,
        a.arch,
        "legacy-go.rpm",
        package_name=package.LEGACY_PACKAGE_NAME,
    )
(a.out / "fixture.json").write_text(
    json.dumps(
        {
            "scope": __doc__,
            "source": package.provenance(),
            "version": a.version or version,
            "inputSha256": package.sha256(a.package),
            "fixtureSha256": package.sha256(built),
            "guiSha256": package.sha256(stage / "usr/bin/uniclipboard"),
            "daemonSha256": package.sha256(stage / "usr/bin/uniclipd"),
        },
        indent=2,
    )
    + "\n"
)
