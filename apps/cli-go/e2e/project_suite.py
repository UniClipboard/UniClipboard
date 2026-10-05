#!/usr/bin/env python3
"""Run the repository's own CLI E2E suite (tests/e2e) against a chosen uniclip.

The suite resolves `uniclip` and `uniclipd` from `$CARGO_TARGET_DIR/debug` at
run time, so the same compiled test binaries can drive either CLI. Every run
uses a throwaway HOME and `UC_DISABLE_SYSTEM_CLIPBOARD=1`; the suite's own
default (real HOME, live system clipboard) is not used.

Usage:
  project_suite.py --target-dir DIR --out FILE [--threads N] [--only TEST_BINARY ...]
                   [--repo DIR] [--cli PATH] [--dev-cli PATH]

DIR/debug must contain `uniclipd` built with the `e2e-rendezvous` feature
(`cargo build -p uc-daemon --features uc-daemon/e2e-rendezvous`) and, unless
--cli is given, the user-facing CLI (`scripts/e2e/build-cli.sh`). --dev-cli
points the few tests that seed history at `uc-dev-cli`
(`cargo build -p uc-dev-cli --features uc-dev-cli/dev-tools`); --cli lets the
same compiled tests drive another CLI binary, for baseline comparisons.
--repo runs the suite from another checkout (default: this one), which is how
it runs from outside a directory tree with a cargo config override.
Writes one `binary<TAB>test<TAB>result` line per test to FILE.
"""
import argparse
import json
import os
import re
import subprocess
import sys
import tempfile

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
# Same skips as the CI job in .github/workflows/pr-check.yml.
SKIPS = ["bad_metadata_stays_read_only_across_three_restricted_starts",
         "cli_auto_start_exposes_restricted_recovery",
         "healthy_saved_profile_stays_healthy_after_restart",
         "abandoned_pairings_red_green"]
RESULT = re.compile(r"^test (\S+) \.\.\. (ok|FAILED|ignored)", re.M)


def test_binaries(repo):
    out = subprocess.run(["cargo", "test", "--manifest-path", os.path.join(repo, "tests", "e2e", "Cargo.toml"),
                          "--no-run", "--message-format=json"], capture_output=True, text=True, check=True).stdout
    bins = {}
    for line in out.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") == "compiler-artifact" and msg.get("executable") and msg["target"]["kind"] == ["test"]:
            bins[msg["target"]["name"]] = msg["executable"]
    return bins


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--target-dir", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--threads", default="2")
    ap.add_argument("--only", nargs="*")
    ap.add_argument("--repo", default=REPO)
    ap.add_argument("--cli")
    ap.add_argument("--dev-cli")
    args = ap.parse_args()
    repo = os.path.abspath(args.repo)
    target = os.path.abspath(args.target_dir)
    required = ["uniclipd"] + ([] if args.cli else ["uniclip"])
    for name in required:
        if not os.path.isfile(os.path.join(target, "debug", name)):
            sys.exit(f"missing {target}/debug/{name}")
    logs = args.out + ".logs"
    os.makedirs(logs, exist_ok=True)
    lines = []
    for name, exe in sorted(test_binaries(repo).items()):
        if args.only and name not in args.only:
            continue
        home = tempfile.mkdtemp(prefix=f"uc-suite-{name}-")
        env = {"HOME": home, "PATH": os.environ["PATH"], "TMPDIR": os.environ.get("TMPDIR", "/tmp"),
               "CARGO_TARGET_DIR": target, "UC_DISABLE_SYSTEM_CLIPBOARD": "1",
               "UC_E2E_EVIDENCE_DIR": os.path.join(logs, name + "-evidence")}
        if os.environ.get("CARGO_HOME"):
            env["CARGO_HOME"] = os.environ["CARGO_HOME"]
        if args.cli:
            env["UC_E2E_CLI"] = os.path.abspath(args.cli)
        if args.dev_cli:
            env["UC_E2E_DEV_CLI"] = os.path.abspath(args.dev_cli)
        argv = [exe, "--ignored", f"--test-threads={args.threads}"]
        for skip in SKIPS:
            argv += ["--skip", skip]
        proc = subprocess.run(argv, capture_output=True, text=True, env=env, cwd=os.path.join(repo, "tests", "e2e"))
        with open(os.path.join(logs, name + ".log"), "w") as fh:
            fh.write(proc.stdout + "\n--- stderr\n" + proc.stderr)
        found = RESULT.findall(proc.stdout)
        for test, result in found:
            lines.append(f"{name}\t{test}\t{result}")
        if not found:
            lines.append(f"{name}\t<binary>\texit={proc.returncode}")
        print(f"{name}: {len(found)} tests, exit {proc.returncode}", flush=True)
    with open(args.out, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
