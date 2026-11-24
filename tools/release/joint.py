#!/usr/bin/env python3
"""Prepare the Desktop metadata for an accepted joint CLI candidate."""

import argparse
import json
from pathlib import Path

from release import digest, identity, metadata, validate_manifest

PRODUCTS = Path("release/products.toml")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("--metadata", type=Path, default=PRODUCTS)
    parser.add_argument("--stable", action="store_true")
    parser.add_argument("--cli-manifest", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--github-output", type=Path, required=True)
    args = parser.parse_args()
    cli = metadata(args.metadata)
    desktop = metadata(args.metadata, "desktop")
    if cli["version"] != args.version or desktop["version"] != args.version:
        raise ValueError(
            "both configured product versions must match the joint release"
        )
    if args.stable and (desktop["cli_version"], desktop["cli_build_number"]) != (
        cli["version"],
        cli["build_number"],
    ):
        raise ValueError(
            "stable Desktop must lock the CLI build selected for npm latest"
        )

    if args.cli_manifest:
        manifest = validate_manifest(args.cli_manifest)
        if (
            not manifest["source_clean"]
            or identity(manifest) != identity(cli)
            or set(manifest["targets"]) != set(desktop["targets"])
        ):
            raise ValueError("CLI candidate does not match the joint release identity")
        lock = {
            "version": desktop["version"],
            "build_number": desktop["build_number"],
            "targets": desktop["targets"],
            "update_public_key": desktop["update_public_key"],
            "cli_version": manifest["version"],
            "cli_build_number": manifest["build_number"],
            "cli_manifest_sha256": digest(args.cli_manifest),
        }
        args.output.write_text(
            "schema = 1\n[products.desktop]\n"
            + "".join(f"{key} = {json.dumps(value)}\n" for key, value in lock.items())
        )

    with args.github_output.open("a") as output:
        print(f"desktop_build={desktop['build_number']}", file=output)
        if args.stable:
            print(f"cli_manifest_sha256={desktop['cli_manifest_sha256']}", file=output)


if __name__ == "__main__":
    main()
