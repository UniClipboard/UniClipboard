#!/usr/bin/env python3
"""Acquire a complete CLI candidate from its successful canonical build workflow."""

import argparse
import base64
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import release as cli  # noqa: E402


def acquire(
    run_id, output, metadata, repository="UniClipboard/UniClipboard", signed=False
):
    if not str(run_id).isdigit() or int(run_id) < 1:
        raise ValueError("invalid CLI build run ID")

    def gh(*args):
        return subprocess.check_output(["gh", *map(str, args)], text=True).strip()

    run = json.loads(gh("api", f"repos/{repository}/actions/runs/{run_id}"))
    workflow = json.loads(
        gh("api", f"repos/{repository}/actions/workflows/build-cli.yml")
    )
    if (
        run.get("conclusion") != "success"
        or run.get("workflow_id") != workflow["id"]
        or run.get("repository", {}).get("full_name") != repository
    ):
        raise ValueError("CLI run is not a successful canonical repository build")
    output = Path(output).resolve()
    if output.exists():
        raise ValueError("candidate output already exists")
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=".cli-candidate-", dir=output.parent
    ) as temporary:
        work = Path(temporary)
        gh(
            "run",
            "download",
            run_id,
            "--repo",
            repository,
            "--name",
            "cli-npm-candidate",
            "--dir",
            work,
        )
        manifest = cli.validate_manifest(work / "release-manifest.json")
        record = cli.metadata(metadata, "cli")
        if (
            manifest["version"],
            manifest["build_number"],
            set(manifest["targets"]),
        ) != (record["version"], record["build_number"], set(record["targets"])):
            raise ValueError(
                "CLI candidate does not match the selected complete product matrix"
            )
        if (
            manifest.get("source_clean") is not True
            or manifest["source_sha"] != run["head_sha"]
        ):
            raise ValueError(
                "CLI candidate source disagrees with the accepted workflow run"
            )
        for target in record["targets"]:
            native = manifest["target_manifests"][target]
            if not all(
                native.get("acceptance", {}).get(k) is True
                for k in ["native", "version_smoke", "alias_smoke"]
            ):
                raise ValueError("CLI target lacks native acceptance: " + target)
            if (
                signed
                and target.startswith("darwin-")
                and native.get("signed") is not True
            ):
                raise ValueError(
                    "Desktop requires a distribution-signed macOS CLI candidate"
                )
        for package in manifest["packages"]:
            path = cli.checked_file(work, package["file"], package["sha256"])
            integrity = (
                "sha512-"
                + base64.b64encode(hashlib.sha512(path.read_bytes()).digest()).decode()
            )
            if integrity != package["integrity"]:
                raise ValueError("CLI npm package integrity mismatch")
        work.rename(output)
    return manifest


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--run-id", required=True)
    p.add_argument("--output", required=True)
    p.add_argument("--metadata", default=str(cli.ROOT / "release/products.toml"))
    p.add_argument("--require-macos-signing", action="store_true")
    p.add_argument("--lock-desktop", action="store_true")
    args = p.parse_args()
    m = acquire(
        args.run_id, args.output, args.metadata, signed=args.require_macos_signing
    )
    if args.lock_desktop:
        path = Path(args.metadata)
        data = path.read_text()
        digest = cli.digest(Path(args.output) / "release-manifest.json")
        prefix, section = data.split("[products.desktop]", 1)
        body, rest = section.split("[products.android]", 1)
        for name, value in [
            ("cli_version", json.dumps(m["version"])),
            ("cli_build_number", str(m["build_number"])),
            ("cli_manifest_sha256", json.dumps(digest)),
        ]:
            body, count = re.subn(
                r"(?m)^" + name + r"\s*=.*$", name + " = " + value, body
            )
            if count != 1:
                raise ValueError("missing or duplicate Desktop lock field: " + name)
        if "cli_artifact_run_id" in body:
            body = re.sub(
                r"(?m)^cli_artifact_run_id\s*=.*$",
                "cli_artifact_run_id = " + str(int(args.run_id)),
                body,
            )
        else:
            body += "cli_artifact_run_id = " + str(int(args.run_id)) + "\n\n"
        path.write_text(
            prefix + "[products.desktop]" + body + "[products.android]" + rest
        )
    print(Path(args.output) / "release-manifest.json")


if __name__ == "__main__":
    main()
