#!/usr/bin/env python3
"""Construct both Windows formats from actual cross-compiled development payloads."""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
out = ROOT / ".cache/windows-package-e2e"
out.mkdir(parents=True, exist_ok=True)
report = {
    "status": "running",
    "scope": "actual Windows PE cross-compilation and NSIS/ZIP construction",
    "not_run": [
        "native Windows execution",
        "installer lifecycle",
        "production CLI lock",
        "signing",
    ],
    "commands": [],
    "checks": [],
}


def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()


def run(args, **kwargs):
    result = subprocess.run(
        [str(x) for x in args],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        **kwargs,
    )
    report["commands"].append(
        {
            "argv": [str(x) for x in args],
            "exit_code": result.returncode,
            "output": result.stdout[-3000:],
        }
    )
    if result.returncode:
        raise RuntimeError(result.stdout[-3000:])
    return result.stdout


try:
    source = run(["git", "rev-parse", "HEAD"]).strip()
    report["source_sha"] = source
    report["source_files"] = {
        str(p.relative_to(ROOT)): sha(p)
        for directory in ["tools/package", "packaging", "internal/localpaths"]
        for p in (ROOT / directory).rglob("*")
        if p.is_file() and "__pycache__" not in str(p)
    }
    report["nsis_version"] = run(["makensis", "-VERSION"]).strip()
    with tempfile.TemporaryDirectory(prefix="unic-windows-package-") as temporary:
        temp = Path(temporary)
        for arch in ["amd64", "arm64"]:
            stage = temp / arch
            payload = stage / "payload"
            payload.mkdir(parents=True)
            for name in ["unic", "unicd", "unic-desktop"]:
                app = "desktop" if name == "unic-desktop" else "service"
                run(
                    [
                        "go",
                        "build",
                        "-trimpath",
                        "-ldflags",
                        f"-X github.com/uniclipboard/unic/internal/releaseinfo.Version=2.0.0 -X github.com/uniclipboard/unic/internal/releaseinfo.BuildNumber=1 -X github.com/uniclipboard/unic/internal/releaseinfo.Source={source}"
                        + (" -H windowsgui" if name == "unic-desktop" else ""),
                        "-o",
                        payload / (name + ".exe"),
                        "./app/" + app + "/cmd/" + name,
                    ],
                    env=dict(os.environ, GOOS="windows", GOARCH=arch, CGO_ENABLED="0"),
                )
                assert (payload / (name + ".exe")).read_bytes()[:2] == b"MZ"
            (payload / "uniclip.cmd").write_bytes(
                b'@echo off\r\n"%~dp0unic.exe" %*\r\nexit /b %errorlevel%\r\n'
            )
            (payload / "installation.json").write_text(
                '{"update_owner":"package-manager"}\n'
            )
            (payload / "LICENSE").write_bytes((ROOT / "LICENSE").read_bytes())
            cli_hashes = {
                name: sha(payload / name) for name in ["unic.exe", "unicd.exe"]
            }
            files = {str(p.relative_to(stage)): sha(p) for p in payload.iterdir()}
            (stage / "manifest.json").write_text(
                json.dumps(
                    {
                        "schema": 1,
                        "product": "desktop",
                        "version": "2.0.0",
                        "build_number": 1,
                        "target": "windows-" + arch,
                        "source_sha": source,
                        "source_clean": False,
                        "development": True,
                        "files": files,
                        "links": {},
                        "payload": "payload",
                        "cli": {
                            "version": "2.0.0",
                            "build_number": 1,
                            "manifest_sha256": "0" * 64,
                            "binaries_sha256": cli_hashes,
                        },
                        "acceptance": {
                            "native_identity": False,
                            "package_lifecycle": False,
                            "signing": False,
                        },
                    }
                )
            )
            for kind in ["setup.exe", "portable.zip"]:
                directory = temp / (arch + "-" + kind)
                run(
                    [
                        sys.executable,
                        ROOT / "tools/package/desktop.py",
                        "package",
                        "--stage",
                        stage,
                        "--format",
                        kind,
                        "--output",
                        directory,
                    ]
                )
                manifest = json.loads((directory / "artifacts.json").read_text())
                assert all(
                    sha(directory / item["file"]) == item["sha256"]
                    for item in manifest["artifacts"]
                )
                report["checks"].append("constructed and hashed " + arch + "/" + kind)
    report["status"] = "passed"
except BaseException as error:
    report["status"] = "failed"
    report["error"] = str(error)
    raise
finally:
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (out / "SHA256SUMS").write_text(sha(out / "report.json") + "  report.json\n")
    print(out / "report.json")
