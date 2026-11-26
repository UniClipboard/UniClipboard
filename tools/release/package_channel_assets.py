#!/usr/bin/env python3
"""Publish exact standalone package bundles in a dedicated GitHub Release."""

import argparse
from pathlib import Path
import sys
from tempfile import TemporaryDirectory
from urllib.parse import quote

sys.dont_write_bytecode = True
from joint_plan import API, PREFIX, REPO, GitHub, cli, desktop, release_assets  # noqa: E402
from package_channel import check_current, verify_asset  # noqa: E402


def publish(gh, channel, output, manifest, files):
    m = cli.read_json(manifest)
    check_current("stable", m)
    identity = cli.identity(m)
    tag = f"{channel}/v{identity}"
    base = ["gh", "release", "--repo", REPO]
    files = [*files, manifest]
    assets = {file.name: file for file in files}
    if len(assets) != len(files):
        raise ValueError("duplicate channel release asset")
    sums = output / "SHA256SUMS.txt"
    sums.write_text(
        "".join(f"{cli.digest(p)}  {name}\n" for name, p in sorted(assets.items()))
    )
    assets[sums.name] = sums

    # Paginate the release list so a resumable draft and a missing tag differ.
    release = cli.find_github_release(
        REPO, tag, lambda endpoint: gh.api("GET", "/" + endpoint)
    )
    endpoint = f"{PREFIX}/git/ref/tags/{quote(tag, safe='')}"
    ref = gh.api("GET", endpoint, missing=True)
    if ref is not None and (ref["object"]["type"], ref["object"]["sha"]) != (
        "commit",
        m["source_sha"],
    ):
        raise ValueError("channel release tag points to another build")
    if release is None:
        cli.run(
            base
            + [
                "create",
                tag,
                "--target",
                m["source_sha"],
                "--draft",
                "--prerelease",
                "--latest=false",
                "--title",
                f"UniClipboard {channel} {identity}",
                "--notes",
                f"Package-manager assets for Desktop {identity}.",
            ]
        )
        release = cli.find_github_release(
            REPO, tag, lambda endpoint: gh.api("GET", "/" + endpoint)
        )
    release_endpoint = f"{PREFIX}/releases/{release['id']}"
    if release["draft"]:
        stage(gh, base, tag, release_endpoint, release, assets)
    published = gh.api("GET", f"{PREFIX}/releases/tags/{quote(tag, safe='')}")
    if (
        published["id"] != release["id"]
        or published["draft"]
        or not published["prerelease"]
    ):
        raise ValueError("channel Release state did not read back")
    ref = gh.api("GET", endpoint)
    if (ref["object"]["type"], ref["object"]["sha"]) != ("commit", m["source_sha"]):
        raise ValueError("channel release tag points to another build")
    with TemporaryDirectory(prefix="unic-published-bundles-") as temp:
        path = Path(temp)
        release_assets(tag, path, manifest.name, sums.name)
        verify_asset(published, sums.name, cli.digest(path / sums.name))
        checksums = dict(
            reversed(line.split("  ", 1))
            for line in (path / sums.name).read_text().splitlines()
        )
        if (
            set(checksums) != assets.keys() - {sums.name}
            or checksums[manifest.name] != cli.digest(manifest)
            or cli.digest(path / manifest.name) != cli.digest(manifest)
            or {asset["name"] for asset in published["assets"]} != assets.keys()
        ):
            raise ValueError(
                "published bundle does not select the exact Desktop catalogue"
            )
        # Already published bundles may differ from a later repack. Verify the
        # original immutable selection rather than trying to replace it.
        for name, digest in checksums.items():
            verify_asset(published, name, digest)
        sums.write_bytes((path / sums.name).read_bytes())
    desktop.save(
        output / "publication.json",
        {
            "channel": channel,
            "status": "available",
            "tag": tag,
            "manifest_sha256": cli.digest(manifest),
            "url": published["html_url"],
        },
    )
    print(published["html_url"])


def stage(gh, base, tag, endpoint, release, assets):
    remote_assets = {a["name"] for a in release["assets"]}
    if remote_assets - assets.keys():
        raise ValueError("unexpected channel release asset")
    for name, file in assets.items():
        if name in remote_assets:
            verify_asset(release, name, cli.digest(file), file.stat().st_size)
        else:
            cli.run(base + ["upload", tag, str(file)])
    current = gh.api("GET", endpoint)
    for name, file in assets.items():
        verify_asset(current, name, cli.digest(file), file.stat().st_size)
    cli.run(base + ["edit", tag, "--draft=false", "--latest=false"])


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("channel", choices=["flatpak", "nix"])
    p.add_argument("--output", required=True, type=Path)
    p.add_argument("--manifest", required=True, type=Path)
    p.add_argument("files", nargs="+", type=Path)
    args = p.parse_intermixed_args()
    args.output.mkdir(parents=True, exist_ok=True)
    publish(GitHub(API), args.channel, args.output, args.manifest, args.files)


if __name__ == "__main__":
    main()
