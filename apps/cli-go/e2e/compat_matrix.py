#!/usr/bin/env python3
"""Generate the command compatibility matrix (Markdown) for the Go CLI.

Rows come from the Rust help oracle index (every visible and hidden command
path), columns from the Go command tree, the scenarios that exercise each path
(by scanning scenario sources for the path's argument tokens) and the latest
differential results.

Usage: compat_matrix.py --help-dir DIR --results DIR [--results DIR ...] > matrix.md
"""
import argparse
import ast
import glob
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
HIDDEN = {"status", "init", "invite", "join", "join status", "join cancel", "members", "devices", "recv"}
DEV_TOOLS = [
    ("probe watch|capture|restore|inspect", "Reads/writes the system clipboard through uc-platform in process"),
    ("blob publish|fetch", "Engine DevOperation::PublishBlob / FetchBlob in process"),
    ("dev pairing addrs|issue", "Engine DevOperation::ListPairingInvitationAddresses / constrained invitation in process"),
    ("dev seed-clipboard", "Engine DevOperation::SeedText in process"),
    ("dev dump-clipboard", "Engine Operation::ListHistoryEntries in process (prints decrypted previews)"),
    ("dev capture-files", "Engine DevOperation::CaptureFilePaths plus settings patches in process"),
    ("mobile debug put-text|put-file|get-doc|get-file", "Engine mobile-sync operations in process"),
]


def scenario_index():
    """Map scenario name -> list of argument lists found in its source."""
    out = {}
    for path in sorted(glob.glob(os.path.join(HERE, "scenarios*.py"))):
        tree = ast.parse(open(path).read())
        for node in tree.body:
            if isinstance(node, ast.FunctionDef) and any(
                    getattr(d, "id", None) == "scenario" for d in node.decorator_list):
                lists = []
                for sub in ast.walk(node):
                    if isinstance(sub, (ast.List, ast.Tuple)) and sub.elts and all(
                            isinstance(e, ast.Constant) and isinstance(e.value, str) for e in sub.elts):
                        lists.append([e.value for e in sub.elts])
                out[node.name] = (os.path.basename(path), lists)
    return out


def covers(path_tokens, arglist):
    words = [a for a in arglist if not a.startswith("-")]
    n = len(path_tokens)
    return any(words[i:i + n] == path_tokens for i in range(len(words) - n + 1)) and (
        len(words) == n or words[n:n + 1] == [] or path_tokens[-1] == words[n - 1])


def results(dirs):
    status = {}
    for d in dirs:
        tsv = os.path.join(d, "summary.tsv")
        if os.path.exists(tsv):
            for line in open(tsv):
                if "\t" in line:
                    s, name = line.strip().split("\t")
                    status[name] = s
    return status


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--help-dir", required=True)
    ap.add_argument("--results", action="append", default=[])
    args = ap.parse_args()
    paths = [p.strip() for p in open(os.path.join(args.help_dir, "index.txt")) if p.strip() and p.strip() != "<root>"]
    scen = scenario_index()
    res = results(args.results)
    print("| Command path | Visibility | Go | Scenarios (result) |")
    print("| --- | --- | --- | --- |")
    runnable_groups = {"search", "upgrade", "space join", "join"}
    for p in paths:
        tokens = p.split()
        if p not in runnable_groups and any(o.startswith(p + " ") for o in paths):
            vis = "hidden (deprecated alias)" if p.startswith("mobile-sync") else "public"
            print(f"| `{p}` | {vis} | ported | command group; see its subcommands |")
            continue
        vis = "hidden (deprecated alias)" if p in HIDDEN or p.startswith("mobile-sync") else "public"
        hits = []
        for name, (_, lists) in scen.items():
            if name == "argument_errors":
                continue
            if any(covers(tokens, l) for l in lists):
                hits.append(f"{name} ({res.get(name, 'not run')})")
        print(f"| `{p}` | {vis} | ported | {', '.join(hits) if hits else 'help and argument checks only'} |")
    print()
    print("Not in the release Rust CLI (`dev-tools` feature) and not ported:")
    print()
    print("| Command | Why it has no daemon route |")
    print("| --- | --- |")
    for cmd, why in DEV_TOOLS:
        print(f"| `{cmd}` | {why} |")


if __name__ == "__main__":
    main()
