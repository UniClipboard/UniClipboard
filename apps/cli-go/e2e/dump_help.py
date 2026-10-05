#!/usr/bin/env python3
"""Recursively dump `--help` for every visible and known-hidden command path.

Usage: dump_help.py <uniclip-binary> <out-dir>

Writes <out-dir>/<path joined by '_'>.txt (stdout+stderr+exit code) and
index.txt. Used as the help-compatibility oracle for the Go CLI.
"""
import os
import subprocess
import sys
import tempfile

from isolated import isolated_env

# clap does not list hidden commands; enumerate the ones main.rs declares.
HIDDEN = ["status", "init", "invite", "join", "join status", "join cancel",
          "members", "devices", "recv", "mobile-sync"]


_HOME = tempfile.mkdtemp(prefix="uniclip-help-")


def run(binary, args):
    # Isolated even for help: a mis-parsed case may reach a command that
    # spawns a daemon.
    env = isolated_env(_HOME, "compat-help")
    proc = subprocess.run([binary, *args], capture_output=True, text=True, env=env)
    return proc.returncode, proc.stdout, proc.stderr


def subcommands(text):
    out, on = [], False
    for line in text.splitlines():
        if line.startswith("Commands:"):
            on = True
            continue
        if on:
            if not line.strip():
                break
            name = line.split()[0]
            if name != "help":
                out.append(name)
    return out


def main():
    binary, outdir = sys.argv[1], sys.argv[2]
    os.makedirs(outdir, exist_ok=True)
    seen, order = set(), []

    def walk(path):
        if path in seen:
            return
        seen.add(path)
        order.append(path)
        name = (path or "root").replace(" ", "_")
        for flag, suffix in (("--help", ""), ("-h", ".short")):
            code, out, err = run(binary, [*path.split(), flag])
            with open(os.path.join(outdir, name + suffix + ".txt"), "w") as fh:
                fh.write(f"exit={code}\n--- stdout\n{out}--- stderr\n{err}")
        code, out, err = run(binary, [*path.split(), "--help"])
        for sub in subcommands(out):
            walk(f"{path} {sub}".strip())

    walk("")
    for hidden in HIDDEN:
        walk(hidden)
    for name, args in (("_bare", []), ("_version", ["--version"]), ("_help_sub", ["help", "send"]),
                       ("_unknown", ["frobnicate"]), ("_bad_flag", ["send", "--nope"]),
                       ("_conflict", ["send", "--text", "--file", "x"]),
                       ("_reset_noyes", ["space", "reset"]),
                       ("_capture_bound", ["debug", "capture", "start", "--minutes", "16"])):
        code, out, err = run(binary, args)
        with open(os.path.join(outdir, name + ".txt"), "w") as fh:
            fh.write(f"exit={code}\n--- stdout\n{out}--- stderr\n{err}")
    with open(os.path.join(outdir, "index.txt"), "w") as fh:
        fh.write("\n".join(p or "<root>" for p in order) + "\n")


if __name__ == "__main__":
    main()
