#!/usr/bin/env python3
"""Real renderer processes with disposable artifact fixtures; no native acceptance."""

import argparse
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--output", default=str(ROOT / ".cache/channel-e2e"))
a = p.parse_args()
out = Path(a.output).resolve()
out.mkdir(parents=True, exist_ok=True)
report = {
    "status": "running",
    "scope": "renderer process orchestration with synthetic package bytes",
    "not_run": [
        "native package validity",
        "package-manager installation",
        "public destinations",
    ],
    "checks": [],
    "commands": [],
}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(args, ok=True):
    r = subprocess.run([str(x) for x in args], text=True, cwd=ROOT, capture_output=True)
    report["commands"].append(
        {
            "argv": [str(x) for x in args],
            "exit_code": r.returncode,
            "output": (r.stdout + r.stderr)[-3000:],
        }
    )
    if (r.returncode == 0) != ok:
        raise RuntimeError(r.stderr + r.stdout)


try:
    with tempfile.TemporaryDirectory(prefix="unic-channel-e2e-") as temporary:
        temp = Path(temporary)
        inputs = temp / "inputs"
        inputs.mkdir()
        npm = temp / "npm"
        npm.mkdir()
        packages = []
        targets = []
        for arch in ["amd64", "arm64"]:
            target = "darwin-" + arch
            targets.append(target)
            file = npm / ("cli-" + target + ".tgz")
            file.write_bytes(("fixture " + target).encode())
            packages.append(
                {
                    "name": "@uniclipboard/cli-darwin-"
                    + ("x64" if arch == "amd64" else "arm64"),
                    "target": target,
                    "version": "2.0.0-build.1",
                    "file": file.name,
                    "sha256": sha(file),
                    "integrity": "sha512-"
                    + base64.b64encode(
                        hashlib.sha512(file.read_bytes()).digest()
                    ).decode(),
                }
            )
        packages.append({"name": "@uniclipboard/cli", "version": "2.0.0-build.1"})
        manifest = npm / "release-manifest.json"
        manifest.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "product": "cli",
                    "version": "2.0.0",
                    "build_number": 1,
                    "npm_version": "2.0.0-build.1",
                    "targets": targets,
                    "packages": packages,
                }
            )
        )
        lock = sha(manifest)
        formats = {
            "darwin": ["dmg"],
            "windows": ["setup.exe", "portable.zip"],
            "linux": ["deb", "rpm", "archlinux"],
        }
        for os_name, kinds in formats.items():
            for arch in ["amd64", "arm64"]:
                directory = inputs / (os_name + "-" + arch)
                directory.mkdir()
                artifacts = []
                for kind in kinds:
                    file = directory / ("fixture-" + os_name + "-" + arch + "." + kind)
                    file.write_bytes(("fixture artifact " + file.name).encode())
                    artifacts.append(
                        {"format": kind, "file": file.name, "sha256": sha(file)}
                    )
                (directory / "artifacts.json").write_text(
                    json.dumps(
                        {
                            "schema": 1,
                            "product": "desktop",
                            "version": "2.0.0",
                            "build_number": 1,
                            "source_sha": "a" * 40,
                            "target": os_name + "-" + arch,
                            "source_clean": False,
                            "development": True,
                            "cli": {"manifest_sha256": lock},
                            "artifacts": artifacts,
                        }
                    )
                )
        command = [
            sys.executable,
            ROOT / "tools/package/channels.py",
            "--input",
            inputs,
            "--base-url",
            "https://example.org/desktop/2.0.0/build-1",
            "--cli-manifest",
            manifest,
        ]
        output = temp / "rendered"
        run(command + ["--output", output, "--development"])
        rendered = json.loads((output / "channels.json").read_text())
        assert len(rendered["channels"]) == 12
        assert all(
            x["availability"] == "pending" for x in rendered["channels"].values()
        )
        assert all(
            sha(output / name) == value for name, value in rendered["files"].items()
        )
        report["checks"] += [
            "all twelve channel definitions rendered",
            "all publication states remain pending",
            "all definition hashes verified",
        ]
        run(command + ["--output", temp / "production"], ok=False)
        report["checks"].append(
            "development fixtures rejected for production rendering"
        )
        path = inputs / "linux-amd64/artifacts.json"
        item = json.loads(path.read_text())
        item["version"] = "2.0.1"
        path.write_text(json.dumps(item))
        run(command + ["--output", temp / "mixed", "--development"], ok=False)
        report["checks"].append("mixed product versions rejected")
        item["version"] = "2.0.0"
        path.write_text(json.dumps(item))
        artifact = inputs / "linux-amd64" / item["artifacts"][0]["file"]
        artifact.write_bytes(b"corrupt")
        run(command + ["--output", temp / "corrupt", "--development"], ok=False)
        report["checks"].append("changed artifact bytes rejected")
    report["status"] = "passed"
except BaseException as error:
    report["status"] = "failed"
    report["error"] = str(error)
    raise
finally:
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (out / "SHA256SUMS").write_text(sha(out / "report.json") + "  report.json\n")
    print(out / "report.json")
