#!/usr/bin/env python3
"""Rust/Go differential end-to-end runner for the uniclip CLI.

Each scenario runs once per CLI flavor against a real `uniclipd`, in a fresh
isolated HOME and profile (see isolated.py). Steps record exit code, stdout,
stderr and optional state probes; after normalizing volatile values (ids,
pids, ports, timestamps, temp paths) the two flavors are diffed step by step.

Usage:
  compat.py --rust DIR --go DIR [--out DIR] [--only NAME ...] [--list]

DIR holds `uniclip` and a sibling `uniclipd`. Exit status is non-zero when any
scenario differs or fails to run.
"""
import argparse
import difflib
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from isolated import isolated_env  # noqa: E402
import scenarios  # noqa: E402

NORMALIZERS = [
    (re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"), "<UUID>"),
    (re.compile(r"\b[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}-[0-9A-Z]{4}\b"), "<FINGERPRINT>"),
    (re.compile(r"(pid\\?[\"']?\s*[:=]?\s*)\d+"), r"\1<PID>"),
    (re.compile(r"127\.0\.0\.1:\d+"), "127.0.0.1:<PORT>"),
    (re.compile(r"\b1[6-9]\d{11}\b"), "<MS>"),
    (re.compile(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z?"), "<TIME>"),
]


class Step:
    def __init__(self, label, argv, code, out, err, extra=None):
        self.label, self.argv, self.code, self.out, self.err = label, argv, code, out, err
        self.extra = extra or {}


class Runner:
    """Runs one flavor of the CLI for one scenario."""

    def __init__(self, flavor, bindir, rust_dir, workdir, scenario):
        self.flavor = flavor
        self.bindir = bindir
        self.rust_dir = rust_dir
        self.home = tempfile.mkdtemp(prefix=f"{scenario}-{flavor}-", dir=workdir)
        self.profile = f"compat-{scenario}"
        self.steps = []
        self.background = []
        self.extra_profiles = []

    def env(self, profile=None, extra=None):
        return isolated_env(self.home, profile or self.profile, extra)

    def run(self, label, args, stdin=None, timeout=120, cli="self", profile=None, env=None, compare=True):
        """Run one CLI step. cli="self" uses the flavor under test; "rust"
        always uses the Rust baseline (fixture setup shared by both flavors)."""
        binary = os.path.join(self.rust_dir if cli == "rust" else self.bindir, "uniclip")
        argv = [binary, *args]
        try:
            proc = subprocess.run(argv, input=stdin, capture_output=True, timeout=timeout,
                                  env=self.env(profile, env))
            code, out, err = proc.returncode, proc.stdout, proc.stderr
        except subprocess.TimeoutExpired as exc:
            code, out, err = "timeout", exc.stdout or b"", exc.stderr or b""
        step = Step(label, args, code, out, err)
        if compare:
            self.steps.append(step)
        return step

    def spawn(self, label, args, profile=None, env=None, stdin=None, cli="self"):
        """Start a long-running CLI step (watch, invite, get --wait)."""
        binary = os.path.join(self.rust_dir if cli == "rust" else self.bindir, "uniclip")
        proc = subprocess.Popen([binary, *args], stdin=subprocess.PIPE if stdin is not None else subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=self.env(profile, env),
                                start_new_session=True)
        if stdin is not None:
            proc.stdin.write(stdin)
            proc.stdin.close()
        handle = {"label": label, "args": args, "proc": proc}
        self.background.append(handle)
        return handle

    def finish(self, handle, sig=None, timeout=60, compare=True):
        proc = handle["proc"]
        if sig is not None and proc.poll() is None:
            # Like a terminal Ctrl-C: signal the whole process group.
            os.killpg(proc.pid, sig)
        try:
            out, err = proc.communicate(timeout=timeout)
            code = proc.returncode
        except subprocess.TimeoutExpired:
            proc.kill()
            out, err = proc.communicate()
            code = "timeout"
        step = Step(handle["label"], handle["args"], code, out, err)
        if compare:
            self.steps.append(step)
        return step

    def note(self, label, value):
        """Record a state probe (any JSON-able value) for comparison."""
        self.steps.append(Step(label, [], 0, json.dumps(value, sort_keys=True, indent=2).encode(), b""))

    def data_root(self, profile=None):
        name = f"app.uniclipboard.desktop-{profile or self.profile}"
        if sys.platform == "darwin":
            return os.path.join(self.home, "Library", "Application Support", name)
        return os.path.join(self.home, ".local", "share", name)

    def cleanup(self):
        for handle in self.background:
            if handle["proc"].poll() is None:
                handle["proc"].kill()
        for profile in {self.profile, *self.extra_profiles}:
            subprocess.run([os.path.join(self.rust_dir, "uniclip"), "stop"], capture_output=True,
                           env=self.env(profile), timeout=60)


def normalize(text, home):
    text = text.replace(home, "<HOME>")
    text = re.sub(r"/private/var/folders/[^\s\"']+", "<TMP>", text)
    for pattern, repl in NORMALIZERS:
        text = pattern.sub(repl, text)
    return text


def render(step, home):
    out = step.out.decode("utf-8", "replace")
    err = step.err.decode("utf-8", "replace")
    return normalize(f"# {step.label}: {' '.join(step.argv)}\nexit={step.code}\n--- stdout\n{out}\n--- stderr\n{err}\n", home)


def main():
    # A runner started as a shell background job inherits SIGINT as ignored,
    # and children would inherit that too. Restore default handling so CLI
    # Ctrl-C behavior does not depend on how the runner was launched; the
    # ignored case is covered by explicit scenarios.
    if signal.getsignal(signal.SIGINT) == signal.SIG_IGN:
        signal.signal(signal.SIGINT, signal.default_int_handler)
    ap = argparse.ArgumentParser()
    ap.add_argument("--rust", required=True)
    ap.add_argument("--go", required=True)
    ap.add_argument("--out", default=None)
    ap.add_argument("--only", nargs="*")
    ap.add_argument("--list", action="store_true")
    args = ap.parse_args()
    if args.list:
        for name in scenarios.ALL:
            print(name)
        return 0
    rust_dir, go_dir = os.path.abspath(args.rust), os.path.abspath(args.go)
    out_dir = os.path.abspath(args.out or tempfile.mkdtemp(prefix="uniclip-compat-"))
    os.makedirs(out_dir, exist_ok=True)
    work = tempfile.mkdtemp(prefix="uniclip-compat-work-")
    failures = []
    summary = []
    for name, fn in scenarios.ALL.items():
        if args.only and name not in args.only:
            continue
        rendered = {}
        stale = os.path.join(out_dir, f"{name}.diff")
        if os.path.exists(stale):
            os.remove(stale)
        for flavor, bindir in (("rust", rust_dir), ("go", go_dir)):
            runner = Runner(flavor, bindir, rust_dir, work, name)
            started = time.time()
            try:
                fn(runner)
            except Exception as exc:  # scenario bug or hard failure
                runner.steps.append(Step("scenario-error", [], "error", b"", repr(exc).encode()))
            finally:
                runner.cleanup()
            rendered[flavor] = "".join(render(s, runner.home) for s in runner.steps)
            with open(os.path.join(out_dir, f"{name}.{flavor}.txt"), "w") as fh:
                fh.write(rendered[flavor])
            print(f"[{name}] {flavor}: {len(runner.steps)} steps in {time.time() - started:.1f}s", flush=True)
        diff = list(difflib.unified_diff(rendered["rust"].splitlines(True), rendered["go"].splitlines(True),
                                         f"{name}.rust", f"{name}.go"))
        status = "same" if not diff else "DIFF"
        if diff:
            failures.append(name)
            with open(os.path.join(out_dir, f"{name}.diff"), "w") as fh:
                fh.writelines(diff)
        summary.append(f"{status}\t{name}")
        print(f"[{name}] {status}", flush=True)
    with open(os.path.join(out_dir, "summary.tsv"), "w") as fh:
        fh.write("\n".join(summary) + "\n")
    shutil.rmtree(work, ignore_errors=True)
    print(f"results: {out_dir}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
