#!/usr/bin/env python3
"""Immutable Desktop catalogue, conditional R2 delivery and FlareRelease API promotion."""
import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sys
import subprocess
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
            if acceptance.get("native_identity") is not True:
                raise ValueError("missing native identity acceptance")
            if target.startswith(("darwin-", "windows-")) and not a.get("signed"):
                raise ValueError("missing production code signature")
            if kind == "setup.exe" and not a.get("container_signed"):
                raise ValueError("missing Windows installer signature")
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
    repo = "UniClipboard/UniClipboard"

    def gh(*argv):
        return subprocess.check_output(["gh", *map(str, argv)], text=True).strip()

    run = json.loads(gh("api", f"repos/{repo}/actions/runs/{args.run_id}"))
    workflow = json.loads(gh("api", f"repos/{repo}/actions/workflows/build-cli.yml"))
    if (
        run.get("conclusion") != "success"
        or run.get("workflow_id") != workflow["id"]
        or run.get("repository", {}).get("full_name") != repo
    ):
        raise ValueError("candidate is not a successful canonical repository build")
    output = package.fresh_output(args.output)
    import tempfile

    with tempfile.TemporaryDirectory(
        prefix=".desktop-candidate-", dir=output.parent
    ) as temporary:
        work = Path(temporary) / "packages"
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
                m["source_sha"] != run["head_sha"]
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
                "source_sha": run["head_sha"],
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


def request(base, path, body=None, access=False):
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

    return boto3.client(
        "s3",
        endpoint_url=safe_base(endpoint, fixture),
        region_name="auto",
        config=Config(
            signature_version="s3v4",
            s3={"addressing_style": "path"},
            retries={"max_attempts": 3},
            request_checksum_calculation="when_required",
            response_checksum_validation="when_required",
        ),
    )


def matches(client, bucket, key, path):
    from botocore.exceptions import ClientError

    try:
        result = client.get_object(Bucket=bucket, Key=key)
    except ClientError as e:
        if e.response["Error"]["Code"] in ("NoSuchKey", "404"):
            return False
        raise
    h, size = hashlib.sha256(), 0
    with result["Body"] as body:
        while block := body.read(1 << 20):
            size += len(block)
            if size > path.stat().st_size:
                raise ValueError("conflicting immutable object: " + key)
            h.update(block)
    if size != path.stat().st_size or h.hexdigest() != package.sha(path):
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
        if (
            not args.cli_manifest
            or package.sha(args.cli_manifest) != m["cli_manifest_sha256"]
        ):
            raise ValueError(
                "exact locked CLI manifest is required before Desktop publication"
            )
        cli_manifest = package.cli.validate_manifest(args.cli_manifest)
        for item in cli_manifest["packages"]:
            remote = (
                package.cli.registry_get("https://registry.npmjs.org", item["name"])
                .get("versions", {})
                .get(item["version"])
            )
            if (
                not remote
                or remote.get("dist", {}).get("integrity") != item["integrity"]
            ):
                raise ValueError("locked CLI npm availability is not verified")
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
    # Preflight all existing objects before the first remote mutation.
    present = {key: matches(client, args.bucket, key, path) for key, path in files}
    from botocore.exceptions import ClientError

    for key, path in files:
        if package.sha(path) != expected[key]:
            raise ValueError("candidate changed during publication")
        if not present[key]:
            try:
                with path.open("rb") as stream:
                    client.put_object(
                        Bucket=args.bucket,
                        Key=key,
                        Body=stream,
                        ContentLength=path.stat().st_size,
                        IfNoneMatch="*",
                        Metadata={"sha256": package.sha(path)},
                    )
            except ClientError as e:
                if e.response["Error"]["Code"] not in ("PreconditionFailed", "412"):
                    raise
        if not matches(client, args.bucket, key, path):
            raise ValueError("uploaded object is missing")
    if any(package.sha(path) != expected[key] for key, path in files):
        raise ValueError("candidate changed during publication")
    result = request(admin, "/api/releases/register", body, access=True)["data"]
    # Registration does not advance a channel.
    for key, path in files:
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
                if n > path.stat().st_size:
                    raise ValueError("public artifact exceeds accepted size")
        if n != path.stat().st_size or h.hexdigest() != package.sha(path):
            raise ValueError("public artifact read-back mismatch")
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
        for a in m["artifacts"]:
            if (
                not a["acceptance"].get("package_lifecycle")
                or a["acceptance"].get("native_gui") != "passed"
            ):
                raise ValueError(
                    "stable requires package lifecycle and native GUI acceptance"
                )
    admin, public = safe_base(args.admin, args.fixture), safe_base(
        args.public, args.fixture
    )
    check_contract(admin, public)
    channel_id = "desktop/" + args.channel
    path = "/api/channels/" + quote(channel_id, safe="")
    state = request(admin, path, access=True)["data"]
    if state["internalRevision"] != args.expected_revision:
        raise ValueError("channel revision changed before promotion")
    release_id = receipt["release"]["id"]
    result = request(
        admin,
        path + "/promote",
        {
            "releaseId": release_id,
            "expectedRevisions": {channel_id: args.expected_revision},
        },
        access=True,
    )
    feed = request(public, "/v1/desktop/" + args.channel + ".json")
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
    s = sub.add_parser("assemble")
    s.add_argument("--input", required=True)
    s.add_argument("--output", required=True)
    s.add_argument("--metadata", default=str(ROOT / "release/products.toml"))
    s.add_argument("--development", action="store_true")
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
            s.add_argument("--channel", choices=["alpha", "stable"], required=True)
            s.add_argument("--expected-revision", type=int, required=True)
    args = p.parse_args()
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
