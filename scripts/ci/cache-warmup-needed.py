#!/usr/bin/env python3
"""Avoid a full compiler warmup for local package version changes alone."""

import argparse
import importlib.util
import os
import subprocess
from pathlib import Path


def warmup_needed(before, after):
    if not before or set(before) == {"0"}:
        return True
    # Compare the dependency seed, never just a commit message or branch name.
    spec = importlib.util.spec_from_file_location("snapshot", Path(__file__).with_name("mbx-snapshot.py"))
    snapshot = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(snapshot)
    changed = subprocess.check_output(
        ["git", "diff", "--name-only", before, after], text=True
    ).splitlines()
    for name in changed:
        if name in {"Cargo.toml", "Cargo.lock"} or name.endswith("/Cargo.toml"):
            exists = subprocess.run(["git", "cat-file", "-e", f"{before}:{name}"], capture_output=True)
            if exists.returncode:
                return True
            exists = subprocess.run(["git", "cat-file", "-e", f"{after}:{name}"], capture_output=True)
            if exists.returncode:
                return True
            old = subprocess.check_output(["git", "show", f"{before}:{name}"])
            new = subprocess.check_output(["git", "show", f"{after}:{name}"])
            kind = "Cargo.lock" if name == "Cargo.lock" else "Cargo.toml"
            if snapshot.release_seed_input(kind, old) != snapshot.release_seed_input(kind, new):
                return True
        elif (name == "rust-toolchain.toml" or name.endswith(".rs")
              or name.startswith((".cargo/", "scripts/", ".github/actions/", ".github/docker/", "apps/gui-go/e2e/linux/"))):
            return True
        # Go/JS package versions and changelogs do not change the Rust dependency
        # baseline. Compiler inputs above remain conservative invalidators.
    return False


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("before")
    parser.add_argument("after")
    args = parser.parse_args()
    needed = warmup_needed(args.before, args.after)
    with open(os.environ["GITHUB_OUTPUT"], "a") as output:
        output.write(f"needed={str(needed).lower()}\n")
    print("Warmup required" if needed else "Dependency seed unchanged; skipping background warmup")
