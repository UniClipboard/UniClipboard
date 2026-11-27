#!/usr/bin/env python3
"""Drive production catalogue/page gates with synthetic files and real Ed25519 signatures."""

import copy
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/release"))
import desktop_release as release  # noqa: E402

out = ROOT / ".cache/signing-policy-e2e"
out.mkdir(parents=True, exist_ok=True)
report = {
    "status": "running",
    "scope": "production controller subprocesses; synthetic catalogue; temporary Ed25519 key; no remote writes or native signing proof",
    "source_sha": subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip(),
    "checks": [],
}


def run(argv, expected=None, env=None):
    result = subprocess.run(
        list(map(str, argv)),
        cwd=ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=180,
    )
    if expected is None:
        if result.returncode:
            raise RuntimeError(result.stderr[-2000:])
    elif result.returncode == 0 or expected not in result.stderr:
        raise RuntimeError(f"Expected rejection: {expected}\n{result.stderr[-2000:]}")
    return result.stdout


try:
    with tempfile.TemporaryDirectory(prefix="unic-signing-policy-") as work:
        temp = Path(work)
        signer = temp / ("sign.exe" if os.name == "nt" else "sign")
        run(["go", "build", "-o", signer, "./tools/release/update-sign"])
        run([signer, "-keygen", temp / "keys"])
        public = (temp / "keys/public.key").read_text().strip()
        env = dict(
            os.environ,
            UNIC_UPDATE_PRIVATE_KEY=(temp / "keys/private.key").read_text().strip(),
        )
        cli = {
            "schema": 1,
            "product": "cli",
            "version": "2.0.0",
            "build_number": 2,
            "npm_version": "2.0.0-build.2",
            "targets": release.TARGETS,
            "packages": [
                {
                    "name": release.package.cli.package_name(t),
                    "file": t + ".tgz",
                    "version": "2.0.0-build.2",
                }
                for t in release.TARGETS
            ]
            + [
                {
                    "name": "@uniclipboard/cli",
                    "file": "cli.tgz",
                    "version": "2.0.0-build.2",
                }
            ],
        }
        cli_path = temp / "cli.json"
        release.save(cli_path, cli)
        lock = release.package.sha(cli_path)
        metadata = temp / "products.toml"
        metadata.write_text(
            'schema=1\n[products.desktop]\nversion="2.0.0"\nbuild_number=1\n'
            f'cli_manifest_sha256="{lock}"\nupdate_public_key="{public}"\n'
            "targets=" + json.dumps(release.TARGETS) + "\n"
        )
        records = temp / "records"
        originals = {}
        for target in release.TARGETS:
            directory = records / target
            directory.mkdir(parents=True)
            artifacts = []
            for kind in sorted(release.FORMATS[target.split("-")[0]]):
                path = directory / (target + "." + kind)
                path.write_bytes(("synthetic policy fixture: " + path.name).encode())
                updater = kind == (
                    "app.tar.gz" if target.startswith("darwin-") else "update.tar.gz"
                )
                signature = (
                    json.loads(run([signer, "-file", path], env=env))["signature"]
                    if updater
                    else ""
                )
                artifacts.append(
                    {
                        "file": path.name,
                        "format": kind,
                        "sha256": release.package.sha(path),
                        "size": path.stat().st_size,
                        "signed": target.startswith("darwin-"),
                        "payload_signed": target.startswith("darwin-"),
                        "container_signed": False,
                        "notarized": target.startswith("darwin-"),
                        "signature": signature,
                    }
                )
            record = {
                "schema": 1,
                "product": "desktop",
                "version": "2.0.0",
                "build_number": 1,
                "source_sha": report["source_sha"],
                "source_clean": True,
                "development": False,
                "cli": {"manifest_sha256": lock},
                "target": target,
                "acceptance": {
                    "native_identity": True,
                    "shared_configuration_api": True,
                },
                "artifacts": artifacts,
            }
            originals[directory / "artifacts.json"] = record

        def reset():
            for path, record in originals.items():
                release.save(path, record)

        counter = 0

        def assemble(expected=None):
            global counter
            counter += 1
            destination = temp / f"catalogue-{counter}"
            run(
                [
                    sys.executable,
                    ROOT / "tools/release/desktop_release.py",
                    "assemble",
                    "--input",
                    records,
                    "--output",
                    destination,
                    "--metadata",
                    metadata,
                ],
                expected,
            )
            return destination

        reset()
        catalogue = assemble()
        manifest = json.loads((catalogue / "release-manifest.json").read_text())
        windows = [
            a for a in manifest["artifacts"] if a["target"].startswith("windows-")
        ]
        assert windows and all(
            a["signed"] is False and a["container_signed"] is False for a in windows
        )
        report["checks"].append(
            "unsigned Windows payload and setup accepted without signed claims"
        )
        notes = temp / "notes.json"
        release.save(
            notes, {"en": "Synthetic policy fixture.", "zh": "Synthetic fixture."}
        )
        source_root = temp / "source"
        changelog_dir = source_root / "release/changelogs/desktop"
        changelog_dir.mkdir(parents=True)
        (changelog_dir / "v2.0.0-build.1.md").write_text(
            "## v2.0.0-build.1 (desktop) - 2026-11-27\n\n### Features\n\n- Synthetic policy fixture item.\n",
            encoding="utf-8",
        )
        (changelog_dir / "v2.0.0-build.1.zh.md").write_text(
            "## v2.0.0-build.1 (desktop) - 2026-11-27\n\n### Features\n\n- 合成策略测试条目。\n",
            encoding="utf-8",
        )
        for channel in ["alpha", "stable"]:
            page = temp / ("page-" + channel)
            run(
                [
                    sys.executable,
                    ROOT / "tools/release/github_desktop.py",
                    "preview",
                    "--manifest",
                    catalogue / "release-manifest.json",
                    "--cli-manifest",
                    cli_path,
                    "--notes",
                    notes,
                    "--source-root",
                    source_root,
                    "--channel",
                    channel,
                    "--output",
                    page,
                ]
            )
            page_text = (page / "release-notes.md").read_text(encoding="utf-8")
            assert "Windows packages are unsigned" in page_text
            assert "Synthetic policy fixture item." in page_text
            assert "<summary>中文变更日志</summary>" in page_text
            report["checks"].append(channel + " page declares unsigned Windows")

        cases = [
            (
                "macOS signature",
                "darwin-arm64",
                "signed",
                False,
                "missing production code signature",
            ),
            (
                "macOS notarization",
                "darwin-arm64",
                "notarized",
                False,
                "missing Apple notarization",
            ),
            (
                "missing updater signature",
                "windows-arm64",
                "signature",
                "",
                "missing native updater key/signature",
            ),
            (
                "damaged updater signature",
                "windows-arm64",
                "signature",
                "AA==",
                "archive signature mismatch",
            ),
        ]
        for label, target, field, value, error in cases:
            reset()
            path = records / target / "artifacts.json"
            record = copy.deepcopy(originals[path])
            for artifact in record["artifacts"]:
                if field != "signature" or artifact["format"] == "update.tar.gz":
                    artifact[field] = value
            release.save(path, record)
            assemble(error)
            report["checks"].append(label + " rejected")
        reset()
        text = metadata.read_text()
        metadata.write_text(text.replace(public, ""))
        assemble("missing native updater key/signature")
        metadata.write_text(text)
        report["checks"].append("missing updater key rejected")
        for label, field, value, error in [
            (
                "CLI mismatch",
                "cli",
                {"manifest_sha256": "b" * 64},
                "mixed Desktop or CLI identity",
            ),
            (
                "source mismatch",
                "source_sha",
                "b" * 40,
                "mixed Desktop or CLI identity",
            ),
        ]:
            reset()
            path = records / "windows-arm64/artifacts.json"
            record = copy.deepcopy(originals[path])
            record[field] = value
            release.save(path, record)
            assemble(error)
            report["checks"].append(label + " rejected")
        reset()
        artifact = records / "windows-arm64/windows-arm64.setup.exe"
        artifact.write_bytes(b"changed")
        assemble("asset digest mismatch")
        report["checks"].append("changed artifact rejected")
    report["status"] = "passed"
except BaseException as error:
    report["status"] = "failed"
    report["error"] = str(error)
    raise
finally:
    report["source_files"] = {
        str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in [
            Path(__file__),
            ROOT / "tools/release/desktop_release.py",
            ROOT / "tools/release/github_desktop.py",
            ROOT / "tools/package/desktop.py",
        ]
    }
    release.save(out / "report.json", report)
    (out / "SHA256SUMS").write_text(
        hashlib.sha256((out / "report.json").read_bytes()).hexdigest()
        + "  report.json\n"
    )
print(json.dumps(report, indent=2))
