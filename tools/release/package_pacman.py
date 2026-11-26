#!/usr/bin/env python3
"""Advance the v1-compatible signed pacman GitHub repository."""

import argparse
from pathlib import Path
import re
import sys
from tempfile import TemporaryDirectory

sys.dont_write_bytecode = True
from joint_plan import API, PREFIX, REPO, GitHub, cli, desktop, release_assets  # noqa: E402
from package_channel import check_current, verify_asset  # noqa: E402


def verify_signature(file, fingerprint):
    output = cli.run(
        ["gpg", "--batch", "--status-fd=1", "--verify", str(file) + ".sig", file]
    )
    valid = [
        line.split()
        for line in output.splitlines()
        if line.startswith("[GNUPG:] VALIDSIG ")
    ]
    if len(valid) != 1 or fingerprint not in (valid[0][2], valid[0][-1]):
        raise ValueError("repository signing fingerprint mismatch")


def database_version(path):
    [description] = [
        name
        for name in cli.run(["bsdtar", "-tf", path]).splitlines()
        if name.endswith("/desc")
    ]
    fields = dict(
        re.findall(
            r"%([A-Z0-9]+)%\n([^\n]+)", cli.run(["bsdtar", "-xOf", path, description])
        )
    )
    if fields["NAME"] != "uniclipboard":
        raise ValueError("unexpected pacman package")
    return fields["VERSION"], fields["FILENAME"]


def publish(args):
    gh = GitHub(API)
    m = cli.read_json(args.manifest)
    check_current("stable", m)
    if not re.fullmatch(r"[A-F0-9]{40}|[A-F0-9]{64}", args.fingerprint):
        raise ValueError("PACMAN_SIGNING_FINGERPRINT must be the configured public key")
    tag = "pacman-stable-x86_64"
    command = ["gh", "release", "--repo", REPO]
    release = cli.find_github_release(
        REPO, tag, lambda endpoint: gh.api("GET", "/" + endpoint)
    )
    with TemporaryDirectory(prefix="unic-pacman-") as temp:
        work = Path(temp)
        [package] = args.snapshot.glob("*.pkg.tar.zst")
        sig = package.with_name(package.name + ".sig")
        verify_signature(package, args.fingerprint)
        [accepted] = [
            item
            for item in m["artifacts"]
            if item["target"] == "linux-amd64" and item["format"] == "archlinux"
        ]
        if (package.name, cli.digest(package)) != (
            accepted["file"],
            accepted["sha256"],
        ):
            raise ValueError(
                "signed repository package differs from the Stable catalogue"
            )
        index = args.snapshot / "uniclipboard.db"
        verify_signature(index, args.fingerprint)
        version, filename = database_version(index)
        if filename != package.name:
            raise ValueError("signed index points to another package")
        verify_signature(args.snapshot / "uniclipboard.files", args.fingerprint)
        metadata = [
            args.snapshot / "uniclipboard.asc",
            index,
            args.snapshot / "uniclipboard.db.sig",
            args.snapshot / "uniclipboard.files",
            args.snapshot / "uniclipboard.files.sig",
        ]
        fingerprint = work / "signing-fingerprint.txt"
        fingerprint.write_text(args.fingerprint + "\n")
        metadata.insert(1, fingerprint)
        old = {a["name"]: a for a in release["assets"]} if release else {}
        if release and release.get("immutable"):
            raise ValueError("pacman channel requires a mutable GitHub Release")
        same_version = False
        if release and not release["draft"]:
            previous = work / "previous"
            release_assets(tag, previous, "uniclipboard.db", "uniclipboard.db.sig")
            verify_signature(previous / index.name, args.fingerprint)
            prior_version, prior_file = database_version(previous / index.name)
            comparison = int(cli.run(["vercmp", version, prior_version]))
            if comparison < 0 or (comparison == 0 and prior_file != package.name):
                raise ValueError(
                    "pacman channel already selects this version or newer with different bytes"
                )
            same_version = comparison == 0
        if not release:
            cli.run(
                command
                + [
                    "create",
                    tag,
                    "--draft",
                    "--prerelease",
                    "--latest=false",
                    "--target",
                    m["source_sha"],
                    "--title",
                    "Pacman stable repository (x86_64)",
                    "--notes",
                    "Signed UniClipboard x86_64 package index.",
                ]
            )
            release = cli.find_github_release(
                REPO, tag, lambda endpoint: gh.api("GET", "/" + endpoint)
            )
        if package.name in old:
            verify_asset(
                release, package.name, cli.digest(package), package.stat().st_size
            )
        else:
            cli.run(command + ["upload", tag, str(package)])
        if sig.name in old:
            release_assets(tag, work, sig.name)
            saved = work / package.name
            saved.symlink_to(package.resolve())
            verify_signature(saved, args.fingerprint)
            sig = work / sig.name
        else:
            cli.run(command + ["upload", tag, str(sig)])
        endpoint = f"{PREFIX}/releases/{release['id']}"
        uploaded = gh.api("GET", endpoint)
        for path in (package, sig):
            verify_asset(uploaded, path.name, cli.digest(path), path.stat().st_size)
        if not same_version:
            # Keep the old packages. A client checking the index between uploads
            # rejects a mixed pair because the index signature is mandatory.
            cli.run(
                command
                + ["upload", tag, *(str(path) for path in metadata), "--clobber"]
            )
        if release["draft"]:
            cli.run(command + ["edit", tag, "--draft=false", "--latest=false"])
        final = gh.api("GET", endpoint)
        if final["draft"] or not final["prerelease"]:
            raise ValueError("pacman release state did not read back")
        if not same_version:
            for path in metadata:
                verify_asset(final, path.name, cli.digest(path), path.stat().st_size)
        if same_version:
            release_assets(
                tag, work / "previous", "uniclipboard.files", "uniclipboard.files.sig"
            )
            verify_signature(work / "previous/uniclipboard.files", args.fingerprint)
        desktop.save(
            args.receipt,
            {
                "status": "available",
                "repository": final["html_url"],
                "package": package.name,
                "package_sha256": accepted["sha256"],
                "version": version,
            },
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("snapshot", "manifest", "receipt"):
        parser.add_argument("--" + name, required=True, type=Path)
    parser.add_argument("--fingerprint", required=True)
    publish(parser.parse_args())


if __name__ == "__main__":
    main()
