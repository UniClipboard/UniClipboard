#!/usr/bin/env python3
"""Reserve joint release identities and select accepted immutable builds."""

import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import re
import shutil
import sys
import tempfile
import tomllib
from urllib.error import HTTPError
from urllib.parse import quote

sys.dont_write_bytecode = True
import desktop_release as desktop  # noqa: E402
from github_desktop import API, REPO, GitHub  # noqa: E402
import release as cli  # noqa: E402

PREFIX = f"/repos/{REPO}"
RECORDS = "joint-release"
PUBLIC = "https://release.uniclipboard.app"
NPM = "https://registry.npmjs.org"
BUILD = r"[1-9][0-9]{0,8}"
IDENTITY = re.compile(rf"(.+)-build\.({BUILD})")


def github_pages(path):
    output = cli.run(["gh", "api", "--paginate", "--slurp", path])
    return [entry for page in json.loads(output) for entry in page]


def refs(prefix):
    return [
        item["ref"].removeprefix("refs/tags/")
        for item in github_pages(f"{PREFIX}/git/matching-refs/tags/{prefix}")
    ]


def release_assets(tag, directory, *patterns):
    directory.mkdir(parents=True, exist_ok=True)
    cli.run(
        ["gh", "release", "download", tag, "--repo", REPO, "--dir", str(directory)]
        + [arg for pattern in patterns for arg in ("--pattern", pattern)]
    )


def read_record(github, name):
    ref = github.api(
        "GET", f"{PREFIX}/git/ref/tags/{quote(name, safe='')}", missing=True
    )
    if ref is None:
        return None
    if ref["object"]["type"] != "tag":
        raise ValueError(f"expected annotated release record: {name}")
    tag = github.api("GET", f"{PREFIX}/git/tags/{ref['object']['sha']}")
    data = json.loads(tag["message"])
    obj = tag["object"]
    if (
        tag["tag"] != name
        or obj["type"] != "commit"
        or obj["sha"] != data["source_sha"]
        or data.get("schema") != 1
    ):
        raise ValueError(f"invalid release record: {name}")
    return data


def write_record(github, name, value):
    existing = read_record(github, name)
    if existing is None:
        annotated = github.api(
            "POST",
            f"{PREFIX}/git/tags",
            {
                "tag": name,
                "message": json.dumps(value, sort_keys=True),
                "object": value["source_sha"],
                "type": "commit",
                "tagger": {
                    "name": "github-actions[bot]",
                    "email": "41898282+github-actions[bot]@users.noreply.github.com",
                },
            },
        )
        github.api(
            "POST",
            f"{PREFIX}/git/refs",
            {
                "ref": f"refs/tags/{name}",
                "sha": annotated["sha"],
            },
        )
        existing = read_record(github, name)
    if existing != value:
        raise ValueError(f"immutable release record conflict: {name}")


def check_product_ref(github, tag, source):
    ref = github.api("GET", f"{PREFIX}/git/ref/tags/{quote(tag, safe='')}")
    obj = ref["object"]
    if obj["type"] != "commit" or obj["sha"] != source:
        raise ValueError(f"immutable build source conflict: {tag}")


def product_build(tag, product, version):
    match = re.fullmatch(rf"{product}/v{re.escape(version)}-build\.({BUILD})", tag)
    return int(match[1]) if match else 0


def published_release(github, tag):
    release = github.api("GET", f"{PREFIX}/releases/tags/{quote(tag, safe='')}")
    if release["tag_name"] != tag or release["draft"]:
        raise ValueError(f"published release record required: {tag}")
    return release


def released_cli(github, tag, directory):
    published_release(github, tag)
    release_assets(tag, directory, "release-manifest.json")
    path = directory / "release-manifest.json"
    manifest = cli.validate_manifest(path)
    if not manifest["source_clean"] or tag != "cli/v" + cli.identity(manifest):
        raise ValueError("released CLI manifest identity mismatch")
    check_product_ref(github, tag, manifest["source_sha"])
    return path, manifest


def compatible_cli(github, version, releases, targets, directory, npm_build):
    latest = max(
        (
            product_build(r["tag_name"], "cli", version)
            for r in releases
            if not r["draft"]
        ),
        default=0,
    )
    if not latest or latest < npm_build:
        return None
    path, manifest = released_cli(github, f"cli/v{version}-build.{latest}", directory)
    if set(manifest["targets"]) != set(targets):
        raise ValueError("released CLI target set disagrees with Desktop")
    for target in targets:
        try:
            native = desktop.package.check_cli_source(manifest, target)
        except desktop.package.SharedSourceMismatch as error:
            print(
                f"CLI build {latest} is incompatible for {target}: {error}", flush=True
            )
            return None
        if target.startswith("darwin-") and native.get("signed") is not True:
            raise ValueError("released CLI lacks required macOS signing")
    desktop.verify_cli({"cli_manifest_sha256": cli.digest(path)}, path, NPM)
    return {"build_number": latest, "manifest_sha256": cli.digest(path)}


def planned_metadata(path, config, plan):
    version = plan["version"]
    updates = {
        "cli": {
            "version": version,
            "build_number": plan["cli_build"],
            "targets": cli.metadata(config, "cli")["targets"],
        },
        "desktop": {
            "version": version,
            "build_number": plan["desktop_build"],
            "targets": cli.metadata(config, "desktop")["targets"],
            "update_public_key": cli.metadata(config, "desktop")["update_public_key"],
        },
    }
    path.write_text(
        "schema = 1\n"
        + "".join(
            f"\n[products.{product}]\n"
            + "".join(f"{key} = {json.dumps(value)}\n" for key, value in fields.items())
            for product, fields in updates.items()
        )
    )


def build_plan(github, source, version, run_id, config, directory):
    prefix = f"{RECORDS}/alpha/v{version}/"
    reservations = []
    for tag in refs(prefix):
        match = re.fullmatch(
            rf"{re.escape(prefix)}([1-9][0-9]*)-(0|{BUILD})-({BUILD})", tag
        )
        if not match:
            raise ValueError(f"invalid Alpha reservation name: {tag}")
        reservations.append((tag, match[1], int(match[2]), int(match[3])))
    matching = [item for item in reservations if item[1] == run_id]
    if matching:
        if len(matching) != 1:
            raise ValueError("run has multiple Alpha plans")
        tag, _, reserved_cli, reserved_desktop = matching[0]
        plan = read_record(github, tag)
        if (
            (plan["source_sha"], plan["version"], plan["run_id"], plan["channel"])
            != (source, version, run_id, "alpha")
            or plan["desktop_build"] != reserved_desktop
            or (0 if plan["reused_cli_sha256"] else plan["cli_build"]) != reserved_cli
        ):
            raise ValueError("saved Alpha plan identity conflict")
        return plan

    releases = github_pages(f"{PREFIX}/releases?per_page=100")
    product_refs = refs(f"cli/v{version}-build.") + refs(f"desktop/v{version}-build.")
    inventory = product_refs + [item["tag_name"] for item in releases]

    targets = cli.metadata(config, "desktop")["targets"]
    names = ["@uniclipboard/cli", *[cli.package_name(target) for target in targets]]
    with ThreadPoolExecutor(max_workers=4) as executor:
        published_npm = list(
            executor.map(lambda name: cli.registry_get(NPM, name), names)
        )
    npm_build = max(
        (
            product_build("cli/v" + identity, "cli", version)
            for npm in published_npm
            for identity in npm["versions"]
        ),
        default=0,
    )
    for npm in published_npm:
        channel = npm.get("dist-tags", {}).get("alpha")
        if channel and cli.channel_downgrade(channel, f"{version}-build.999999999"):
            raise ValueError("joint release cannot downgrade npm alpha")

    def allocate(product, reserved):
        record = cli.metadata(config, product)
        existing = max(
            (product_build(tag, product, version) for tag in inventory), default=0
        )
        floor = record["build_number"] if record["version"] == version else 1
        number = max(existing + 1, max(reserved, default=0) + 1, floor)
        if number > 999999999:
            raise ValueError(f"{product} build numbers exhausted")
        return number

    candidate = compatible_cli(github, version, releases, targets, directory, npm_build)
    desktop_build = allocate("desktop", [item[3] for item in reservations])
    reserved_cli = (
        0
        if candidate
        else allocate("cli", [npm_build, *[item[2] for item in reservations]])
    )
    plan = {
        "schema": 1,
        "source_sha": source,
        "version": version,
        "run_id": run_id,
        "channel": "alpha",
        "desktop_build": desktop_build,
        "cli_build": candidate["build_number"] if candidate else reserved_cli,
        "reused_cli_sha256": candidate["manifest_sha256"] if candidate else "",
    }
    write_record(github, f"{prefix}{run_id}-{reserved_cli}-{desktop_build}", plan)
    return plan


def alpha_candidate(manifest_path, cli_path):
    manifest = cli.read_json(manifest_path)
    candidate = cli.validate_manifest(cli_path)
    if (
        manifest["product"] != "desktop"
        or not manifest["source_clean"]
        or manifest["development"]
        or not candidate["source_clean"]
        or manifest["version"] != candidate["version"]
        or manifest["cli_manifest_sha256"] != cli.digest(cli_path)
    ):
        raise ValueError("Alpha CLI lock does not match the catalogue")
    return {
        "schema": 1,
        "source_sha": manifest["source_sha"],
        "version": manifest["version"],
        "build_number": manifest["build_number"],
        "manifest_sha256": cli.digest(manifest_path),
        "cli_manifest_sha256": manifest["cli_manifest_sha256"],
        "cli_build": candidate["build_number"],
    }


def marked_alpha(args, github):
    candidate = alpha_candidate(args.manifest, args.cli_manifest)
    receipt = cli.read_json(args.promotion)
    if (
        receipt.get("status"),
        receipt.get("channel"),
        receipt.get("manifest_sha256"),
        receipt.get("fixture"),
    ) != ("promoted", "desktop/alpha", candidate["manifest_sha256"], False):
        raise ValueError("verified Alpha promotion receipt required")
    name = cli.identity(candidate)
    check_product_ref(github, f"desktop/v{name}", candidate["source_sha"])
    write_record(github, f"{RECORDS}/published/v{name}", candidate)


def verify_alpha(github, candidate, directory):
    name = cli.identity(candidate)
    manifest_dir = directory / "desktop"
    tag = f"desktop/v{name}"
    release = published_release(github, tag)
    release_assets(tag, manifest_dir, "release-manifest.json")
    path = manifest_dir / "release-manifest.json"
    cli_path, _ = released_cli(
        github,
        f"cli/v{candidate['version']}-build.{candidate['cli_build']}",
        directory / "cli",
    )
    if alpha_candidate(path, cli_path) != candidate:
        raise ValueError("public Alpha assets disagree with the accepted identity")
    check_product_ref(github, tag, candidate["source_sha"])
    origin = desktop.request(
        PUBLIC,
        f"/artifacts/desktop/{candidate['version']}/build.{candidate['build_number']}/publication.json?source=origin",
    )
    if (
        origin.get("status"),
        origin.get("product"),
        origin.get("version"),
        origin.get("build_number"),
        origin.get("manifest_sha256"),
        origin.get("fixture"),
    ) != (
        "available",
        "desktop",
        candidate["version"],
        candidate["build_number"],
        candidate["manifest_sha256"],
        False,
    ):
        raise ValueError("matching Alpha origin publication is required")
    manifest = cli.read_json(path)
    assets = {asset["name"]: asset.get("digest") for asset in release["assets"]}
    for item in manifest["artifacts"]:
        file = item["file"]
        if file not in assets or assets[file] not in (None, "sha256:" + item["sha256"]):
            raise ValueError(f"published Alpha package integrity mismatch: {file}")
    for file in ("desktop-notes.json", "release-page.json", "SHA256SUMS.txt"):
        if file not in assets:
            raise ValueError(f"published Alpha release asset missing: {file}")
    desktop.verify_cli(manifest, cli_path, NPM)
    return path, manifest, cli_path


def recover_current_alpha(github, identity, version, directory):
    tag = f"desktop/v{identity}"
    published_release(github, tag)
    release_assets(tag, directory, "release-manifest.json")
    path = directory / "release-manifest.json"
    manifest = cli.read_json(path)
    if cli.identity(manifest) != identity:
        raise ValueError("published Alpha identity mismatch")
    desktop.verify_channel_feed(
        desktop.request(PUBLIC, "/v1/desktop/alpha.json"), manifest
    )
    for release in github_pages(f"{PREFIX}/releases?per_page=100"):
        cli_tag = release["tag_name"]
        if release["draft"] or not product_build(cli_tag, "cli", version):
            continue
        cli_path, _ = released_cli(github, cli_tag, directory / cli_tag)
        if cli.digest(cli_path) == manifest["cli_manifest_sha256"]:
            return alpha_candidate(path, cli_path)
    raise ValueError("published Alpha has no matching CLI build record")


def register_acceptance(args, github):
    match = IDENTITY.fullmatch(args.build)
    if not match or not cli.SEMVER.fullmatch(match[1]):
        raise ValueError(
            "expected a Desktop version and build, for example 2.0.0-build.8"
        )
    alpha_record = f"{RECORDS}/published/v{args.build}"
    candidate = read_record(github, alpha_record)
    with tempfile.TemporaryDirectory(prefix="unic-acceptance-") as work:
        work = Path(work)
        if candidate is None:
            candidate = recover_current_alpha(
                github, args.build, match[1], work / "legacy"
            )
        if cli.identity(candidate) != args.build:
            raise ValueError("matching verified Alpha publication is required")
        path, manifest, _ = verify_alpha(github, candidate, work)
        desktop.verify_stable_acceptance(path, manifest, args.acceptance)
    write_record(github, alpha_record, candidate)
    write_record(
        github,
        f"{RECORDS}/accepted/v{args.build}",
        {
            **candidate,
            "acceptance": cli.read_json(args.acceptance),
        },
    )


def stable_plan(github, source, version, run_id, output):
    name = f"{RECORDS}/stable/v{version}/{run_id}"
    plan = read_record(github, name)
    if plan is None:
        prefix = f"{RECORDS}/accepted/v{version}-build."
        eligible = [
            (int(tag.removeprefix(prefix)), tag)
            for tag in refs(prefix)
            if re.fullmatch(BUILD, tag.removeprefix(prefix))
        ]
        if not eligible:
            raise ValueError(f"no accepted Alpha candidate for {version}")
        build, tag = max(eligible)
        accepted = read_record(github, tag)
        if (accepted["version"], accepted["build_number"]) != (version, build):
            raise ValueError("accepted Alpha record identity mismatch")
        plan = {
            "schema": 1,
            "source_sha": source,
            "version": version,
            "run_id": run_id,
            "channel": "stable",
            "accepted": accepted,
        }
    if (plan["source_sha"], plan["version"], plan["run_id"], plan["channel"]) != (
        source,
        version,
        run_id,
        "stable",
    ):
        raise ValueError("saved Stable plan identity conflict")
    accepted = dict(plan["accepted"])
    receipts = accepted.pop("acceptance")
    with tempfile.TemporaryDirectory(prefix="unic-stable-") as work:
        path, manifest, cli_path = verify_alpha(github, accepted, Path(work))
        cli.save(output / "stable-acceptance.json", receipts)
        desktop.verify_stable_acceptance(
            path, manifest, output / "stable-acceptance.json"
        )
        (output / "cli-manifest.json").write_bytes(cli_path.read_bytes())
    try:
        state = desktop.request(PUBLIC, "/v1/desktop/stable.json")
    except HTTPError as error:
        if error.code != 404:
            raise
    else:
        current = state["version"]
        if state.get("buildNumber"):
            current += "-build." + str(state["buildNumber"])
        if cli.channel_downgrade(current, cli.identity(accepted)):
            raise ValueError("joint release cannot downgrade the Desktop channel")
    write_record(github, name, plan)
    return plan


def fetch_source_file(github, source_sha, relpath, destination_root, missing=False):
    data = github.api(
        "GET",
        f"{PREFIX}/contents/{relpath}?ref={source_sha}",
        missing=missing,
    )
    if data is None:
        return None
    if data.get("encoding") != "base64":
        raise ValueError(f"unsupported release source encoding for {relpath}")
    raw = base64.b64decode(data["content"])
    target = destination_root / relpath
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(raw)
    return raw


def export_source(github, output, version, build_number, source_sha):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    source_root = output / "source"
    if source_root.exists() and not source_root.is_dir():
        raise ValueError("exported source path is not a directory")
    staging_source = Path(
        tempfile.mkdtemp(prefix=".source-staging-", dir=output)
    )
    backup_source = None
    try:
        (staging_source / ".source-commit").write_text(
            source_sha + "\n", encoding="utf-8"
        )
        toml_raw = fetch_source_file(
            github, source_sha, "release/products.toml", staging_source, missing=True
        )
        candidates = [build_number]
        if toml_raw is not None:
            desktop_meta = (
                tomllib.loads(toml_raw.decode("utf-8"))
                .get("products", {})
                .get("desktop", {})
            )
            toml_build = desktop_meta.get("build_number")
            if (
                desktop_meta.get("version") == version
                and type(toml_build) is int
                and 1 <= toml_build < build_number
            ):
                candidates.append(toml_build)
        for candidate_build in candidates:
            en_rel = f"release/changelogs/desktop/v{version}-build.{candidate_build}.md"
            zh_rel = (
                f"release/changelogs/desktop/v{version}-build.{candidate_build}.zh.md"
            )
            en_raw = fetch_source_file(
                github, source_sha, en_rel, staging_source, missing=True
            )
            zh_raw = fetch_source_file(
                github, source_sha, zh_rel, staging_source, missing=True
            )
            if en_raw is not None or zh_raw is not None:
                break
        if source_root.exists():
            backup_source = Path(
                tempfile.mkdtemp(prefix=".source-old-", dir=output)
            )
            backup_source.rmdir()
            source_root.rename(backup_source)
        try:
            staging_source.rename(source_root)
        except BaseException:
            if (
                backup_source is not None
                and backup_source.exists()
                and not source_root.exists()
            ):
                backup_source.rename(source_root)
            raise
    finally:
        if staging_source.exists():
            shutil.rmtree(staging_source, ignore_errors=True)
        if backup_source is not None and backup_source.exists():
            shutil.rmtree(backup_source, ignore_errors=True)
    return source_root


def fetch_stable(args, github):
    plan = cli.read_json(args.plan)
    accepted = dict(plan["accepted"])
    receipts = accepted.pop("acceptance")
    tag = "desktop/v" + cli.identity(accepted)
    release_assets(tag, args.output, "*")
    manifest = args.output / "release-manifest.json"
    if alpha_candidate(manifest, args.cli_manifest) != accepted:
        raise ValueError("Stable acquisition does not match the selected Alpha")
    check_product_ref(github, tag, accepted["source_sha"])
    if receipts != cli.read_json(args.acceptance):
        raise ValueError("Stable acceptance receipt differs from the selection")
    desktop.verify_stable_acceptance(manifest, cli.read_json(manifest), args.acceptance)
    template = github.api(
        "GET",
        f"{PREFIX}/contents/.github/release-notes/desktop.md.tmpl?ref={accepted['source_sha']}",
    )
    if template["encoding"] != "base64":
        raise ValueError("unsupported release page template encoding")
    (args.output / "release-template.md").write_bytes(
        base64.b64decode(template["content"])
    )
    manifest_data = cli.read_json(manifest)
    export_source(
        github,
        args.output,
        manifest_data["version"],
        manifest_data["build_number"],
        accepted["source_sha"],
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("plan")
    p.add_argument("--channel", required=True, choices=["alpha", "stable"])
    p.add_argument("--version", required=True)
    p.add_argument("--run-id", required=True)
    p.add_argument("--source", required=True)
    p.add_argument("--metadata", type=Path, default=Path("release/products.toml"))
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--github-output", type=Path, required=True)
    p = sub.add_parser("mark-alpha")
    p.add_argument("--manifest", type=Path, required=True)
    p.add_argument("--cli-manifest", type=Path, required=True)
    p.add_argument("--promotion", type=Path, required=True)
    p = sub.add_parser("accept")
    p.add_argument("--build", required=True)
    p.add_argument("--acceptance", type=Path, required=True)
    p = sub.add_parser("fetch-stable")
    p.add_argument("--plan", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--cli-manifest", type=Path, required=True)
    p.add_argument("--acceptance", type=Path, required=True)
    args = parser.parse_args()
    github = GitHub(API)
    if args.command == "mark-alpha":
        marked_alpha(args, github)
    elif args.command == "accept":
        register_acceptance(args, github)
    elif args.command == "fetch-stable":
        fetch_stable(args, github)
    else:
        if (
            not cli.SEMVER.fullmatch(args.version)
            or not re.fullmatch(r"[1-9][0-9]*", args.run_id)
            or args.source != cli.run(["git", "rev-parse", "HEAD"])
        ):
            raise ValueError("invalid joint release version, run or source")
        args.output.mkdir(parents=True, exist_ok=True)
        base = (github, args.source, args.version, args.run_id)
        if args.channel == "alpha":
            with tempfile.TemporaryDirectory(prefix="unic-plan-") as work:
                plan = build_plan(*base, args.metadata, Path(work))
            planned_metadata(args.output / "products.toml", args.metadata, plan)
            with args.github_output.open("a") as output:
                print(f"desktop_build={plan['desktop_build']}", file=output)
                print(f"reused_cli_sha256={plan['reused_cli_sha256']}", file=output)
        else:
            plan = stable_plan(*base, args.output)
            with args.github_output.open("a") as output:
                print(
                    f"cli_manifest_sha256={plan['accepted']['cli_manifest_sha256']}",
                    file=output,
                )
        cli.save(args.output / "plan.json", plan)


if __name__ == "__main__":
    main()
