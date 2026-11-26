#!/usr/bin/env python3
"""Replay package preparation from a real joint release without publishing."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[2]
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT / "tools/release"))
from package_channel import INPUTS  # noqa: E402


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def exercise(channel, command, scratch):
    output = scratch / channel
    record = scratch / (channel + ".env")
    result = subprocess.run(
        [
            *command,
            "--channel",
            channel,
            "--output",
            str(output),
            "--github-output",
            str(record),
        ],
        check=False,
        cwd=ROOT,
        text=True,
        capture_output=True,
        timeout=300,
    )
    if result.returncode:
        return channel, {
            "status": "failed",
            "error": (result.stderr or result.stdout)[-3000:],
        }
    values = dict(line.split("=", 1) for line in record.read_text().splitlines())
    definitions = json.loads((output / "channels.json").read_text())
    expected = definitions["files"]
    verified = {name: digest(output / name) for name in expected}
    if values["publish"] != "false" or verified != expected:
        return channel, {
            "status": "failed",
            "error": "preview policy or definition digest mismatch",
        }
    return channel, {
        "status": "passed",
        "tag": values["tag"],
        "track": values["track"],
        "catalogue_sha256": digest(output / "release-manifest.json"),
        "definitions": verified,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--attempt", default="1")
    parser.add_argument(
        "--output", type=Path, default=ROOT / ".cache/channel-release-e2e"
    )
    args = parser.parse_args()
    command = [
        sys.executable,
        str(ROOT / "tools/release/package_channel.py"),
        "prepare",
        "--run-id",
        args.run_id,
        "--attempt",
        args.attempt,
    ]
    with TemporaryDirectory(prefix="unic-channel-release-e2e-") as temp:
        scratch = Path(temp)
        with ThreadPoolExecutor(max_workers=4) as executor:
            results = dict(
                executor.map(
                    lambda channel: exercise(channel, command, scratch), INPUTS
                )
            )
    passed = (
        all(result["status"] == "passed" for result in results.values())
        and len({result["catalogue_sha256"] for result in results.values()}) == 1
    )
    args.output.mkdir(parents=True, exist_ok=True)
    report = args.output / "report.json"
    report.write_text(
        json.dumps(
            {
                "status": "passed" if passed else "failed",
                "scope": "read-only preparation from real GitHub Actions and Release artifacts",
                "joint_release_run": args.run_id,
                "joint_release_attempt": args.attempt,
                "channels": results,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )
    (args.output / "SHA256SUMS.txt").write_text(f"{digest(report)}  {report.name}\n")
    print(report)
    sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()
