#!/usr/bin/env python3
"""Immutable Desktop catalogue, conditional R2 delivery and FlareRelease API promotion."""

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
import subprocess
from urllib.error import HTTPError
from urllib.parse import quote, urlparse
from urllib.request import Request, urlopen

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/package"))
import desktop as package  # noqa: E402 - repository tool path

TARGETS = sorted(package.TARGETS)
# AppImage.tar.gz is the package-manager archive. update.tar.gz is the native SDK archive.
FORMATS = {
    "darwin": {"dmg", "app.tar.gz"},
    "windows": {"setup.exe", "portable.zip", "update.tar.gz"},
    "linux": {
        "deb",
        "rpm",
        "archlinux",
        "AppImage",
        "AppImage.tar.gz",
        "update.tar.gz",
    },
}


def save(path, data):
    Path(path).write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")


def safe_base(value, fixture=False):
    parsed = urlparse(value)
    if (
        parsed.username
        or parsed.password
        or parsed.query
        or parsed.fragment
        or not parsed.hostname
    ):
        raise ValueError("unsafe service URL")
    if parsed.scheme != "https" and not (
        fixture and parsed.scheme == "http" and parsed.hostname == "127.0.0.1"
    ):
        raise ValueError("service must use HTTPS")
    return value.rstrip("/")


def load(manifest, production=True):
    manifest = Path(manifest).resolve()
    m = json.loads(manifest.read_text())
    if (
        m.get("schema") != 1
        or m.get("product") != "desktop"
        or not package.cli.SEMVER.fullmatch(m.get("version", ""))
        or type(m.get("build_number")) is not int
        or not 1 <= m["build_number"] <= 999999999
    ):
        raise ValueError("invalid Desktop catalogue identity")
    if production and (m.get("development") or not m.get("source_clean")):
        raise ValueError("production requires clean accepted inputs")
    if not re.fullmatch("[a-f0-9]{40}", m.get("source_sha", "")) or not re.fullmatch(
        "[a-f0-9]{64}", m.get("cli_manifest_sha256", "")
    ):
        raise ValueError("invalid immutable source or CLI lock")
    slots = set()
    for a in m["artifacts"]:
        target, kind = a["target"], a["format"]
        if (
            target not in TARGETS
            or kind not in FORMATS[target.split("-")[0]]
            or (target, kind) in slots
        ):
            raise ValueError("invalid or duplicate catalogue slot")
        slots.add((target, kind))
        p = package.cli.checked_file(manifest.parent, a["file"], a["sha256"])
        if p.stat().st_size != a["size"]:
            raise ValueError("catalogue size mismatch")
        if production:
            acceptance = a["acceptance"]
            if (
                acceptance.get("native_identity") is not True
                or acceptance.get("shared_configuration_api") is not True
            ):
                raise ValueError("missing native identity acceptance")
            if target.startswith("darwin-") and not a.get("signed"):
                raise ValueError("missing production code signature")
            if target.startswith("darwin-") and not a.get("notarized"):
                raise ValueError("missing Apple notarization")
            if a["updater"]:
                if not a.get("signature") or not m.get("update_public_key"):
                    raise ValueError("missing native updater key/signature")
                package.run(
                    [
                        "go",
                        "run",
                        "./tools/release/update-sign",
                        "-verify",
                        "-public-key",
                        m["update_public_key"],
                        "-signature",
                        a["signature"],
                        "-file",
                        p,
                    ]
                )
    expected = {(t, f) for t in TARGETS for f in FORMATS[t.split("-")[0]]}
    if slots != expected:
        raise ValueError("incomplete six-target package catalogue")
    return manifest, m


def acquire(args):
    if not str(args.run_id).isdigit():
        raise ValueError("invalid candidate run ID")
    if args.source_sha and not args.current_artifacts:
        raise ValueError("--source-sha is supported only with --current-artifacts")
    repo = "UniClipboard/UniClipboard"

    def gh(*argv):
        return subprocess.check_output(["gh", *map(str, argv)], text=True).strip()

    run = json.loads(gh("api", f"repos/{repo}/actions/runs/{args.run_id}"))
    workflow_name = "release-cli.yml" if args.current_artifacts else "build-desktop.yml"
    workflow = json.loads(gh("api", f"repos/{repo}/actions/workflows/{workflow_name}"))
    if (
        run.get("workflow_id") != workflow["id"]
        or run.get("repository", {}).get("full_name") != repo
    ):
        raise ValueError("candidate is not a successful canonical repository build")
    if args.current_artifacts:
        if (str(run["id"]), run["head_sha"]) != (
            os.environ.get("GITHUB_RUN_ID"),
            os.environ.get("GITHUB_SHA"),
        ):
            raise ValueError("in-run candidate must belong to this workflow and source")
    elif run.get("conclusion") != "success":
        raise ValueError("candidate is not a successful canonical repository build")
    expected_source = run["head_sha"]
    if args.source_sha:
        if not re.fullmatch(r"[0-9a-f]{40}", args.source_sha):
            raise ValueError("invalid source SHA")
        if args.source_sha != run["head_sha"]:
            compare = json.loads(
                gh("api", f"repos/{repo}/compare/{args.source_sha}...{run['head_sha']}")
            )
            if (
                compare.get("status") not in ("identical", "ahead")
                or compare.get("behind_by") != 0
            ):
                raise ValueError(
                    "candidate source is not an ancestor of workflow head"
                )
        expected_source = args.source_sha
    output = package.fresh_output(args.output)
    import tempfile

    with tempfile.TemporaryDirectory(
        prefix=".desktop-candidate-", dir=output.parent
    ) as temporary:
        work = Path(temporary) / "packages"
        if args.current_artifacts:
            shutil.move(args.current_artifacts, work)
        else:
            work.mkdir()
            gh(
                "run",
                "download",
                args.run_id,
                "--repo",
                repo,
                "--pattern",
                "desktop-*",
                "--dir",
                work,
            )
        targets = set()
        slots = set()
        for path in work.rglob("artifacts.json"):
            m = package.cli.read_json(path)
            if (
                m["source_sha"] != expected_source
                or not m["source_clean"]
                or m["development"]
            ):
                raise ValueError("candidate source disagrees with successful workflow")
            targets.add(m["target"])
            for item in m["artifacts"]:
                slot = (m["target"], item["format"])
                if slot in slots:
                    raise ValueError("duplicate candidate target/format")
                slots.add(slot)
                package.cli.checked_file(path.parent, item["file"], item["sha256"])
        if slots != {(t, f) for t in TARGETS for f in FORMATS[t.split("-")[0]]}:
            raise ValueError("candidate lacks the complete 22-artifact matrix")
        if targets != set(TARGETS):
            raise ValueError("candidate does not contain six accepted native targets")
        save(
            work / "acquisition.json",
            {
                "run_id": int(args.run_id),
                "source_sha": expected_source,
                "created_at": run["created_at"],
                "repository": repo,
            },
        )
        work.rename(output)
    print(output)


def assemble(args):
    output = package.fresh_output(args.output)
    record = package.cli.metadata(args.metadata, "desktop")
    artifacts, identity, source_clean = [], None, True
    for path in sorted(Path(args.input).rglob("artifacts.json")):
        m = package.cli.read_json(path)
        if m.get("schema") != 1 or m.get("product") != "desktop":
            raise ValueError("invalid Desktop package record")
        key = (
            m["version"],
            m["build_number"],
            m["source_sha"],
            m["cli"]["manifest_sha256"],
        )
        if identity is not None and key != identity:
            raise ValueError("mixed Desktop or CLI identity")
        identity = key
        if (
            key[:2] != (record["version"], record["build_number"])
            or key[3] != record["cli_manifest_sha256"]
        ):
            raise ValueError("catalogue differs from selected product metadata")
        source_clean = source_clean and m["source_clean"] and not m["development"]
        for a in m["artifacts"]:
            source = package.cli.checked_file(path.parent, a["file"], a["sha256"])
            updater = a["format"] == (
                "app.tar.gz" if m["target"].startswith("darwin-") else "update.tar.gz"
            )
            artifacts.append(
                dict(
                    a,
                    target=m["target"],
                    updater=updater,
                    acceptance=m["acceptance"],
                    file=a["file"],
                    _source=source,
                )
            )
    if identity is None:
        raise ValueError("empty package catalogue")
    output.mkdir()
    try:
        for a in artifacts:
            destination = output / a["file"]
            if destination.exists():
                raise ValueError("duplicate immutable filename")
            destination.parent.mkdir(exist_ok=True)
            shutil.copyfile(a.pop("_source"), destination)
        manifest = output / "release-manifest.json"
        acquisition_path = Path(args.input) / "acquisition.json"
        acquisition = (
            package.cli.read_json(acquisition_path) if acquisition_path.exists() else {}
        )
        if acquisition and acquisition.get("source_sha") != identity[2]:
            raise ValueError("acquisition disagrees with package source")
        save(
            manifest,
            {
                "published_at": acquisition.get("created_at"),
                "schema": 1,
                "product": "desktop",
                "version": identity[0],
                "build_number": identity[1],
                "source_sha": identity[2],
                "cli_manifest_sha256": identity[3],
                "source_clean": source_clean,
                "development": args.development,
                "update_public_key": record.get("update_public_key", ""),
                "artifacts": artifacts,
            },
        )
        load(manifest, not args.development)
    except BaseException:
        shutil.rmtree(output)
        raise
    print(manifest)


def request(base, path, body=None, access=False, token=None):
    headers = {
        "Accept": "application/json",
        "Cache-Control": "no-cache",
        "User-Agent": "UniClipboard-Release",
    }
    if access:
        for env, header in [
            ("CF_ACCESS_CLIENT_ID", "CF-Access-Client-Id"),
            ("CF_ACCESS_CLIENT_SECRET", "CF-Access-Client-Secret"),
        ]:
            if not os.environ.get(env):
                raise ValueError("Cloudflare Access service credentials are required")
            headers[header] = os.environ[env]
    if token:
        headers["Authorization"] = "Bearer " + token
    data = None if body is None else json.dumps(body).encode()
    if data is not None:
        headers["Content-Type"] = "application/json"
    req = Request(base + path, data=data, headers=headers)
    # Never forward Access credentials through redirects.
    import urllib.request

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None

    with urllib.request.build_opener(NoRedirect).open(req, timeout=60) as response:
        data = response.read((2 << 20) + 1)
    if len(data) > 2 << 20:
        raise ValueError("release API response exceeds limit")
    return json.loads(data)


def check_contract(admin, public):
    c = request(admin, "/api/contract", access=True)
    if c.get("buildIdentity") != "product-version-build-v1" or "desktop" not in c.get(
        "products", []
    ):
        raise ValueError("FlareRelease does not implement numbered product builds")
    c = request(public, "/contract")
    if c.get("buildIdentity") != "product-version-build-v1" or "desktop" not in c.get(
        "products", []
    ):
        raise ValueError(
            "public FlareRelease does not implement numbered product builds"
        )


def storage(endpoint, fixture):
    import boto3
    from botocore.config import Config

    endpoint = safe_base(endpoint, fixture)
    key_id = os.environ.get("AWS_ACCESS_KEY_ID")
    secret = os.environ.get("AWS_SECRET_ACCESS_KEY")
    if bool(key_id) != bool(secret):
        raise ValueError("configure both R2 S3 credentials")
    if not key_id and not fixture:
        account = os.environ.get("CLOUDFLARE_ACCOUNT_ID", "")
        token = os.environ.get("CLOUDFLARE_API_TOKEN")
        if (
            not token
            or not re.fullmatch(r"[a-f0-9]{32}", account)
            or endpoint != f"https://{account}.r2.cloudflarestorage.com"
        ):
            raise ValueError(
                "R2 token reuse requires the configured account's default S3 endpoint"
            )
        key_id = cloudflare_token_id(account, token)
        secret = hashlib.sha256(token.encode()).hexdigest()
    return boto3.client(
        "s3",
        endpoint_url=endpoint,
        aws_access_key_id=key_id,
        aws_secret_access_key=secret,
        region_name="auto",
        config=Config(
            signature_version="s3v4",
            s3={"addressing_style": "path"},
            retries={"max_attempts": 3},
            request_checksum_calculation="when_required",
            response_checksum_validation="when_required",
        ),
    )


def cloudflare_token_id(account, token):
    for path in (f"/accounts/{account}/tokens/verify", "/user/tokens/verify"):
        try:
            response = request(
                "https://api.cloudflare.com/client/v4", path, token=token
            )
        except HTTPError as error:
            if error.code not in (400, 401, 403, 404):
                raise
            continue
        result = response.get("result") or {}
        key_id = result.get("id", "")
        if (
            response.get("success") is True
            and result.get("status") == "active"
            and re.fullmatch(r"[a-f0-9]{32}", key_id)
        ):
            return key_id
    raise ValueError("Cloudflare token verification did not return an active token ID")


def preflight(args):
    client = storage(args.endpoint, False)
    result = client.get_object(
        Bucket=args.bucket, Key="artifacts/v1.6.2/SHA256SUMS.txt"
    )
    with result["Body"] as stream:
        data = stream.read(2878)
    if len(data) != 2877 or hashlib.sha256(data).hexdigest() != (
        "07e5161104b7ae76508dbfb4df8d02198e874b06d1a945fe287af8eb840baf17"
    ):
        raise ValueError("R2 read-back of the known release object failed")
    print("R2 authenticated read-back passed", flush=True)
    check_contract(safe_base(args.admin), safe_base(args.public))
    print("FlareRelease admin and public numbered contracts passed")


def verify_cli(desktop, path, registry, stable=False):
    if not path or package.sha(path) != desktop["cli_manifest_sha256"]:
        raise ValueError("exact locked CLI manifest is required for Desktop delivery")
    cli = package.cli.validate_manifest(path)
    if set(cli["targets"]) != set(TARGETS):
        raise ValueError("locked CLI manifest must cover all Desktop targets")
    for item in cli["packages"]:
        data = package.cli.registry_get(registry, item["name"])
        remote = data.get("versions", {}).get(item["version"], {})
        if remote.get("dist", {}).get("integrity") != item["integrity"]:
            raise ValueError(
                "locked CLI npm integrity is not verified: " + item["name"]
            )
        if stable and data.get("dist-tags", {}).get("latest") != item["version"]:
            raise ValueError(
                "npm latest does not select the locked CLI package: " + item["name"]
            )


def verify_stable_acceptance(manifest, desktop, path):
    if not path:
        raise ValueError("stable requires three native acceptance receipts")
    receipts = package.cli.read_json(path)
    if not isinstance(receipts, dict) or set(receipts) != set(FORMATS):
        raise ValueError(
            "stable requires one receipt each for darwin, linux and windows"
        )
    digest = package.sha(manifest)
    for platform, receipt in receipts.items():
        if receipt.get("manifest_sha256") != digest:
            raise ValueError("stable acceptance catalogue digest mismatch: " + platform)
        safe_base(receipt.get("evidence", ""))
        expected = {
            a["file"]: a["sha256"]
            for a in desktop["artifacts"]
            if a["target"].startswith(platform + "-")
        }
        for artifact in receipt["artifacts"]:
            expected_digest = expected.pop(artifact["file"], None)
            if (
                expected_digest is None
                or expected_digest != artifact.get("sha256")
                or artifact.get("package_lifecycle") != "passed"
                or artifact.get("native_gui") != "passed"
            ):
                raise ValueError(
                    "invalid stable acceptance artifact: " + artifact["file"]
                )
        if expected:
            raise ValueError(
                "stable acceptance does not cover every artifact: " + platform
            )


def matches(client, bucket, key, path, expected_sha256=None):
    from botocore.exceptions import ClientError

    try:
        result = client.get_object(Bucket=bucket, Key=key)
    except ClientError as e:
        if e.response["Error"]["Code"] in ("NoSuchKey", "404"):
            return False
        raise
    h, size = hashlib.sha256(), 0
    with result["Body"] as body:
        expected_size = path.stat().st_size
        if expected_sha256 is None:
            expected_sha256 = package.sha(path)
        while block := body.read(1 << 20):
            size += len(block)
            if size > expected_size:
                raise ValueError("conflicting immutable object: " + key)
            h.update(block)
    if size != expected_size or h.hexdigest() != expected_sha256:
        raise ValueError("conflicting immutable object: " + key)
    return True


def registration(m, public, notes, published):
    prefix = f"artifacts/desktop/{m['version']}/build.{m['build_number']}/"
    artifacts = []
    for a in m["artifacts"]:
        if not a["updater"]:
            continue
        key = prefix + Path(a["file"]).name
        artifacts.append(
            {
                "platform": a["target"],
                "architecture": a["target"].split("-")[1],
                "format": a["format"],
                "filename": Path(a["file"]).name,
                "r2Key": key,
                "size": a["size"],
                "downloadUrl": public + "/" + key,
                "signature": a.get("signature", ""),
                "sha256": a["sha256"],
                "updater": True,
            }
        )
    if len(artifacts) != 6 or sum(a["size"] for a in artifacts) > 512 << 20:
        raise ValueError("updater set exceeds the FlareRelease registration contract")
    return {
        "product": "desktop",
        "version": m["version"],
        "buildNumber": str(m["build_number"]),
        "tagName": f"desktop/v{m['version']}-build.{m['build_number']}",
        "prerelease": False,
        "publishedAt": published,
        "source": m["source_sha"],
        "notes": notes,
        "artifacts": artifacts,
    }


def publish(args):
    manifest, m = load(args.manifest, not args.fixture)
    public = safe_base(args.public, args.fixture)
    admin = safe_base(args.admin, args.fixture)
    notes = package.cli.read_json(args.notes)
    if set(notes) != {"en", "zh"} or not all(
        isinstance(v, str) for v in notes.values()
    ):
        raise ValueError("notes must contain en and zh strings")
    published_at = args.published_at or m.get("published_at")
    if not published_at:
        raise ValueError("immutable candidate publication timestamp is missing")
    date = datetime.fromisoformat(published_at.replace("Z", "+00:00"))
    if date.tzinfo is None:
        raise ValueError("published-at must include a timezone")
    if not args.fixture:
        verify_cli(m, args.cli_manifest, "https://registry.npmjs.org")
    body = registration(m, public, notes, published_at)
    check_contract(admin, public)
    client = storage(args.endpoint, args.fixture)
    prefix = f"artifacts/desktop/{m['version']}/build.{m['build_number']}/"
    files = [
        (prefix + Path(a["file"]).name, manifest.parent / a["file"])
        for a in m["artifacts"]
    ]
    files.append((prefix + manifest.name, manifest))
    expected = {key: package.sha(path) for key, path in files}
    from botocore.exceptions import ClientError

    def verify_stored(entry):
        key, path = entry
        return matches(client, args.bucket, key, path, expected[key])

    def store(entry):
        key, path = entry
        if package.sha(path) != expected[key]:
            raise ValueError("candidate changed during publication")
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
        except ClientError as e:
            if e.response["Error"]["Code"] not in ("PreconditionFailed", "412"):
                raise
        if not verify_stored(entry):
            raise ValueError("uploaded object is missing")

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
                    raise ValueError("public artifact exceeds accepted size")
        if n != size or h.hexdigest() != expected[key]:
            raise ValueError("public artifact read-back mismatch")

    with ThreadPoolExecutor(max_workers=4) as executor:
        # Preflight all existing objects before the first remote mutation.
        present = list(executor.map(verify_stored, files))
        missing = [
            entry for entry, found in zip(files, present, strict=True) if not found
        ]
        list(executor.map(store, missing))
        if any(package.sha(path) != expected[key] for key, path in files):
            raise ValueError("candidate changed during publication")
        result = request(admin, "/api/releases/register", body, access=True)["data"]
        # Registration does not advance a channel.
        list(executor.map(verify_public, files))
    save(
        args.receipt,
        {
            "status": "available",
            "product": "desktop",
            "version": m["version"],
            "build_number": m["build_number"],
            "manifest_sha256": package.sha(manifest),
            "release": {"id": result["id"]},
            "objects": len(files),
            "channel_changed": False,
            "fixture": args.fixture,
        },
    )
    receipt_path = Path(args.receipt)
    receipt_key = prefix + "publication.json"
    if not matches(client, args.bucket, receipt_key, receipt_path):
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
        if not matches(client, args.bucket, receipt_key, receipt_path):
            raise ValueError("publication receipt read-back failed")


def verify_channel_feed(feed, m):
    if (feed.get("product"), feed.get("version"), feed.get("buildNumber")) != (
        "desktop",
        m["version"],
        str(m["build_number"]),
    ):
        raise ValueError("public channel identity differs from accepted build")
    expected = {(a["target"], a["sha256"]) for a in m["artifacts"] if a["updater"]}
    if {
        (a["platform"], a["sha256"])
        for a in feed.get("artifacts", [])
        if a.get("updater")
    } != expected:
        raise ValueError("public updater set differs from accepted build")


def promote(args):
    manifest, m = load(args.manifest, not args.fixture)
    receipt = package.cli.read_json(args.publication)
    if (
        receipt.get("status") != "available"
        or receipt.get("manifest_sha256") != package.sha(manifest)
        or bool(receipt.get("fixture")) != args.fixture
    ):
        raise ValueError("matching publication read-back receipt is required")
    if args.channel == "stable":
        verify_stable_acceptance(manifest, m, args.acceptance)
        registry, fixture_registry = package.cli.registry_url(args.registry)
        if fixture_registry != args.fixture:
            raise ValueError("a fixture must use an isolated npm registry")
        verify_cli(m, args.cli_manifest, registry, stable=True)
    admin, public = (
        safe_base(args.admin, args.fixture),
        safe_base(args.public, args.fixture),
    )
    check_contract(admin, public)
    channel_id = "desktop/" + args.channel
    path = "/api/channels/" + quote(channel_id, safe="")
    state = request(admin, path, access=True)["data"]
    revision = args.expected_revision
    selected = False
    release_id = receipt["release"]["id"]
    if args.forward_only:
        version = state["currentVersion"]
        if version:
            build = state["currentBuildNumber"]
            current = f"{version}-build.{build}" if build else version
            target = package.cli.identity(m)
            if package.cli.channel_downgrade(current, target):
                raise ValueError("joint release cannot downgrade the Desktop channel")
            selected = current == target
        if revision is None:
            revision = state["internalRevision"]
    result = {"data": state}
    if not selected:
        if state["internalRevision"] != revision:
            raise ValueError("channel revision changed before promotion")
        result = request(
            admin,
            path + "/promote",
            {
                "releaseId": release_id,
                "expectedRevisions": {channel_id: revision},
            },
            access=True,
        )
    feed = request(public, "/v1/desktop/" + args.channel + ".json")
    verify_channel_feed(feed, m)
    save(
        args.receipt,
        {
            "status": "promoted",
            "channel": channel_id,
            "manifest_sha256": package.sha(manifest),
            "result": result,
            "fixture": args.fixture,
        },
    )


def main():
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest="command", required=True)
    s = sub.add_parser("acquire")
    s.add_argument("--run-id", required=True)
    s.add_argument("--output", required=True)
    s.add_argument("--current-artifacts", type=Path)
    s.add_argument("--source-sha", default="")
    s = sub.add_parser("assemble")
    s.add_argument("--input", required=True)
    s.add_argument("--output", required=True)
    s.add_argument("--metadata", default=str(ROOT / "release/products.toml"))
    s.add_argument("--development", action="store_true")
    s = sub.add_parser("preflight")
    s.add_argument("--endpoint", required=True)
    s.add_argument("--bucket", default="uniclipboard-releases")
    s.add_argument("--admin", required=True)
    s.add_argument("--public", default="https://release.uniclipboard.app")
    for name in ("publish", "promote"):
        s = sub.add_parser(name)
        s.add_argument("--manifest", required=True)
        s.add_argument("--admin", required=True)
        s.add_argument("--public", default="https://release.uniclipboard.app")
        s.add_argument("--receipt", required=True)
        s.add_argument(
            "--fixture",
            action="store_true",
            help="Allow only isolated loopback fixture services",
        )
        if name == "publish":
            s.add_argument("--endpoint", required=True)
            s.add_argument("--bucket", default="uniclipboard-releases")
            s.add_argument("--cli-manifest")
            s.add_argument("--notes", required=True)
            s.add_argument("--published-at", default="")
        else:
            s.add_argument("--publication", required=True)
            s.add_argument("--acceptance")
            s.add_argument("--cli-manifest")
            s.add_argument("--registry", default="https://registry.npmjs.org")
            s.add_argument("--channel", choices=["alpha", "stable"], required=True)
            s.add_argument("--expected-revision", required=True)
            s.add_argument("--forward-only", action="store_true")
    args = p.parse_args()
    if args.command == "promote":
        if args.expected_revision.isdecimal():
            args.expected_revision = int(args.expected_revision)
        elif args.expected_revision == "auto" and args.forward_only:
            args.expected_revision = None
        else:
            p.error("expected revision must be a nonnegative integer")
    if getattr(args, "fixture", False):
        for name in ("admin", "public", "endpoint"):
            if (
                hasattr(args, name)
                and urlparse(getattr(args, name)).hostname != "127.0.0.1"
            ):
                p.error("--fixture can access only loopback services")
    try:
        {
            "acquire": acquire,
            "assemble": assemble,
            "preflight": preflight,
            "publish": publish,
            "promote": promote,
        }[args.command](args)
    except BaseException as error:
        if hasattr(args, "receipt"):
            save(
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
