#!/usr/bin/env python3
"""Exercise the actual single-target Desktop test-package controller and native archive."""

import hashlib
import json
from pathlib import Path
import platform
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / "tools/package/desktop_test.py"
OUT = ROOT / ".cache/desktop-test-package-e2e"
OUT.mkdir(parents=True, exist_ok=True)
report = {
    "status": "running",
    "scope": "real native CLI/Desktop/archive processes; no GUI, installation or public publication",
    "checks": [],
}


def run(*args, error=None):
    result = subprocess.run(
        [sys.executable, str(TOOL), *map(str, args)],
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=600,
    )
    if error is None:
        if result.returncode:
            raise RuntimeError(result.stderr[-4000:])
    elif result.returncode == 0 or error not in result.stderr:
        raise RuntimeError(f"Expected {error}: {result.stderr[-4000:]}")
    return result.stdout


def check(name, value):
    assert value, name
    report["checks"].append(name)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


try:
    report["source_sha"] = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    for target, runner, kind in [
        ("darwin-arm64", "macos-15", "dmg"),
        ("darwin-amd64", "macos-15-intel", "dmg"),
        ("windows-arm64", "windows-11-arm", "setup.exe"),
        ("windows-amd64", "windows-2025", "setup.exe"),
        ("linux-arm64", "ubuntu-24.04-arm", "deb"),
        ("linux-amd64", "ubuntu-24.04", "deb"),
    ]:
        selected = json.loads(run("plan", "--target", target))
        check(
            target + " selects exactly its native runner/default format",
            selected["target"] == target
            and selected["runner"] == runner
            and selected["format"] == kind,
        )
    run("plan", "--target", "windows-x86", error="unsupported test target")
    run(
        "plan",
        "--target",
        "windows-arm64",
        "--format",
        "dmg",
        error="format does not match target",
    )
    run(
        "plan",
        "--target",
        "linux-amd64",
        "--format",
        "../../setup.exe",
        error="format does not match target",
    )
    report["checks"].append(
        "invalid target, mismatched format and unsafe format rejected"
    )
    target = (
        {"Darwin": "darwin", "Linux": "linux", "Windows": "windows"}[platform.system()]
        + "-"
        + {
            "arm64": "arm64",
            "aarch64": "arm64",
            "ARM64": "arm64",
            "x86_64": "amd64",
            "AMD64": "amd64",
        }[platform.machine()]
    )
    # The canonical native process runner already covers Windows ZIP construction.
    # This case inspects the native tar container on macOS/Linux.
    if target.startswith("windows-"):
        raise RuntimeError("run this archive-content case on macOS or Linux")
    kind = "app.tar.gz" if target.startswith("darwin-") else "update.tar.gz"
    original_metadata = sha(ROOT / "release/products.toml")
    with tempfile.TemporaryDirectory(prefix="unic-test-package-e2e-") as work:
        temp = Path(work)
        wrong = "windows-arm64" if not target.startswith("windows-") else "darwin-arm64"
        run(
            "build",
            "--target",
            wrong,
            "--output",
            temp / "wrong",
            error="requires matching native host",
        )
        check("wrong-host build rejected before output", not (temp / "wrong").exists())
        output = temp / "bundle"
        run("build", "--target", target, "--format", kind, "--output", output)
        receipt = json.loads((output / "test-package.json").read_text())
        cli = json.loads((output / "cli-manifest.json").read_text())
        stage = json.loads((output / "stage-manifest.json").read_text())
        check(
            "one target/format built from exact source",
            receipt["target"] == target
            and receipt["format"] == kind
            and receipt["source_sha"] == report["source_sha"]
            and cli["targets"] == [target]
            and cli["source_sha"] == receipt["source_sha"],
        )
        check(
            "development provenance and disabled updater",
            receipt["purpose"] == "device-testing"
            and stage["development"] is True
            and stage["update_public_key"] == ""
            and receipt["publication"] == "not_requested",
        )
        check(
            "native identity and shared API accepted",
            stage["acceptance"]["native_identity"]
            and stage["acceptance"]["shared_configuration_api"],
        )
        check(
            "exact local CLI manifest locked",
            stage["cli"]["manifest_sha256"]
            == sha(output / "cli-manifest.json")
            == receipt["cli_manifest_sha256"],
        )
        check(
            "checked-in release metadata untouched",
            sha(ROOT / "release/products.toml") == original_metadata,
        )
        records = json.loads((output / "artifacts.json").read_text())
        check(
            "one explicitly unsigned test package",
            len(records["artifacts"]) == 1
            and records["artifacts"][0]["file"].startswith("test-")
            and records["artifacts"][0]["signed"] is False,
        )
        artifact = output / records["artifacts"][0]["file"]
        with tarfile.open(artifact) as archive:
            for name, expected in stage["cli"]["binaries_sha256"].items():
                matches = [
                    p
                    for p in archive.getmembers()
                    if p.name.endswith("/" + name) or p.name == name
                ]
                check(
                    name + " archive bytes match immutable CLI lock",
                    len(matches) == 1
                    and matches[0].isfile()
                    and hashlib.sha256(
                        archive.extractfile(matches[0]).read()
                    ).hexdigest()
                    == expected,
                )
        run("verify", "--directory", output)
        report["checks"].append("complete downloadable checksum inventory verified")
        run(
            "build",
            "--target",
            target,
            "--format",
            kind,
            "--output",
            output,
            error="output already exists",
        )
        report["checks"].append("existing output refused")
        original = artifact.read_bytes()
        artifact.write_bytes(b"damaged")
        run("verify", "--directory", output, error="bundle digest mismatch")
        artifact.write_bytes(original)
        (output / "unexpected.txt").write_text("extra")
        run("verify", "--directory", output, error="bundle inventory mismatch")
        (output / "unexpected.txt").unlink()
        run("verify", "--directory", output)
        report["checks"].append("tampered and extra downloadable files rejected")
        report["artifact"] = receipt["artifact"]
        check(
            "temporary build workspace removed",
            sorted(p.name for p in temp.iterdir()) == ["bundle"],
        )
    report["status"] = "passed"
except BaseException as error:
    report["status"] = "failed"
    report["error"] = str(error)
    raise
finally:
    report["source_files"] = {
        str(p.relative_to(ROOT)): sha(p) for p in [Path(__file__), TOOL] if p.exists()
    }
    (OUT / "report.json").write_text(
        json.dumps(report, indent=2, sort_keys=True) + "\n"
    )
    (OUT / "SHA256SUMS").write_text(sha(OUT / "report.json") + "  report.json\n")
print(json.dumps(report, indent=2))
