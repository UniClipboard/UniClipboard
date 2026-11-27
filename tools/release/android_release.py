#!/usr/bin/env python3
"""Android APK delivery: conditional R2 upload, FlareRelease registration/promotion, and GitHub Release staging."""

import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sys
import tempfile
from urllib.parse import quote, urlparse
from urllib.request import Request, urlopen

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/release"))
import desktop_release as desktop  # noqa: E402
import github_desktop as gh_desktop  # noqa: E402
import release as cli_release  # noqa: E402

REPO = gh_desktop.REPO
WEB = gh_desktop.WEB
API = gh_desktop.API
TEMPLATE = ROOT / ".github/release-notes/android.md.tmpl"


def write_text(path, text):
    Path(path).write_bytes(text.encode("utf-8"))


def sha256_file(path):
    return cli_release.digest(path)


def load_candidate(path, production=True):
    candidate_path = Path(path).resolve()
    data = json.loads(candidate_path.read_text(encoding="utf-8"))
    if (
        data.get("schema") != 1
        or data.get("platform") != "android"
        or data.get("format") != "apk"
        or not cli_release.SEMVER.fullmatch(data.get("version", ""))
        or type(data.get("build_number")) is not int
        or not 1 <= data["build_number"] <= 999999999
        or not re.fullmatch(r"[a-f0-9]{40}", data.get("source_sha", ""))
        or not re.fullmatch(r"[A-Za-z][A-Za-z0-9]*(?:\.[A-Za-z][A-Za-z0-9]*)+", data.get("application_id", ""))
    ):
        raise ValueError("invalid Android APK candidate identity")
    if data.get("architecture") not in ("arm64-v8a", "x86_64"):
        raise ValueError("unsupported Android APK architecture")
    if production and data.get("source_clean") is not True:
        raise ValueError("production Android delivery requires a clean source checkout")
    artifact = data.get("artifact") or {}
    name = artifact.get("name", "")
    expected_sha = artifact.get("sha256", "")
    expected_bytes = artifact.get("bytes")
    if not re.fullmatch(r"[A-Za-z0-9._-]+\.apk", name):
        raise ValueError("unsafe Android APK filename")
    apk_path = cli_release.checked_file(candidate_path.parent, name, expected_sha)
    if apk_path.stat().st_size != expected_bytes:
        raise ValueError("Android APK size mismatch")
    return candidate_path, data, apk_path


def load_notes(path):
    notes_bytes = Path(path).read_bytes()
    notes = json.loads(notes_bytes)
    if set(notes) != {"en", "zh"} or not all(
        isinstance(v, str) and v.strip() for v in notes.values()
    ):
        raise ValueError("English and Chinese changelog strings are required")
    return notes_bytes, notes


def check_android_contract(admin, public):
    for label, base, path, access in (
        ("admin", admin, "/api/contract", True),
        ("public", public, "/contract", False),
    ):
        c = desktop.request(base, path, access=access)
        if (
            c.get("buildIdentity") != "product-version-build-v1"
            or "android" not in c.get("products", [])
            or not {"alpha", "stable"} <= set(c.get("channels", []))
        ):
            raise ValueError(f"FlareRelease {label} does not implement numbered Android channels")


def registration_body(candidate, apk_path, public, notes, published_at, channel):
    prefix = f"artifacts/android/{candidate['version']}/build.{candidate['build_number']}/"
    key = prefix + apk_path.name
    size = candidate["artifact"]["bytes"]
    if size > 512 << 20:
        raise ValueError("Android APK exceeds FlareRelease 512 MiB limit")
    return {
        "product": "android",
        "version": candidate["version"],
        "buildNumber": str(candidate["build_number"]),
        "tagName": f"android/v{candidate['version']}-build.{candidate['build_number']}",
        "prerelease": channel == "alpha",
        "publishedAt": published_at,
        "source": candidate["source_sha"],
        "notes": notes,
        "artifacts": [
            {
                "platform": "android",
                "architecture": candidate["architecture"],
                "format": "apk",
                "filename": apk_path.name,
                "r2Key": key,
                "size": size,
                "downloadUrl": public + "/" + key,
                "sha256": candidate["artifact"]["sha256"],
            }
        ],
    }


def publish_r2(args):
    candidate_path, c, apk_path = load_candidate(args.candidate, production=not args.fixture)
    public = desktop.safe_base(args.public, args.fixture)
    admin = desktop.safe_base(args.admin, args.fixture)
    _, notes = load_notes(args.notes)
    published_at = args.published_at or c.get("published_at")
    if not published_at:
        raise ValueError("candidate publication timestamp is missing")
    parsed_date = datetime.fromisoformat(published_at.replace("Z", "+00:00"))
    if parsed_date.tzinfo is None:
        raise ValueError("published-at must include a timezone")
    body = registration_body(c, apk_path, public, notes, published_at, args.channel)
    check_android_contract(admin, public)
    client = desktop.storage(args.endpoint, args.fixture)
    prefix = f"artifacts/android/{c['version']}/build.{c['build_number']}/"
    files = [
        (prefix + apk_path.name, apk_path),
        (prefix + candidate_path.name, candidate_path),
    ]
    expected = {key: sha256_file(path) for key, path in files}
    from botocore.exceptions import ClientError

    def verify_stored(entry):
        key, path = entry
        return desktop.matches(client, args.bucket, key, path, expected[key])

    def store(entry):
        key, path = entry
        if sha256_file(path) != expected[key]:
            raise ValueError("Android candidate changed during publication")
        try:
            with path.open("rb") as stream:
                client.put_object(
                    Bucket=args.bucket,
                    Key=key,
                    Body=stream,
                    ContentLength=path.stat().st_size,
                    IfNoneMatch="*",
                    Metadata={"sha256": expected[key]},
                )
        except ClientError as error:
            if error.response["Error"]["Code"] not in ("PreconditionFailed", "412"):
                raise
        if not verify_stored(entry):
            raise ValueError("uploaded Android object is missing")

    def verify_public(entry):
        key, path = entry
        size = path.stat().st_size
        with urlopen(
            Request(
                public + "/" + key + "?source=origin",
                headers={"User-Agent": "UniClipboard-Release"},
            ),
            timeout=120,
        ) as response:
            h, n = hashlib.sha256(), 0
            while block := response.read(1 << 20):
                h.update(block)
                n += len(block)
                if n > size:
                    raise ValueError("public Android artifact exceeds accepted size")
        if n != size or h.hexdigest() != expected[key]:
            raise ValueError("public Android artifact read-back mismatch")

    with ThreadPoolExecutor(max_workers=2) as executor:
        present = list(executor.map(verify_stored, files))
        missing = [entry for entry, found in zip(files, present, strict=True) if not found]
        list(executor.map(store, missing))
        if any(sha256_file(path) != expected[key] for key, path in files):
            raise ValueError("Android candidate changed during publication")
        result = desktop.request(admin, "/api/releases/register", body, access=True)["data"]
        if result.get("status") != "ready":
            raise ValueError(f"FlareRelease registration did not reach ready: {result.get('status')!r}")
        list(executor.map(verify_public, files))

    desktop.save(
        args.receipt,
        {
            "status": "available",
            "product": "android",
            "version": c["version"],
            "build_number": c["build_number"],
            "candidate_sha256": sha256_file(candidate_path),
            "artifact_sha256": c["artifact"]["sha256"],
            "r2_prefix": prefix,
            "release": {"id": result["id"]},
            "objects": len(files),
            "channel_changed": False,
            "fixture": args.fixture,
        },
    )
    receipt_path = Path(args.receipt)
    receipt_key = prefix + "publication.json"
    if not desktop.matches(client, args.bucket, receipt_key, receipt_path):
        with receipt_path.open("rb") as stream:
            try:
                client.put_object(
                    Bucket=args.bucket,
                    Key=receipt_key,
                    Body=stream,
                    ContentLength=receipt_path.stat().st_size,
                    IfNoneMatch="*",
                )
            except ClientError as error:
                if error.response["Error"]["Code"] not in ("PreconditionFailed", "412"):
                    raise
        if not desktop.matches(client, args.bucket, receipt_key, receipt_path):
            raise ValueError("Android publication receipt read-back failed")


def verify_android_channel_feed(feed, candidate):
    if (feed.get("product"), feed.get("version"), str(feed.get("buildNumber"))) != (
        "android",
        candidate["version"],
        str(candidate["build_number"]),
    ):
        raise ValueError("public Android channel identity differs from accepted build")
    expected = (candidate["artifact"]["name"], candidate["artifact"]["sha256"])
    observed = {
        (a.get("filename"), a.get("sha256"))
        for a in feed.get("artifacts", [])
    }
    if expected not in observed:
        raise ValueError("public Android channel feed does not match accepted APK")


def promote(args):
    candidate_path, c, _ = load_candidate(args.candidate, production=not args.fixture)
    receipt = cli_release.read_json(args.publication)
    if (
        receipt.get("status") != "available"
        or receipt.get("product") != "android"
        or receipt.get("candidate_sha256") != sha256_file(candidate_path)
        or bool(receipt.get("fixture")) != args.fixture
    ):
        raise ValueError("matching Android publication read-back receipt is required")
    admin = desktop.safe_base(args.admin, args.fixture)
    public = desktop.safe_base(args.public, args.fixture)
    check_android_contract(admin, public)
    channel_id = "android/" + args.channel
    path = "/api/channels/" + quote(channel_id, safe="")
    state = desktop.request(admin, path, access=True)["data"]
    revision = args.expected_revision
    selected = False
    release_id = receipt["release"]["id"]
    if args.forward_only:
        version = state.get("currentVersion")
        if version:
            build = state.get("currentBuildNumber")
            current = f"{version}-build.{build}" if build else version
            target = cli_release.identity(c)
            if cli_release.channel_downgrade(current, target):
                raise ValueError("Android promotion cannot downgrade the selected channel")
            selected = current == target
        if revision is None:
            revision = state["internalRevision"]
    result = {"data": state}
    if not selected:
        if state["internalRevision"] != revision:
            raise ValueError("Android channel revision changed before promotion")
        result = desktop.request(
            admin,
            path + "/promote",
            {
                "releaseId": release_id,
                "expectedRevisions": {channel_id: revision},
            },
            access=True,
        )
    feed = desktop.request(public, "/v1/android/" + args.channel + ".json")
    verify_android_channel_feed(feed, c)
    desktop.save(
        args.receipt,
        {
            "status": "promoted",
            "channel": channel_id,
            "candidate_sha256": sha256_file(candidate_path),
            "result": result,
            "fixture": args.fixture,
        },
    )


def render_page(c, notes, channel, previous, generated, template):
    tag = f"android/v{c['version']}-build.{c['build_number']}"
    download = WEB + "/releases/download/" + quote(tag, safe="") + "/"
    apk = c["artifact"]["name"]
    rows = [
        "| Platform | Architecture | Format | Download |",
        "| --- | --- | --- | --- |",
        f"| Android | {c['architecture']} | APK | [{apk}]({download + quote(apk)}) |",
    ]
    changelog = f"## {c['version']} — Android build {c['build_number']}\n\n{notes['en']}"
    source = (
        f"**Full Changelog**: {WEB}/compare/{quote(previous, safe='')}...{c['source_sha']}"
        if previous
        else f"**Source**: {WEB}/commit/{c['source_sha']}"
    )
    warnings = []
    if channel == "alpha":
        warnings.append("## Prerelease Warning\n\nThis is an alpha release for testing.")
    warnings.append(
        "All paired devices must use v2/v3. Previous mobile history is imported offline on first launch."
    )
    contributors = ""
    if generated:
        match = re.search(r"^## New Contributors\s*\n(.*?)(?=^## |\Z)", generated, re.M | re.S)
        if match:
            contributors = "\n## New Contributors\n\n" + match[1].strip() + "\n"
    values = {
        "CHANGELOG_SECTION": changelog,
        "SOURCE_SECTION": source,
        "NOTES_URL": download + "android-notes.json",
        "WARNINGS": "\n\n".join(warnings),
        "INSTALLER_TABLE": "\n".join(rows),
        "SOURCE_SHA": c["source_sha"],
        "APPLICATION_ID": c["application_id"],
        "NEW_CONTRIBUTORS_SECTION": contributors,
    }
    text = template
    for key, value in values.items():
        text = text.replace("{{" + key + "}}", value)
    if re.search(r"{{[A-Z_]+}}", text):
        raise ValueError("unresolved Android Release template field")
    return text


def bundle_github(args):
    candidate_path, c, apk_path = load_candidate(args.candidate, production=not (args.fixture or args.development))
    notes_bytes, notes = load_notes(args.notes)
    output = Path(args.output).resolve()
    if output == candidate_path.parent:
        raise ValueError("GitHub page output must not replace candidate directory")
    output.mkdir(parents=True, exist_ok=True)
    files = {}
    shutil.copyfile(apk_path, output / apk_path.name)
    files[apk_path.name] = output / apk_path.name
    if sha256_file(files[apk_path.name]) != c["artifact"]["sha256"]:
        raise ValueError("APK changed during Release staging")
    shutil.copyfile(candidate_path, output / "candidate.json")
    files["candidate.json"] = output / "candidate.json"
    (output / "android-notes.json").write_bytes(notes_bytes)
    files["android-notes.json"] = output / "android-notes.json"
    generated = (
        Path(args.generated_notes).read_text(encoding="utf-8")
        if args.generated_notes
        else ""
    )
    template = args.template.read_text(encoding="utf-8")
    bodies = {
        ch: render_page(c, notes, ch, args.previous_tag, generated, template)
        for ch in ("alpha", "stable")
    }
    write_text(output / "release-notes.md", bodies[args.channel])
    context = {
        "schema": 1,
        "candidate_sha256": sha256_file(files["candidate.json"]),
        "notes_sha256": sha256_file(files["android-notes.json"]),
        "previous_tag": args.previous_tag,
        "generated_notes": generated,
    }
    write_text(output / "release-page.json", json.dumps(context, indent=2) + "\n")
    files["release-page.json"] = output / "release-page.json"
    write_text(
        output / "SHA256SUMS.txt",
        "".join(sha256_file(p) + "  " + name + "\n" for name, p in sorted(files.items())),
    )
    files["SHA256SUMS.txt"] = output / "SHA256SUMS.txt"
    return c, files, bodies


def matching_receipt(path, c, args, status, candidate_sha):
    if not path:
        raise ValueError("required Android availability or promotion receipt is missing")
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    if (
        data.get("status") != status
        or data.get("candidate_sha256") != candidate_sha
        or bool(data.get("fixture")) != args.fixture
    ):
        raise ValueError("Android Release availability/promotion receipt mismatch")
    if status == "available" and (
        data.get("product"),
        data.get("version"),
        data.get("build_number"),
    ) != ("android", c["version"], c["build_number"]):
        raise ValueError("Android Release availability identity mismatch")
    if status == "promoted" and data.get("channel") != "android/" + args.channel:
        raise ValueError("Android Release promotion channel mismatch")


def github_run(args):
    parsed = urlparse(args.api_base)
    if args.fixture:
        if (
            parsed.scheme != "http"
            or parsed.hostname != "127.0.0.1"
            or parsed.path
            or parsed.query
            or parsed.fragment
            or parsed.username
            or parsed.password
            or os.environ.get("GH_TOKEN") != "fixture-only"
            or os.environ.get("GH_ENTERPRISE_TOKEN") != "fixture-only"
        ):
            raise ValueError("fixture requires loopback API and disposable credentials")
    elif args.api_base != API:
        raise ValueError("production GitHub API is fixed")
    if args.development and args.operation != "preview":
        raise ValueError("development inputs can only generate a local preview")
    c, files, bodies = bundle_github(args)
    tag = f"android/v{c['version']}-build.{c['build_number']}"
    receipt = {
        "status": "preview",
        "tag": tag,
        "source_sha": c["source_sha"],
        "candidate_sha256": sha256_file(files["candidate.json"]),
        "fixture": args.fixture,
        "assets": {name: sha256_file(path) for name, path in sorted(files.items())},
    }
    if args.operation == "preview":
        return receipt
    if args.operation == "publish":
        matching_receipt(args.publication, c, args, "available", receipt["candidate_sha256"])
        if args.channel == "stable" or args.promotion:
            matching_receipt(args.promotion, c, args, "promoted", receipt["candidate_sha256"])
    gh = gh_desktop.GitHub(args.api_base)
    prefix = "/repos/" + REPO
    ref = gh.api("GET", prefix + "/git/ref/tags/" + quote(tag, safe=""), missing=True)
    if ref and (ref["object"].get("type"), ref["object"].get("sha")) != ("commit", c["source_sha"]):
        raise ValueError("Android tag source conflict")
    existing = cli_release.find_github_release(
        REPO, tag, read=lambda endpoint: gh.api("GET", "/" + endpoint)
    )
    title = f"UniClipboard Android {c['version']} (build {c['build_number']})"
    if existing and args.channel == "alpha" and not existing["draft"] and not existing["prerelease"]:
        raise ValueError("cannot downgrade stable Android Release to alpha")
    assets = {}
    if existing:
        items = gh.api("GET", prefix + f"/releases/{existing['id']}/assets?per_page=100")
        if len(items) >= 100 or len({a["name"] for a in items}) != len(items):
            raise ValueError("ambiguous Android Release asset catalogue")
        assets = {a["name"]: a for a in items}
        if set(assets) - set(files):
            raise ValueError("unexpected existing Android Release attachment")
        if "release-page.json" in assets:
            with tempfile.TemporaryDirectory(prefix="unic-android-gh-context-") as temporary:
                context_path = Path(temporary) / "release-page.json"
                gh.api("GET", prefix + f"/releases/assets/{assets['release-page.json']['id']}", download=context_path)
                context = json.loads(context_path.read_text(encoding="utf-8"))
                if (
                    context.get("schema") != 1
                    or context.get("candidate_sha256") != receipt["candidate_sha256"]
                    or context.get("notes_sha256") != sha256_file(files["android-notes.json"])
                    or not isinstance(context.get("previous_tag"), str)
                    or not isinstance(context.get("generated_notes"), str)
                ):
                    raise ValueError("Android Release page context identity mismatch")
                args.previous_tag = context["previous_tag"]
                args.generated_notes = str(Path(temporary) / "generated.md")
                write_text(args.generated_notes, context["generated_notes"])
                c, files, bodies = bundle_github(args)
                receipt["assets"] = {name: sha256_file(path) for name, path in sorted(files.items())}
        if existing["name"] != title or existing["body"] not in bodies.values():
            raise ValueError("existing Android Release notes differ from accepted inputs")
        with tempfile.TemporaryDirectory(prefix="unic-android-gh-readback-") as temporary:
            for name, a in assets.items():
                if a.get("digest"):
                    if a["digest"] != "sha256:" + receipt["assets"][name]:
                        raise ValueError("existing Android Release attachment byte conflict")
                    continue
                downloaded = Path(temporary) / name
                gh.api("GET", prefix + f"/releases/assets/{a['id']}", download=downloaded)
                if sha256_file(downloaded) != receipt["assets"][name]:
                    raise ValueError("existing Android Release attachment byte conflict")
    if existing and not existing["draft"] and set(assets) != set(files):
        raise ValueError("published Android Release attachment set is incomplete")
    if args.operation == "publish" and (not existing or set(assets) != set(files)):
        raise ValueError("complete draft staging is required before Android publication")
    if not ref:
        gh.api("POST", prefix + "/git/refs", {"ref": "refs/tags/" + tag, "sha": c["source_sha"]})
    if not existing:
        existing = gh.api(
            "POST",
            prefix + "/releases",
            {
                "tag_name": tag,
                "target_commitish": c["source_sha"],
                "name": title,
                "body": bodies[args.channel],
                "draft": True,
                "prerelease": args.channel == "alpha",
                "make_latest": "false",
            },
        )
    upload = gh_desktop.GitHub(args.api_base if args.fixture else "https://uploads.github.com")
    for name in sorted(set(files) - set(assets)):
        a = upload.api("POST", prefix + f"/releases/{existing['id']}/assets?name={quote(name)}", upload=files[name])
        assets[name] = a
        with tempfile.TemporaryDirectory(prefix="unic-android-gh-upload-") as temporary:
            downloaded = Path(temporary) / name
            gh.api("GET", prefix + f"/releases/assets/{a['id']}", download=downloaded)
            if sha256_file(downloaded) != receipt["assets"][name]:
                raise ValueError("uploaded Android Release attachment read-back mismatch")
    current_assets = gh.api("GET", prefix + f"/releases/{existing['id']}/assets?per_page=100")
    expected_assets = {name: (assets[name]["id"], path.stat().st_size) for name, path in files.items()}
    observed_assets = {a["name"]: (a["id"], a["size"]) for a in current_assets}
    if len(current_assets) != len(files) or observed_assets != expected_assets:
        raise ValueError("Android Release attachment set changed during staging")
    if args.operation == "publish":
        gh.api(
            "PATCH",
            prefix + f"/releases/{existing['id']}",
            {
                "draft": False,
                "prerelease": args.channel == "alpha",
                "body": bodies[args.channel],
                "make_latest": "true" if args.channel == "stable" else "false",
            },
        )
    final = gh.api("GET", prefix + f"/releases/{existing['id']}")
    if args.operation == "publish" and (
        final["draft"]
        or final["prerelease"] != (args.channel == "alpha")
        or final["body"] != bodies[args.channel]
    ):
        raise ValueError("published Android Release state read-back mismatch")
    receipt.update(
        status="draft" if final["draft"] else "published",
        url=final["html_url"],
        channel=args.channel,
    )
    return receipt


def main():
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest="command", required=True)

    pub = sub.add_parser("publish-r2")
    pub.add_argument("--candidate", required=True)
    pub.add_argument("--notes", required=True)
    pub.add_argument("--endpoint", required=True)
    pub.add_argument("--bucket", default="uniclipboard-releases")
    pub.add_argument("--admin", required=True)
    pub.add_argument("--public", default="https://release.uniclipboard.app")
    pub.add_argument("--channel", choices=["alpha", "stable"], default="alpha")
    pub.add_argument("--published-at", default="")
    pub.add_argument("--receipt", required=True)
    pub.add_argument("--fixture", action="store_true")

    pro = sub.add_parser("promote")
    pro.add_argument("--candidate", required=True)
    pro.add_argument("--publication", required=True)
    pro.add_argument("--admin", required=True)
    pro.add_argument("--public", default="https://release.uniclipboard.app")
    pro.add_argument("--channel", choices=["alpha", "stable"], required=True)
    pro.add_argument("--expected-revision", required=True)
    pro.add_argument("--forward-only", action="store_true")
    pro.add_argument("--receipt", required=True)
    pro.add_argument("--fixture", action="store_true")

    gh_cmd = sub.add_parser("github")
    gh_cmd.add_argument("operation", choices=["preview", "draft", "publish"])
    gh_cmd.add_argument("--candidate", required=True)
    gh_cmd.add_argument("--notes", required=True)
    gh_cmd.add_argument("--output", required=True)
    gh_cmd.add_argument("--template", type=Path, default=TEMPLATE)
    gh_cmd.add_argument("--channel", choices=["alpha", "stable"], default="alpha")
    gh_cmd.add_argument("--previous-tag", default="")
    gh_cmd.add_argument("--generated-notes", default="")
    gh_cmd.add_argument("--publication")
    gh_cmd.add_argument("--promotion")
    gh_cmd.add_argument("--api-base", default=API)
    gh_cmd.add_argument("--fixture", action="store_true")
    gh_cmd.add_argument("--development", action="store_true")

    args = p.parse_args()
    if args.command == "promote":
        if args.expected_revision.isdecimal():
            args.expected_revision = int(args.expected_revision)
        elif args.expected_revision == "auto" and args.forward_only:
            args.expected_revision = None
        else:
            p.error("expected revision must be a nonnegative integer or 'auto' with --forward-only")
    if getattr(args, "fixture", False):
        for name in ("admin", "public", "endpoint"):
            if hasattr(args, name) and urlparse(getattr(args, name)).hostname != "127.0.0.1":
                p.error("--fixture can access only loopback services")

    if args.command == "github":
        try:
            result = github_run(args)
        except BaseException as error:
            if Path(args.output).is_dir():
                write_text(
                    Path(args.output) / "github-receipt.json",
                    json.dumps(
                        {
                            "status": "failed",
                            "operation": args.operation,
                            "error_type": type(error).__name__,
                            "fixture": args.fixture,
                        }
                    )
                    + "\n",
                )
            raise
        write_text(Path(args.output) / "github-receipt.json", json.dumps(result, indent=2) + "\n")
        print(json.dumps({"status": result["status"], "tag": result["tag"]}))
        return

    try:
        {"publish-r2": publish_r2, "promote": promote}[args.command](args)
    except BaseException as error:
        desktop.save(
            args.receipt,
            {
                "status": "failed",
                "phase": args.command,
                "error_type": type(error).__name__,
                "fixture": args.fixture,
            },
        )
        raise


if __name__ == "__main__":
    main()
