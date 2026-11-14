#!/usr/bin/env python3
"""Build a signed pacman repository from exact accepted package files."""

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from desktop import cli, fresh_output, sha

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("--input", required=True)
p.add_argument("--output", required=True)
p.add_argument("--key", required=True)
a = p.parse_args()
if not os.environ.get("GNUPGHOME"):
    raise ValueError("use an explicit disposable GNUPGHOME for repository signing")
source = Path(a.input)
records = []
for path in source.rglob("artifacts.json"):
    m = cli.read_json(path)
    if m["target"] != "linux-amd64":
        continue
    for item in m["artifacts"]:
        if item["format"] == "archlinux":
            records.append(
                (m, item, cli.checked_file(path.parent, item["file"], item["sha256"]))
            )
if len(records) != 1:
    raise ValueError("expected one accepted x86_64 pacman package")
m, item, package = records[0]
if (
    m.get("development")
    or not m.get("source_clean")
    or m["acceptance"].get("package_lifecycle") is not True
):
    raise ValueError(
        "repository signing requires accepted production package lifecycle"
    )
output = fresh_output(a.output)
with tempfile.TemporaryDirectory(prefix=".unic-repo-", dir=output.parent) as temporary:
    work = Path(temporary)
    shutil.copy(package, work / package.name)

    def run(args):
        return subprocess.run(
            args, cwd=work, check=True, text=True, stdout=subprocess.PIPE
        ).stdout

    run(["gpg", "--batch", "--local-user", a.key, "--detach-sign", package.name])
    run(["gpg", "--batch", "--verify", package.name + ".sig", package.name])
    run(["repo-add", "--sign", "--key", a.key, "uniclipboard.db.tar.gz", package.name])
    run(
        [
            "gpg",
            "--batch",
            "--verify",
            "uniclipboard.db.tar.gz.sig",
            "uniclipboard.db.tar.gz",
        ]
    )
    pub = run(["gpg", "--batch", "--armor", "--export", a.key])
    if not pub:
        raise ValueError("signing public key is missing")
    (work / "uniclipboard.asc").write_text(pub)
    cli.save(
        work / "repository.json",
        {
            "schema": 1,
            "product": "desktop",
            "version": m["version"],
            "build_number": m["build_number"],
            "source_sha": m["source_sha"],
            "package_sha256": item["sha256"],
            "files": {
                p.name: sha(p)
                for p in work.iterdir()
                if p.is_file() and not p.is_symlink()
            },
            "availability": "pending",
        },
    )
    output.mkdir()
    for path in work.iterdir():
        path.rename(output / path.name)
print(output / "repository.json")
