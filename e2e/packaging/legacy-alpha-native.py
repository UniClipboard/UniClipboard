"""Verify and replace an installed v1 archive with an exact signed alpha archive."""

import argparse
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "e2e/release"))
from migration_acceptance import frozen_source  # noqa: E402

sys.path.insert(0, str(ROOT / "tools/release"))
from legacy_desktop_alpha import OLD_SOURCE, REPOSITORY, TARGETS, digest  # noqa: E402

OLD_ASSETS = {
    "darwin-aarch64": "UniClipboard_aarch64-apple-darwin.app.tar.gz",
    "darwin-x86_64": "UniClipboard_x86_64-apple-darwin.app.tar.gz",
    "linux-aarch64": "UniClipboard_1.6.2-alpha.2_aarch64.AppImage.tar.gz",
    "linux-x86_64": "UniClipboard_1.6.2-alpha.2_amd64.AppImage.tar.gz",
    "windows-aarch64": "UniClipboard_1.6.2-alpha.2_arm64-setup.exe",
    "windows-x86_64": "UniClipboard_1.6.2-alpha.2_x64-setup.exe",
}


def run(*command, **kwargs):
    return subprocess.run(
        [str(arg) for arg in command], check=True, timeout=300, **kwargs
    )


def download(tag, destination, name):
    run(
        "gh",
        "release",
        "download",
        tag,
        "--repo",
        REPOSITORY,
        "--dir",
        destination,
        "--pattern",
        name,
    )


def native_acceptance(args, report):
    source = args.legacy_source.resolve()
    commit = subprocess.check_output(
        ["git", "-C", source, "rev-parse", "HEAD"], text=True
    ).strip()
    if commit != OLD_SOURCE:
        raise ValueError("unexpected legacy verifier source")
    module = source / "apps/gui-go"
    offer = json.loads((args.offer / "desktop-legacy-alpha.json").read_text())
    catalogue = json.loads((args.offer / "release-manifest.json").read_text())
    version, build = offer["version"], offer["buildNumber"]
    filename = f"uniclipboard-{version}-build.{build}-{TARGETS[args.target]}"
    record = next(item for item in catalogue["artifacts"] if item["file"] == filename)
    with tempfile.TemporaryDirectory(prefix="unic-legacy-alpha-") as temporary:
        work = Path(temporary).resolve()
        assets = work / "assets"
        assets.mkdir()
        old_name = OLD_ASSETS[args.target]
        download("v1.6.2-alpha.2", assets, old_name)
        download("v1.6.2-alpha.2", assets, old_name + ".sig")
        download(f"desktop/v{version}-build.{build}", work, filename)
        archive = work / filename
        if digest(archive) != record["sha256"]:
            raise ValueError("candidate archive differs from the signed catalogue")
        candidate = assets / ("UniClipboard" + filename.removeprefix("uniclipboard"))
        archive.rename(candidate)
        (assets / (candidate.name + ".sig")).write_text(
            offer["signatures"][args.target]
        )
        run(
            "go",
            "run",
            "./cmd/updater-sign",
            "--verify-only",
            "--artifacts-dir",
            assets,
            "--evidence",
            args.output / "signatures.json",
            cwd=module,
        )
        report["candidate_sha256"] = digest(candidate)
        report["v1_asset_sha256"] = digest(assets / old_name)
        report["manifest_digest"] = offer["manifestDigest"]
        report["checks"].append("v1_key_verifies_both_archives")

        legacy_profile = work / "legacy"
        fixture, database = frozen_source(ROOT, legacy_profile)
        with sqlite3.connect(database) as data:
            data.execute("DELETE FROM clipboard_entry WHERE entry_id = 'entry-bad'")

        if args.target.startswith("windows"):
            stage = Path(os.environ["RUNNER_TEMP"]) / ("unic-legacy-v1-" + work.name)
            run(
                "pwsh",
                "-NoProfile",
                "-File",
                ROOT / "e2e/packaging/windows-v1-replacement.ps1",
                "-V1Installer",
                assets / old_name,
                "-V1Sha256",
                report["v1_asset_sha256"],
                "-Installer",
                candidate,
                "-Fixture",
                legacy_profile,
                "-FixtureMetadata",
                ROOT / "internal/v1migration/testdata/v3.json",
                "-Root",
                stage,
                "-Receipt",
                args.output / "windows",
                "-Target",
                "windows-arm64" if args.target.endswith("aarch64") else "windows-amd64",
                "-LegacyUpdater",
            )
            result = json.loads(
                (args.output / "windows/report.json").read_text(encoding="utf-8-sig")
            )
            current = result["v2_identity"]
            if result["status"] != "passed" or (
                current["version"],
                current["build_number"],
            ) != (version, build):
                raise ValueError("unexpected Windows installation receipt")
            report["identity"] = current
            report["checks"].extend(result["assertions"])
            return

        probe_source = module / "e2e" / "legacy-alpha-install.go"
        shutil.copyfile(ROOT / "e2e/packaging/legacy-alpha-install.go", probe_source)
        try:
            probe = work / "install-probe"
            run("go", "build", "-o", probe, probe_source, cwd=module)
        finally:
            probe_source.unlink()

        def snapshot():
            return {
                p.relative_to(legacy_profile).as_posix(): digest(p)
                for p in legacy_profile.rglob("*")
                if p.is_file()
            }

        before = snapshot()
        home = work / "home"
        home.mkdir()
        env = dict(
            os.environ,
            HOME=str(home),
            XDG_DATA_HOME=str(home / "data"),
            XDG_CONFIG_HOME=str(home / "config"),
            UNIC_HOME=str(work / "profile"),
        )
        if args.target.startswith("darwin"):
            run("/usr/bin/tar", "-xzf", assets / old_name, "-C", work)
            bundle = work / "UniClipboard.app"
            entry = bundle / "Contents/MacOS/gui-go"
            if not entry.is_file() or entry.is_symlink():
                raise ValueError("unexpected old macOS installation")
            run(probe, candidate, bundle)
            run("codesign", "--verify", "--deep", "--strict", bundle)
            run("xcrun", "stapler", "validate", bundle)
            invocation = [entry, "--resident-restart"]
            cli = bundle / "Contents/MacOS/unic"
        else:
            image = work / "UniClipboard.AppImage"
            with tarfile.open(assets / old_name, "r:gz") as tar:
                entry = next(
                    m for m in tar if m.isfile() and m.name.endswith(".AppImage")
                )
                with tar.extractfile(entry) as data, image.open("wb") as file:
                    shutil.copyfileobj(data, file)
            image.chmod(0o755)
            previous = digest(image)
            run(probe, candidate, image)
            if digest(image) == previous:
                raise ValueError("v1 AppImage was not replaced")
            env["APPIMAGE_EXTRACT_AND_RUN"] = "1"
            invocation = [image]
            run(image, "--appimage-extract", cwd=work, env=env, capture_output=True)
            cli = work / "squashfs-root/usr/lib/uniclipboard/unic"
        current = json.loads(
            run(
                *invocation,
                "--version",
                "--json",
                env=env,
                text=True,
                capture_output=True,
            ).stdout
        )
        if (current["version"], current["build_number"]) != (version, build):
            raise ValueError("unexpected installed Desktop identity")
        if snapshot() != before:
            raise ValueError("program replacement changed legacy data")
        report["identity"] = current
        report["checks"].append("old_install_path_executes_v2")
        migrated = run(
            cli,
            "migrate",
            "--source",
            legacy_profile,
            "--target",
            work / "profile",
            "--passphrase-stdin",
            "--json",
            env=env,
            text=True,
            capture_output=True,
            input=fixture["passphrase"] + "\n",
        )
        result = json.loads(migrated.stdout.splitlines()[-1])
        if (result["status"], result["imported"], result["verified"]) != (
            "complete",
            1,
            1,
        ):
            raise ValueError("history was not imported and reopened")
        if snapshot() != before:
            raise ValueError("explicit import changed legacy data")
        report["checks"].append("explicit_migration_preserves_v1_data")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=sorted(OLD_ASSETS), required=True)
    parser.add_argument("--legacy-source", type=Path, required=True)
    parser.add_argument("--offer", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    report = {
        "status": "failed",
        "target": args.target,
        "checks": [],
        "scope": "actual v1 payload, v1 signature and installer, exact installed v2 process identity and migration; graphical interaction not run",
    }
    try:
        native_acceptance(args, report)
        report["status"] = "passed"
    except Exception as error:
        report["error_type"] = type(error).__name__
        raise
    finally:
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
        entries = sorted(args.output.rglob("*.json"))
        (args.output / "SHA256SUMS").write_text(
            "".join(
                f"{digest(path)}  {path.relative_to(args.output).as_posix()}\n"
                for path in entries
            )
        )


if __name__ == "__main__":
    main()
