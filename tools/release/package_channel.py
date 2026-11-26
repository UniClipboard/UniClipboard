#!/usr/bin/env python3
"""Select a successful joint release and prepare one package channel."""

import argparse
import json
from pathlib import Path
import re
import shutil
import sys
from tempfile import TemporaryDirectory
from urllib.parse import quote

sys.dont_write_bytecode = True
import joint_plan as joint  # noqa: E402
from channels import render  # noqa: E402
from joint_plan import API, PREFIX, PUBLIC, REPO, GitHub, cli, desktop  # noqa: E402


# The destination workflow may consume only the formats it actually packages.
INPUTS = {
    "homebrew-tap": ("dmg", "darwin"),
    "homebrew-cask": ("dmg", "darwin"),
    "scoop": ("portable.zip", "windows"),
    "chocolatey": ("setup.exe", "windows"),
    "winget": ("setup.exe", "windows"),
    "aur": ("deb", "linux"),
    "pacman": ("archlinux", "linux"),
    "copr": ("rpm", "linux"),
    "snap": ("deb", "linux"),
    "nix": ("deb", "linux"),
    "flatpak": ("deb", "linux"),
}


def get_jobs(path):
    pages = json.loads(cli.run(["gh", "api", "--paginate", "--slurp", path]))
    return [job for page in pages for job in page["jobs"]]


def selection(gh, run_id, attempt, path):
    if not all(re.fullmatch(r"[1-9][0-9]*", value) for value in (run_id, attempt)):
        raise ValueError("expected a numeric joint run ID and attempt")
    run = gh.api("GET", f"{PREFIX}/actions/runs/{run_id}/attempts/{attempt}")
    default_branch = gh.api("GET", PREFIX)["default_branch"]
    if (
        run["path"].partition("@")[0] != ".github/workflows/release-cli.yml"
        or run["event"] != "workflow_dispatch"
        or run["head_repository"]["full_name"] != REPO
        or run["head_branch"] != default_branch
        or run["status"] != "completed"
        or run["conclusion"] != "success"
    ):
        raise ValueError("expected a successful joint release on the default branch")
    # Re-running failed jobs can reuse the successful promotion and npm
    # receipts from an earlier attempt of this same run.
    jobs = get_jobs(
        f"{PREFIX}/actions/runs/{run_id}/attempts/{attempt}/jobs?per_page=100"
    )
    verified = [
        job
        for job in jobs
        if job["name"] == "Verify both release results"
        and job["conclusion"] == "success"
    ]
    if not verified:
        # The attempt endpoint can omit jobs that an unsuccessful prior attempt reused.
        current = gh.api("GET", f"{PREFIX}/actions/runs/{run_id}")
        if current["run_attempt"] != int(attempt):
            raise ValueError(
                "cannot resolve jobs for a superseded joint release attempt"
            )
        jobs = get_jobs(f"{PREFIX}/actions/runs/{run_id}/jobs?per_page=100")
        verified = [
            job
            for job in jobs
            if job["name"] == "Verify both release results"
            and job["conclusion"] == "success"
        ]
    if not verified:
        return None
    for job_name, prefix in (
        ("Advance and verify Desktop channel / deliver", "desktop-delivery-promote"),
        ("Publish CLI to npm", "cli-publication-receipt"),
    ):
        matched = [
            job
            for job in jobs
            if job["name"] == job_name and job["conclusion"] == "success"
        ]
        if (
            not matched
            and run["run_attempt"]
            == gh.api("GET", f"{PREFIX}/actions/runs/{run_id}")["run_attempt"]
        ):
            latest = get_jobs(f"{PREFIX}/actions/runs/{run_id}/jobs?per_page=100")
            matched = [
                job
                for job in latest
                if job["name"] == job_name and job["conclusion"] == "success"
            ]
        [job] = matched
        artifact = f"{prefix}-{job['run_attempt']}"
        cli.run(
            [
                "gh",
                "run",
                "download",
                run_id,
                "--repo",
                REPO,
                "--name",
                artifact,
                "--dir",
                str(path / prefix),
            ]
        )
    path = path / "desktop-delivery-promote"
    manifest = path / "catalogue/release-manifest.json"
    m = cli.read_json(manifest)
    digest = cli.digest(manifest)
    receipt = cli.read_json(path / "promotion.json")
    channel = receipt["channel"].removeprefix("desktop/")
    if (
        channel not in {"alpha", "stable"}
        or receipt["channel"] != f"desktop/{channel}"
        or receipt["status"] != "promoted"
        or receipt["fixture"] is not False
        or receipt["manifest_sha256"] != digest
        or m["product"] != "desktop"
        or m["development"] is not False
        or m["source_clean"] is not True
    ):
        raise ValueError("invalid production Desktop promotion receipt")
    tag = "desktop/v" + cli.identity(m)
    joint.check_product_ref(gh, tag, m["source_sha"])
    release = joint.published_release(gh, tag)
    # An Alpha build can be promoted in a later run. It is still safe to
    # render an Alpha preview after that transition.
    if channel == "stable" and release["prerelease"]:
        raise ValueError("stable Desktop release is still a prerelease")
    verify_asset(release, manifest.name, digest, manifest.stat().st_size)
    npm = cli.read_json(path.parent / "cli-publication-receipt/publication.json")
    if (
        npm["status"] != "available"
        or npm["fixture"] is not False
        or npm["channel"] != channel
        or npm["manifest_sha256"] != m["cli_manifest_sha256"]
    ):
        raise ValueError("npm publication receipt differs from the Desktop CLI lock")
    return channel, tag, manifest, m, release


def verify_asset(release, filename, digest, size=None):
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._+-]{0,254}", filename):
        raise ValueError("invalid release asset name")
    matches = [asset for asset in release["assets"] if asset["name"] == filename]
    if (
        len(matches) != 1
        or matches[0]["digest"] != f"sha256:{digest}"
        or (size is not None and matches[0]["size"] != size)
    ):
        raise ValueError(
            f"published release asset differs from its manifest: {filename}"
        )


def check_current(channel, manifest):
    desktop.verify_channel_feed(
        desktop.request(PUBLIC, f"/v1/desktop/{channel}.json"), manifest
    )


def prepare(gh, args, work, selected):
    channel, tag, manifest, m, release = selected
    kind, platform = INPUTS[args.channel]
    live = args.publish and (channel == "stable" or args.channel == "copr")
    if live:
        check_current(channel, m)
    if channel == "stable":
        desktop.verify_stable_acceptance(
            manifest, m, manifest.parent.parent / "stable-acceptance.json"
        )

    path = work / "packages"
    files = []
    for arch in ("amd64", "arm64"):
        target = f"{platform}-{arch}"
        [asset] = [
            a for a in m["artifacts"] if (a["target"], a["format"]) == (target, kind)
        ]
        verify_asset(release, asset["file"], asset["sha256"], asset["size"])
        directory = path / target
        joint.release_assets(tag, directory, asset["file"])
        cli.checked_file(directory, asset["file"], asset["sha256"])
        cli.save(
            directory / "artifacts.json",
            {
                "schema": 1,
                "product": "desktop",
                "version": m["version"],
                "build_number": m["build_number"],
                "source_sha": m["source_sha"],
                "source_clean": True,
                "development": False,
                "target": target,
                "cli": {"manifest_sha256": m["cli_manifest_sha256"]},
                "artifacts": [asset],
                "acceptance": {"package_lifecycle": channel == "stable"},
            },
        )
        files.append(directory / asset["file"])

    cli_path = None
    if args.channel == "homebrew-tap":
        npm = cli.read_json(work / "cli-publication-receipt/publication.json")
        cli_tag = "cli/v" + cli.identity(npm)
        directory = work / "cli"
        cli_release = joint.published_release(gh, cli_tag)
        joint.release_assets(cli_tag, directory, "release-manifest.json")
        cli_path = directory / "release-manifest.json"
        if cli.digest(cli_path) != m["cli_manifest_sha256"]:
            raise ValueError("Homebrew Formula requires the exact Desktop CLI lock")
        verify_asset(
            cli_release,
            cli_path.name,
            m["cli_manifest_sha256"],
            cli_path.stat().st_size,
        )
        locked = cli.validate_manifest(cli_path)
        if not locked["source_clean"] or cli_tag != "cli/v" + cli.identity(locked):
            raise ValueError("released CLI manifest identity mismatch")
        joint.check_product_ref(gh, cli_tag, locked["source_sha"])
        for asset in locked["packages"]:
            if asset.get("target", "").startswith("darwin-"):
                verify_asset(cli_release, asset["file"], asset["sha256"])
                joint.release_assets(cli_tag, directory, asset["file"])
    base = f"https://github.com/{REPO}/releases/download/{quote(tag, safe='')}"
    output = Path(args.output).resolve()
    render(
        argparse.Namespace(
            input=path,
            base_url=base,
            output=output,
            channel=args.channel,
            development=False,
            cli_manifest=cli_path,
            stable=channel == "stable",
            release_channel=channel,
            release_date=release["published_at"][:10],
        )
    )
    (output / "release-manifest.json").write_bytes(manifest.read_bytes())
    if args.channel == "pacman":
        shutil.move(path, output / "packages")
    elif args.channel == "copr":
        (output / "copr/payloads").mkdir()
        for file in files:
            shutil.move(file, output / "copr/payloads" / file.name)
    with args.github_output.open("a", encoding="utf-8") as stream:
        print(f"publish={str(live).lower()}", file=stream)
        print(f"tag={tag}", file=stream)
        print(f"track={channel}", file=stream)
        print(f"version={m['version']}", file=stream)
        print(f"build={m['build_number']}", file=stream)
    print(f"{args.channel}: {tag}, {channel}, publish={live}")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("operation", choices=("select", "prepare", "check-current"))
    p.add_argument("--run-id")
    p.add_argument("--attempt")
    p.add_argument("--channel", choices=INPUTS)
    p.add_argument("--output")
    p.add_argument("--publish", action="store_true")
    p.add_argument("--manifest", type=Path)
    p.add_argument("--track", choices=("alpha", "stable"), default="stable")
    p.add_argument("--github-output", type=Path)
    args = p.parse_args()
    if args.operation == "check-current":
        check_current(args.track, cli.read_json(args.manifest))
        return
    gh = GitHub(API)
    with TemporaryDirectory(prefix="unic-package-release-") as temp:
        work = Path(temp)
        selected = selection(gh, args.run_id, args.attempt, work)
        if args.operation == "select":
            with args.github_output.open("a", encoding="utf-8") as stream:
                print(f"joint={str(selected is not None).lower()}", file=stream)
                print(
                    f"tag={selected[1] if selected is not None else ''}",
                    file=stream,
                )
        elif selected is None:
            raise ValueError("standalone CLI release has no Desktop packages")
        else:
            prepare(gh, args, work, selected)


if __name__ == "__main__":
    main()
