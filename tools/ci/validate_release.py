#!/usr/bin/env python3
"""Validate release metadata, release notes, and release PR conventions."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/release"))

import mobile as mobile_release  # noqa: E402
import prepare_release  # noqa: E402
import release as cli_release  # noqa: E402


def validate_products_toml(path: Path) -> None:
    cli = cli_release.metadata(path, "cli")
    if not cli["targets"]:
        raise ValueError("cli targets must not be empty")
    for target in cli["targets"]:
        cli_release.package_name(target)

    desktop = cli_release.metadata(path, "desktop")
    if not desktop["targets"]:
        raise ValueError("desktop targets must not be empty")
    if not desktop.get("cli_version"):
        raise ValueError("desktop.cli_version is required")
    if type(desktop.get("cli_build_number")) is not int or desktop["cli_build_number"] <= 0:
        raise ValueError("desktop.cli_build_number must be a positive integer")
    cli_sha = desktop.get("cli_manifest_sha256", "")
    if len(cli_sha) != 64 or any(c not in "0123456789abcdef" for c in cli_sha):
        raise ValueError("desktop.cli_manifest_sha256 must be a 64-char lowercase hex digest")

    mobile_release.mobile_metadata("android", path)
    mobile_release.mobile_metadata("ios", path)


def validate_notes_json(path: Path) -> None:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError(f"{path} must be a JSON object")
    for lang in ("en", "zh"):
        value = data.get(lang)
        if not isinstance(value, str) or not value.strip():
            raise ValueError(f"{path} must contain a non-empty string for '{lang}'")
        for marker in prepare_release.PLACEHOLDER_MARKERS:
            if marker in value:
                raise ValueError(f"{path} '{lang}' contains forbidden marker {marker!r}")


def validate_repo(root: Path, head_ref: str = "") -> None:
    validate_products_toml(root / "release/products.toml")
    prepare_release.assert_finalized_desktop_notes(root / "release/desktop-notes.json")
    android_notes = root / "release/android-notes.json"
    if android_notes.exists():
        validate_notes_json(android_notes)

    if head_ref.startswith("release/"):
        subprocess.run(
            [
                sys.executable,
                str(ROOT / "tools/release/prepare_release.py"),
                "parse-branch",
                "--branch",
                head_ref,
                "--validate-repo",
                "--repo-root",
                str(root),
            ],
            check=True,
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT, help="Repository root")
    parser.add_argument("--head-ref", default="", help="PR head branch name (optional)")
    args = parser.parse_args()

    validate_repo(args.root, head_ref=args.head_ref)
    print("release metadata validation passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
