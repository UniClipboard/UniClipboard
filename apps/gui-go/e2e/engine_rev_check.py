#!/usr/bin/env python3
"""Tie a shipped daemon binary to the Engine revision pinned in Cargo.toml, from the binary itself.

  engine_rev_check.py <uniclipd> [--cargo-toml Cargo.toml] [--expect-rev <40-hex>]

A Rust binary built from a git dependency embeds source paths such as `.../checkouts/engine-<hash>/<7-hex revision>/...`
(panic locations, tracing metadata). The set of revisions found for the Engine checkout must be exactly the pinned one. This
proves nothing about how the binary was built; it is the evidence that the bytes in a package contain that Engine and no other,
independent of any build log. It prints the revisions it found and exits non-zero on a mismatch or when none is found.
"""

import argparse
import re
import sys
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("binary", type=Path)
parser.add_argument("--cargo-toml", type=Path, default=Path("Cargo.toml"))
parser.add_argument("--expect-rev", help="40-hex revision; default: the uc-engine rev in --cargo-toml")
args = parser.parse_args()

expected = args.expect_rev
if expected is None:
    match = re.search(r'^uc-engine\s*=\s*\{[^}]*rev\s*=\s*"([0-9a-f]{40})"', args.cargo_toml.read_text(), re.M)
    if not match:
        sys.exit(f"{args.cargo_toml}: no uc-engine git rev found")
    expected = match.group(1)
found = sorted(
    {m.decode() for m in re.findall(rb"checkouts/engine-[0-9a-f]+[/\\]([0-9a-f]{7})", args.binary.read_bytes())}
)
print(f"{args.binary}: engine checkout revisions in the binary: {found or 'none'}; pinned: {expected[:7]} ({expected})")
if found != [expected[:7]]:
    sys.exit("MISMATCH: the binary does not contain exactly the pinned Engine revision")
print("OK")
