#!/usr/bin/env python3
"""Build one native Desktop test package with a freshly accepted, exact local CLI lock."""

import argparse
import json
import os
from pathlib import Path
import platform
import re
import shutil
import sys
import tempfile

sys.dont_write_bytecode = True
import desktop as package  # noqa: E402

ROOT = package.ROOT


def plan(target, kind):
    if target not in package.TARGETS:
        raise ValueError("unsupported test target")
    system = target.split("-")[0]
    kind = (
        {"darwin": "dmg", "windows": "setup.exe", "linux": "deb"}[system]
        if kind == "default"
        else kind
    )
    if kind not in package.FORMATS[system]:
        raise ValueError("format does not match target")
    matrix = json.loads(
        package.run(
            [
                sys.executable,
                ROOT / "tools/release/release.py",
                "matrix",
                "--product",
                "desktop",
            ]
        )
    )
    matches = [entry for entry in matrix["include"] if entry["target"] == target]
    if (
        len(matches) != 1
        or target not in package.cli.metadata(ROOT / "release/products.toml")["targets"]
    ):
        raise ValueError("test target is not declared for both products")
    return dict(matches[0], format=kind)


def native_target():
    return (
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


def checksums(output):
    (output / "SHA256SUMS").write_text(
        "".join(
            package.sha(p) + "  " + p.name + "\n"
            for p in sorted(output.iterdir())
            if p.name != "SHA256SUMS"
        )
    )


def verify(output):
    output = Path(output).resolve()
    entries = {}
    for line in (output / "SHA256SUMS").read_text().splitlines():
        match = re.fullmatch(r"([a-f0-9]{64})  ([A-Za-z0-9][A-Za-z0-9._-]*)", line)
        if not match or match[2] in entries or match[2] == "SHA256SUMS":
            raise ValueError("invalid bundle checksum inventory")
        entries[match[2]] = match[1]
    if set(p.name for p in output.iterdir()) != set(entries) | {"SHA256SUMS"}:
        raise ValueError("bundle inventory mismatch")
    for name, digest in entries.items():
        p = output / name
        if p.is_symlink() or not p.is_file() or package.sha(p) != digest:
            raise ValueError("bundle digest mismatch: " + name)
    receipt = package.cli.read_json(output / "test-package.json")
    stage = package.cli.read_json(output / "stage-manifest.json")
    records = package.cli.read_json(output / "artifacts.json")
    artifact = receipt["artifact"]
    if set(entries) != {
        "test-package.json",
        "stage-manifest.json",
        "cli-manifest.json",
        "artifacts.json",
        "TESTING.md",
        artifact["file"],
    }:
        raise ValueError("bundle inventory mismatch")
    if (
        receipt["status"] != "passed"
        or receipt["purpose"] != "device-testing"
        or receipt["publication"] != "not_requested"
        or stage["development"] is not True
        or stage["update_public_key"]
        or stage["target"] != receipt["target"]
        or stage["source_sha"] != receipt["source_sha"]
        or receipt["cli_manifest_sha256"] != package.sha(output / "cli-manifest.json")
        or stage["cli"]["manifest_sha256"] != receipt["cli_manifest_sha256"]
        or records["artifacts"] != [artifact]
        or artifact["signed"]
        or package.sha(output / artifact["file"]) != artifact["sha256"]
        or (output / artifact["file"]).stat().st_size != artifact["size"]
    ):
        raise ValueError("inconsistent test package receipt")
    return receipt


def build(args):
    selected = plan(args.target, args.format)
    if selected["target"] != native_target():
        raise ValueError("test construction requires matching native host")
    source = package.run(["git", "rev-parse", "HEAD"])
    if args.source_sha and source != args.source_sha:
        raise ValueError("checkout differs from requested source SHA")
    output = package.fresh_output(args.output)
    output.mkdir()
    original_metadata = package.sha(ROOT / "release/products.toml")
    original_sources = package.sources()
    # Test construction never consumes production signing or ingestion credentials.
    env = dict(os.environ)
    for name in [
        "UNIC_UPDATE_PRIVATE_KEY",
        "UNIC_MACOS_SIGNING_IDENTITY",
        "UNIC_WINDOWS_CERT_SHA1",
        "UNIC_POSTHOG_HOST",
        "UNIC_POSTHOG_PROJECT_TOKEN",
        "UNIC_PRODUCT_VERSION",
    ]:
        env.pop(name, None)

    def run(*argv, overrides=None):
        return package.run(list(argv), env=dict(env, **(overrides or {})))

    receipt = {
        "schema": 1,
        "status": "running",
        "purpose": "device-testing",
        "publication": "not_requested",
        "target": args.target,
        "format": selected["format"],
        "source_sha": source,
        "source_clean": not bool(package.run(["git", "status", "--porcelain"])),
        "workflow_run_id": os.environ.get("GITHUB_RUN_ID"),
        "workflow_run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT"),
        "gui": "not_run",
        "installation": "not_run",
    }
    try:
        with tempfile.TemporaryDirectory(
            prefix=".desktop-test-", dir=output.parent
        ) as temporary:
            work = Path(temporary)
            cli_record = package.cli.metadata(ROOT / "release/products.toml")
            desktop_record = package.cli.metadata(
                ROOT / "release/products.toml", "desktop"
            )
            cli_text = f'schema=1\n[products.cli]\nversion="{cli_record["version"]}"\nbuild_number={cli_record["build_number"]}\ntargets=["{args.target}"]\n'
            cli_metadata = work / "cli-products.toml"
            cli_metadata.write_text(cli_text)
            cli_native = work / "cli-native"
            run(
                "go",
                "run",
                "./tools/package",
                "-product",
                "cli",
                "-metadata",
                cli_metadata,
                "-output",
                cli_native,
                overrides={
                    "GOOS": selected["goos"],
                    "GOARCH": selected["goarch"],
                    "CGO_ENABLED": "0",
                },
            )
            run(
                sys.executable,
                ROOT / "tools/release/release.py",
                "smoke",
                "--directory",
                cli_native,
            )
            cli_packages = work / "cli-packages"
            run(
                sys.executable,
                ROOT / "tools/release/release.py",
                "assemble",
                "--metadata",
                cli_metadata,
                "--input",
                cli_native,
                "--output",
                cli_packages,
            )
            cli_manifest = cli_packages / "release-manifest.json"
            lock = package.sha(cli_manifest)
            metadata = work / "desktop-products.toml"
            metadata.write_text(
                cli_text
                + f'\n[products.desktop]\nversion="{desktop_record["version"]}"\nbuild_number={desktop_record["build_number"]}\ntargets=["{args.target}"]\ncli_version="{cli_record["version"]}"\ncli_build_number={cli_record["build_number"]}\ncli_manifest_sha256="{lock}"\nupdate_public_key=""\n'
            )
            stage = work / "stage"
            run(
                sys.executable,
                ROOT / "tools/package/desktop.py",
                "stage",
                "--target",
                args.target,
                "--metadata",
                metadata,
                "--cli-manifest",
                cli_manifest,
                "--output",
                stage,
                "--development",
            )
            run(
                sys.executable,
                ROOT / "tools/package/desktop.py",
                "smoke",
                "--stage",
                stage,
            )
            command = [
                sys.executable,
                ROOT / "tools/package/desktop.py",
                "package",
                "--stage",
                stage,
                "--format",
                selected["format"],
                "--output",
                work / "package",
            ]
            if selected["format"] in {"deb", "rpm", "archlinux"}:
                tool = work / "nfpm"
                run(
                    sys.executable,
                    ROOT / "tools/package/install-tools.py",
                    "--output",
                    tool,
                )
                command += ["--nfpm", tool]
            if selected["format"] in {"AppImage", "AppImage.tar.gz"}:
                for tool in ["appimagetool", "appimage-runtime"]:
                    path = work / tool
                    run(
                        sys.executable,
                        ROOT / "tools/package/install-tools.py",
                        "--tool",
                        tool,
                        "--output",
                        path,
                    )
                    command += ["--" + tool, path]
            run(*command)
            records = package.cli.read_json(work / "package/artifacts.json")
            if len(records["artifacts"]) != 1:
                raise ValueError("test build produced multiple packages")
            artifact = records["artifacts"][0]
            original = work / "package" / artifact["file"]
            artifact["file"] = "test-" + source[:12] + "-" + artifact["file"]
            shutil.copyfile(original, output / artifact["file"])
            package.save(output / "artifacts.json", records)
            shutil.copyfile(cli_manifest, output / "cli-manifest.json")
            shutil.copyfile(stage / "manifest.json", output / "stage-manifest.json")
            (output / "TESTING.md").write_text(f"""# Desktop device-test package

Target: `{args.target}`. Source: `{source}`. Format: `{selected["format"]}`.

This is an unsigned test package, not a published release. Native version and private CLI/service IPC checks passed. GUI and installation are not accepted by these checks. Automatic updates are disabled. macOS packages have no Developer ID/notarization; Windows packages have no production Authenticode signature.

Verify every file with `shasum -a 256 -c SHA256SUMS`, or `python tools/package/desktop_test.py verify --directory <download-directory>`. On Windows, use `Get-FileHash -LiteralPath '{artifact["file"]}' -Algorithm SHA256` and compare the package digest with the Actions run summary.

The application retains the existing UniClipboard installation identity. A test installer can replace an existing installation. Use a dedicated test machine/account for installers. A custom path does not isolate a registered Windows installation. Unpacked archives may use a separate folder. Set `UNIC_HOME` to a dedicated test directory before launching the application. Import only synthetic data. Production CLI release locks and source metadata were not changed.
""")
            if (
                original_metadata != package.sha(ROOT / "release/products.toml")
                or original_sources != package.sources()
                or source != package.run(["git", "rev-parse", "HEAD"])
            ):
                raise ValueError(
                    "source or checked-in metadata changed during test construction"
                )
            receipt.update(status="passed", cli_manifest_sha256=lock, artifact=artifact)
        package.save(output / "test-package.json", receipt)
        checksums(output)
        verify(output)
    except BaseException as error:
        receipt.update(status="failed", error_type=type(error).__name__)
        package.save(output / "test-package.json", receipt)
        checksums(output)
        raise
    print(output / "test-package.json")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for name in ["plan", "build"]:
        p = sub.add_parser(name)
        p.add_argument("--target", required=True)
        p.add_argument("--format", default="default")
        if name == "build":
            p.add_argument("--output", default=str(ROOT / "dist/test-package"))
            p.add_argument("--source-sha", default="")
    p = sub.add_parser("verify")
    p.add_argument("--directory", required=True)
    args = parser.parse_args()
    if args.command == "plan":
        print(json.dumps(plan(args.target, args.format)))
    elif args.command == "build":
        build(args)
    else:
        print(json.dumps(verify(args.directory)))


if __name__ == "__main__":
    main()
