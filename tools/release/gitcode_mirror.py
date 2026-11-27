#!/usr/bin/env python3
"""Prepare and register GitCode release mirrors for Desktop and Android builds."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlparse
from urllib.request import HTTPRedirectHandler, Request, build_opener

sys.dont_write_bytecode = True

DEFAULT_REPO = "UniClipboard/UniClipboard"
DEFAULT_GITHUB_API = "https://api.github.com"
DEFAULT_PUBLIC_URL = "https://release.uniclipboard.app"
DEFAULT_ADMIN_URL = "https://release-admin.uniclipboard.app"

# Keep in sync with FlareRelease src/domain/mirror.ts (the server enforces it too).
ALLOWED_MIRROR_HOSTS = ("gitcode.com", "atomgit.com")
# Keep in sync with mkdir700/gitcode-release-mirror lib/mirror-core.mjs.
GITCODE_TAG_RE = re.compile(r"^[0-9A-Za-z][0-9A-Za-z._+-]{0,127}$")
FILE_NAME_RE = re.compile(r"^[0-9A-Za-z][0-9A-Za-z._+-]{0,127}$")
RELEASE_TAG_RE = re.compile(
    r"^(desktop|android)/v(\d+\.\d+\.\d+)-build\.([1-9][0-9]{0,8})$"
)
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
EXPECTED_DESKTOP_UPDATER_COUNT = 6


class MirrorError(Exception):
    def __init__(self, message: str, *, permanent: bool = False):
        super().__init__(message)
        self.permanent = permanent


class _NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


_NO_REDIRECT_OPENER = build_opener(_NoRedirect)


def redact(text: str, secrets: list[str | None]) -> str:
    output = re.sub(r"access_token=[^&\s\"']+", "access_token=***", str(text))
    for secret in secrets:
        if secret:
            output = output.replace(secret, "***")
    return output


def parse_release_tag(tag_name: str) -> tuple[str, str, int]:
    match = RELEASE_TAG_RE.fullmatch(tag_name)
    if not match:
        raise MirrorError(
            f"unsupported release tag {tag_name!r}; expected desktop/v<semver>-build.<n> or android/v<semver>-build.<n>"
        )
    product, version, build_str = match.group(1), match.group(2), match.group(3)
    return product, version, int(build_str)


def to_gitcode_tag(product: str, version: str, build_number: int) -> str:
    tag = f"{product}-v{version}-build.{build_number}"
    if not GITCODE_TAG_RE.fullmatch(tag) or ".." in tag:
        raise MirrorError(f"derived GitCode tag {tag!r} is invalid")
    return tag


def validate_base_url(value: str, label: str, *, allow_local: bool = False) -> str:
    parsed = urlparse(value)
    if (
        parsed.username
        or parsed.password
        or parsed.query
        or parsed.fragment
        or not parsed.hostname
    ):
        raise MirrorError(f"{label} must be a plain URL without credentials, query, or fragment")
    if parsed.scheme == "https":
        return value.rstrip("/")
    if (
        allow_local
        and parsed.scheme == "http"
        and parsed.hostname in ("127.0.0.1", "localhost")
    ):
        return value.rstrip("/")
    raise MirrorError(f"{label} must use https")


def is_registrable_mirror_url(value: str, *, allow_local: bool = False) -> bool:
    if not isinstance(value, str) or not value:
        return False
    try:
        parsed = urlparse(value)
    except ValueError:
        return False
    if (
        allow_local
        and parsed.scheme == "http"
        and parsed.hostname in ("127.0.0.1", "localhost")
        and not parsed.username
        and not parsed.password
        and not parsed.query
        and not parsed.fragment
    ):
        return True
    if (
        parsed.scheme != "https"
        or parsed.username
        or parsed.password
        or parsed.port is not None
        or parsed.query
        or parsed.fragment
        or not parsed.hostname
        or len(parsed.path) > 1024
    ):
        return False
    host = parsed.hostname.lower()
    return any(host == allowed or host.endswith("." + allowed) for allowed in ALLOWED_MIRROR_HOSTS)


def _validate_filename(name: object) -> str:
    if (
        not isinstance(name, str)
        or not FILE_NAME_RE.fullmatch(name)
        or ".." in name
        or "/" in name
        or "\\" in name
    ):
        raise MirrorError(f"unsafe artifact filename: {name!r}")
    return name


def _validate_sha256(value: object, label: str) -> str:
    if not isinstance(value, str) or not SHA256_RE.fullmatch(value):
        raise MirrorError(f"{label} must be 64 lowercase hex characters")
    return value


def _validate_positive_size(value: object, label: str) -> int:
    if type(value) is not int or value <= 0:
        raise MirrorError(f"{label} must be a positive integer")
    return value


def _github_request(
    url: str,
    token: str,
    *,
    accept: str = "application/vnd.github+json",
    timeout_s: float = 60.0,
    max_bytes: int = 8 << 20,
    follow_redirects: bool = False,
) -> bytes:
    headers = {
        "Accept": accept,
        "Authorization": f"Bearer {token}",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "UniClipboard-Release",
    }
    req = Request(url, headers=headers, method="GET")
    opener = build_opener() if follow_redirects else _NO_REDIRECT_OPENER
    try:
        with opener.open(req, timeout=timeout_s) as response:
            data = response.read(max_bytes + 1)
    except HTTPError as error:
        body = error.read(4096).decode("utf-8", errors="replace")
        raise MirrorError(f"GitHub API {url} returned HTTP {error.code}: {body[:300]}") from error
    except URLError as error:
        raise MirrorError(f"GitHub API {url} failed: {error.reason}") from error
    if len(data) > max_bytes:
        raise MirrorError(f"GitHub API response from {url} exceeded size limit")
    return data


def _download_release_asset_json(
    api_base: str, repo: str, assets_by_name: dict[str, dict], asset_name: str, token: str
) -> dict:
    asset = assets_by_name.get(asset_name)
    if not asset:
        raise MirrorError(f"GitHub Release is missing required metadata asset {asset_name!r}")
    asset_id = asset.get("id")
    if type(asset_id) is not int or asset_id <= 0:
        raise MirrorError(f"invalid GitHub asset id for {asset_name!r}")
    url = f"{api_base}/repos/{repo}/releases/assets/{asset_id}"
    # GitHub asset downloads redirect to a signed object-storage URL; follow redirects
    # without sending Authorization to the redirected host.
    headers = {
        "Accept": "application/octet-stream",
        "Authorization": f"Bearer {token}",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "UniClipboard-Release",
    }

    class _StripAuthRedirect(HTTPRedirectHandler):
        def redirect_request(self, req, fp, code, msg, hdrs, newurl):
            redirected = super().redirect_request(req, fp, code, msg, hdrs, newurl)
            if redirected is not None:
                redirected.remove_header("Authorization")
                redirected.remove_header("X-github-api-version")
            return redirected

    opener = build_opener(_StripAuthRedirect)
    try:
        with opener.open(Request(url, headers=headers, method="GET"), timeout=60.0) as response:
            raw = response.read((4 << 20) + 1)
    except HTTPError as error:
        body = error.read(4096).decode("utf-8", errors="replace")
        raise MirrorError(
            f"downloading GitHub asset {asset_name!r} returned HTTP {error.code}: {body[:300]}"
        ) from error
    except URLError as error:
        raise MirrorError(f"downloading GitHub asset {asset_name!r} failed: {error.reason}") from error
    if len(raw) > (4 << 20):
        raise MirrorError(f"GitHub metadata asset {asset_name!r} exceeds size limit")
    digest = asset.get("digest")
    if digest:
        actual = "sha256:" + hashlib.sha256(raw).hexdigest()
        if digest != actual:
            raise MirrorError(
                f"GitHub metadata asset {asset_name!r} digest mismatch (expected {digest}, got {actual})"
            )
    if asset.get("size") is not None and len(raw) != asset["size"]:
        raise MirrorError(f"GitHub metadata asset {asset_name!r} size mismatch")
    try:
        parsed = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise MirrorError(f"invalid JSON in {asset_name!r}: {error}") from error
    if not isinstance(parsed, dict):
        raise MirrorError(f"expected JSON object in {asset_name!r}")
    return parsed


def _cross_check_release_asset(
    assets_by_name: dict[str, dict], filename: str, size: int, sha256: str
) -> None:
    asset = assets_by_name.get(filename)
    if not asset:
        raise MirrorError(f"published GitHub Release is missing artifact {filename!r}")
    if asset.get("size") != size:
        raise MirrorError(
            f"GitHub Release asset {filename!r} size {asset.get('size')} differs from manifest size {size}"
        )
    digest = asset.get("digest")
    if digest and digest != f"sha256:{sha256}":
        raise MirrorError(
            f"GitHub Release asset {filename!r} digest {digest} differs from manifest sha256:{sha256}"
        )


def _extract_desktop_artifacts(
    manifest: dict,
    assets_by_name: dict[str, dict],
    version: str,
    build_number: int,
    public_url: str,
) -> list[dict]:
    if (
        manifest.get("schema") != 1
        or manifest.get("product") != "desktop"
        or manifest.get("version") != version
        or manifest.get("build_number") != build_number
    ):
        raise MirrorError("Desktop release-manifest.json identity does not match the release tag")
    if manifest.get("development") is not False or manifest.get("source_clean") is not True:
        raise MirrorError("refusing to mirror a non-production or dirty Desktop build")
    raw_artifacts = manifest.get("artifacts")
    if not isinstance(raw_artifacts, list):
        raise MirrorError("Desktop release-manifest.json has no artifacts list")
    prefix = f"{public_url}/artifacts/desktop/{version}/build.{build_number}/"
    selected: list[dict] = []
    seen_names: set[str] = set()
    for item in raw_artifacts:
        if not isinstance(item, dict) or item.get("updater") is not True:
            continue
        raw_file = item.get("file")
        if not isinstance(raw_file, str):
            raise MirrorError("Desktop updater entry is missing file name")
        filename = _validate_filename(Path(raw_file).name)
        if filename != raw_file:
            raise MirrorError(f"Desktop updater file must be a flat filename: {raw_file!r}")
        if filename in seen_names:
            raise MirrorError(f"duplicate Desktop updater filename: {filename!r}")
        seen_names.add(filename)
        sha256 = _validate_sha256(item.get("sha256"), f"{filename} sha256")
        size = _validate_positive_size(item.get("size"), f"{filename} size")
        _cross_check_release_asset(assets_by_name, filename, size, sha256)
        selected.append(
            {
                "filename": filename,
                "size": size,
                "sha256": sha256,
                "url": prefix + quote(filename, safe=""),
            }
        )
    if len(selected) != EXPECTED_DESKTOP_UPDATER_COUNT:
        raise MirrorError(
            f"expected {EXPECTED_DESKTOP_UPDATER_COUNT} Desktop updater artifacts, found {len(selected)}"
        )
    return selected


def _extract_android_artifacts(
    candidate: dict,
    assets_by_name: dict[str, dict],
    version: str,
    build_number: int,
    public_url: str,
) -> list[dict]:
    if (
        candidate.get("schema") != 1
        or candidate.get("platform") != "android"
        or candidate.get("format") != "apk"
        or candidate.get("version") != version
        or candidate.get("build_number") != build_number
    ):
        raise MirrorError("Android candidate.json identity does not match the release tag")
    if candidate.get("source_clean") is not True:
        raise MirrorError("refusing to mirror a dirty Android build")
    artifact = candidate.get("artifact")
    if not isinstance(artifact, dict):
        raise MirrorError("Android candidate.json is missing artifact metadata")
    filename = _validate_filename(artifact.get("name"))
    if not filename.endswith(".apk"):
        raise MirrorError(f"Android artifact must be an .apk file, got {filename!r}")
    sha256 = _validate_sha256(artifact.get("sha256"), f"{filename} sha256")
    size = _validate_positive_size(artifact.get("bytes"), f"{filename} size")
    _cross_check_release_asset(assets_by_name, filename, size, sha256)
    prefix = f"{public_url}/artifacts/android/{version}/build.{build_number}/"
    return [
        {
            "filename": filename,
            "size": size,
            "sha256": sha256,
            "url": prefix + quote(filename, safe=""),
        }
    ]


def write_github_outputs(path: Path, values: dict[str, str]) -> None:
    lines = []
    for key, value in values.items():
        if "\n" in value or "\r" in value:
            raise MirrorError(f"GitHub output {key!r} contains a newline")
        lines.append(f"{key}={value}\n")
    with path.open("a", encoding="utf-8") as stream:
        stream.writelines(lines)


def prepare(args: argparse.Namespace) -> dict:
    product, version, build_number = parse_release_tag(args.tag_name)
    if args.product and args.product != product:
        raise MirrorError(
            f"requested product {args.product!r} does not match tag {args.tag_name!r} (expected {product!r})"
        )
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    if not token:
        raise MirrorError("GH_TOKEN (or GITHUB_TOKEN) must be set")

    api_base = validate_base_url(
        args.api_base, "GitHub API base", allow_local=args.allow_local_http
    )
    public_url = validate_base_url(
        args.public, "public release URL", allow_local=args.allow_local_http
    )

    release_url = f"{api_base}/repos/{args.repo}/releases/tags/{quote(args.tag_name, safe='')}"
    release = json.loads(_github_request(release_url, token).decode("utf-8"))
    if release.get("draft") is not False:
        raise MirrorError(
            f"release {args.tag_name} is still a draft; refusing to mirror an unpublished release"
        )
    if release.get("tag_name") != args.tag_name:
        raise MirrorError(
            f"GitHub release tag_name {release.get('tag_name')!r} does not match {args.tag_name!r}"
        )
    raw_assets = release.get("assets")
    if not isinstance(raw_assets, list):
        raise MirrorError(f"GitHub release {args.tag_name} returned no assets list")
    assets_by_name: dict[str, dict] = {}
    for asset in raw_assets:
        name = asset.get("name")
        if isinstance(name, str):
            if name in assets_by_name:
                raise MirrorError(f"duplicate asset {name!r} in GitHub release {args.tag_name}")
            assets_by_name[name] = asset

    if product == "desktop":
        manifest = _download_release_asset_json(
            api_base, args.repo, assets_by_name, "release-manifest.json", token
        )
        artifacts = _extract_desktop_artifacts(
            manifest, assets_by_name, version, build_number, public_url
        )
    else:
        candidate = _download_release_asset_json(
            api_base, args.repo, assets_by_name, "candidate.json", token
        )
        artifacts = _extract_android_artifacts(
            candidate, assets_by_name, version, build_number, public_url
        )

    gitcode_tag = to_gitcode_tag(product, version, build_number)
    prerelease = bool(release.get("prerelease"))
    plan = {
        "schema": 1,
        "product": product,
        "version": version,
        "buildNumber": str(build_number),
        "tagName": args.tag_name,
        "gitcodeTag": gitcode_tag,
        "prerelease": prerelease,
        "artifacts": artifacts,
    }
    output_path = Path(args.output).resolve()
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(plan, indent=2) + "\n", encoding="utf-8")

    sources = [{"url": a["url"], "sha256": a["sha256"]} for a in artifacts]
    if args.github_output:
        write_github_outputs(
            Path(args.github_output),
            {
                "product": product,
                "version": version,
                "build_number": str(build_number),
                "gitcode_tag": gitcode_tag,
                "prerelease": "true" if prerelease else "false",
                "sources": json.dumps(sources, separators=(",", ":")),
                "artifact_count": str(len(artifacts)),
            },
        )
    print(
        f"Prepared GitCode mirror plan for {args.tag_name} -> {gitcode_tag} "
        f"({product}, prerelease={prerelease}, {len(artifacts)} artifact(s))"
    )
    return plan


def _is_permanent_http(status: int) -> bool:
    return 400 <= status < 500 and status not in (408, 429)


def _put_mirror(
    *,
    admin_url: str,
    access_id: str,
    access_secret: str,
    body: dict,
    attempts: int,
    retry_delay_ms: int,
    api_timeout_ms: int,
) -> dict:
    payload = json.dumps(body).encode("utf-8")
    url = f"{admin_url}/api/mirrors"
    last_error: Exception | None = None
    for attempt in range(1, attempts + 1):
        req = Request(
            url,
            data=payload,
            method="PUT",
            headers={
                "Content-Type": "application/json",
                "Accept": "application/json",
                "CF-Access-Client-Id": access_id,
                "CF-Access-Client-Secret": access_secret,
                "User-Agent": "UniClipboard-Release",
            },
        )
        try:
            with _NO_REDIRECT_OPENER.open(req, timeout=api_timeout_ms / 1000.0) as response:
                text = response.read((1 << 20) + 1).decode("utf-8", errors="replace")
            try:
                parsed = json.loads(text)
            except json.JSONDecodeError as error:
                raise MirrorError(
                    f"FlareRelease returned non-JSON response: {text[:300]}",
                    permanent=True,
                ) from error
            if (parsed.get("data") or {}).get("state") != "ready":
                raise MirrorError(
                    f"FlareRelease did not mark the mirror ready: {text[:300]}",
                    permanent=True,
                )
            return parsed
        except HTTPError as error:
            body_text = error.read(4096).decode("utf-8", errors="replace")
            err = MirrorError(
                f"FlareRelease returned HTTP {error.code}: {body_text[:300]}",
                permanent=_is_permanent_http(error.code),
            )
            last_error = err
        except URLError as error:
            last_error = MirrorError(f"FlareRelease request failed: {error.reason}")
        except MirrorError as error:
            last_error = error
        except Exception as error:  # noqa: BLE001
            last_error = MirrorError(str(error))

        if getattr(last_error, "permanent", False) or attempt == attempts:
            break
        if retry_delay_ms > 0:
            time.sleep((retry_delay_ms * attempt) / 1000.0)
    assert last_error is not None
    raise last_error


def register_mirrors(
    *,
    product: str,
    version: str,
    build_number: str,
    artifacts: list[dict],
    receipt_files: list[dict],
    source: str,
    admin_url: str,
    access_id: str,
    access_secret: str,
    allow_local: bool,
    attempts: int,
    retry_delay_ms: int,
    api_timeout_ms: int,
) -> list[dict]:
    receipt_by_name = {
        file["name"]: file
        for file in receipt_files
        if isinstance(file, dict) and isinstance(file.get("name"), str)
    }
    records: list[dict] = []
    for artifact in artifacts:
        filename = artifact["filename"]
        entry = receipt_by_name.get(filename)
        if not entry:
            records.append(
                {
                    "filename": filename,
                    "status": "failed",
                    "error": "the receipt has no entry for this file",
                }
            )
            continue
        status = entry.get("status")
        if status not in ("mirrored", "reused"):
            stage = f" at {entry['stage']}" if entry.get("stage") else ""
            detail = f": {entry['error']}" if entry.get("error") else ""
            records.append(
                {
                    "filename": filename,
                    "status": "failed",
                    "error": f"not registered: the mirror step reported {status}{stage}{detail}",
                }
            )
            continue
        entry_sha = entry.get("sha256")
        if not isinstance(entry_sha, str) or not SHA256_RE.fullmatch(entry_sha) or entry_sha != artifact["sha256"]:
            records.append(
                {
                    "filename": filename,
                    "status": "failed",
                    "error": "not registered: the mirrored digest differs from the registered artifact",
                }
            )
            continue
        entry_size = entry.get("size")
        if type(entry_size) is not int or entry_size <= 0 or entry_size != artifact["size"]:
            records.append(
                {
                    "filename": filename,
                    "status": "failed",
                    "error": "not registered: the mirrored size differs from the registered artifact",
                }
            )
            continue
        download_url = entry.get("downloadUrl")
        if not is_registrable_mirror_url(download_url, allow_local=allow_local):
            records.append(
                {
                    "filename": filename,
                    "status": "failed",
                    "error": "not registered: the download address is not a plain https GitCode/AtomGit address",
                }
            )
            continue
        try:
            _put_mirror(
                admin_url=admin_url,
                access_id=access_id,
                access_secret=access_secret,
                body={
                    "product": product,
                    "version": version,
                    "buildNumber": build_number,
                    "filename": filename,
                    "provider": "gitcode",
                    "downloadUrl": download_url,
                    "size": entry_size,
                    "sha256": entry_sha,
                    "source": source,
                },
                attempts=attempts,
                retry_delay_ms=retry_delay_ms,
                api_timeout_ms=api_timeout_ms,
            )
            records.append(
                {
                    "filename": filename,
                    "status": "registered",
                    "mirror": status,
                    "downloadUrl": download_url,
                    "size": entry_size,
                    "sha256": entry_sha,
                }
            )
        except Exception as error:  # noqa: BLE001
            records.append(
                {
                    "filename": filename,
                    "status": "failed",
                    "error": f"registering with FlareRelease failed: {error}",
                }
            )
    return records


def _read_json(path_str: str, label: str) -> dict:
    try:
        parsed = json.loads(Path(path_str).resolve().read_text(encoding="utf-8"))
    except Exception as error:  # noqa: BLE001
        raise MirrorError(f"cannot read {label} {path_str}: {error}") from error
    if not isinstance(parsed, dict):
        raise MirrorError(f"{label} {path_str} must be a JSON object")
    return parsed


def _write_registration_outputs(
    product: str,
    version: str,
    build_number: str,
    records: list[dict],
    provenance_path: str | None,
    secrets: list[str | None],
) -> None:
    text = redact(json.dumps(records, indent=2) + "\n", secrets)
    if provenance_path:
        target = Path(provenance_path).resolve()
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary_path:
        lines = [
            f"### FlareRelease mirror registration ({product} {version} build {build_number})",
            "",
        ]
        for record in records:
            lines.append(f"- `{record['filename']}`: **{record['status']}**")
            if record.get("error"):
                lines.append(f"  - {record['error']}")
        with Path(summary_path).open("a", encoding="utf-8") as stream:
            stream.write(redact("\n".join(lines), secrets) + "\n")


def register(args: argparse.Namespace) -> list[dict]:
    receipt = _read_json(args.receipt, "receipt")
    plan = _read_json(args.plan, "mirror plan")
    receipt_files = receipt.get("files")
    if not isinstance(receipt_files, list) or len(receipt_files) == 0:
        raise MirrorError("the receipt lists no files")
    product = plan.get("product")
    version = plan.get("version")
    build_number = plan.get("buildNumber")
    artifacts = plan.get("artifacts")
    if (
        product not in ("desktop", "android")
        or not isinstance(version, str)
        or not version
        or not isinstance(build_number, str)
        or not re.fullmatch(r"[1-9][0-9]{0,8}", build_number)
        or not isinstance(artifacts, list)
        or len(artifacts) == 0
    ):
        raise MirrorError("the mirror plan is missing product, version, buildNumber, or artifacts")

    access_id = os.environ.get("FLARE_RELEASE_ACCESS_CLIENT_ID", "")
    access_secret = os.environ.get("FLARE_RELEASE_ACCESS_CLIENT_SECRET", "")
    missing = [
        name
        for name, value in (
            ("FLARE_RELEASE_ACCESS_CLIENT_ID", access_id),
            ("FLARE_RELEASE_ACCESS_CLIENT_SECRET", access_secret),
        )
        if not value
    ]
    if missing:
        raise MirrorError(f"FlareRelease is not configured; missing {', '.join(missing)}")

    admin_raw = os.environ.get("FLARE_RELEASE_ADMIN_URL") or DEFAULT_ADMIN_URL
    admin_url = validate_base_url(
        admin_raw, "FLARE_RELEASE_ADMIN_URL", allow_local=args.allow_local_http
    )

    if args.attempts < 1:
        raise MirrorError("--attempts must be at least 1")
    if args.retry_delay_ms < 0:
        raise MirrorError("--retry-delay-ms must be non-negative")
    if args.api_timeout_ms <= 0:
        raise MirrorError("--api-timeout-ms must be positive")

    secrets = [access_secret, access_id]
    records = register_mirrors(
        product=product,
        version=version,
        build_number=build_number,
        artifacts=artifacts,
        receipt_files=receipt_files,
        source=args.source,
        admin_url=admin_url,
        access_id=access_id,
        access_secret=access_secret,
        allow_local=args.allow_local_http,
        attempts=args.attempts,
        retry_delay_ms=args.retry_delay_ms,
        api_timeout_ms=args.api_timeout_ms,
    )
    _write_registration_outputs(
        product, version, build_number, records, args.provenance, secrets
    )

    failed = 0
    for record in records:
        if record["status"] == "registered":
            print(
                f"Registered {record['filename']} ({record['mirror']}) -> {record['downloadUrl']}"
            )
        else:
            failed += 1
            reason = redact(record.get("error") or "unknown error", secrets)
            print(
                f"::warning title=FlareRelease mirror not registered::{record['filename']}: {reason}"
            )
    if failed > 0:
        raise MirrorError(f"{failed} mirror registration(s) failed")
    return records


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    prep = sub.add_parser("prepare", help="Validate a published release and build mirror sources")
    prep.add_argument("--repo", default=DEFAULT_REPO)
    prep.add_argument("--tag-name", required=True)
    prep.add_argument("--product", choices=("desktop", "android"), default="")
    prep.add_argument("--public", default=DEFAULT_PUBLIC_URL)
    prep.add_argument("--api-base", default=DEFAULT_GITHUB_API)
    prep.add_argument("--output", required=True)
    prep.add_argument("--github-output", default="")
    prep.add_argument("--allow-local-http", action="store_true")

    reg = sub.add_parser("register", help="Register verified GitCode mirrors with FlareRelease")
    reg.add_argument("--receipt", required=True)
    reg.add_argument("--plan", required=True)
    reg.add_argument("--source", default="github-actions")
    reg.add_argument("--provenance", default="")
    reg.add_argument("--attempts", type=int, default=3)
    reg.add_argument("--retry-delay-ms", type=int, default=5000)
    reg.add_argument("--api-timeout-ms", type=int, default=60_000)
    reg.add_argument("--allow-local-http", action="store_true")

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        if args.command == "prepare":
            prepare(args)
        else:
            register(args)
        return 0
    except MirrorError as error:
        secrets = [
            os.environ.get("GH_TOKEN"),
            os.environ.get("GITHUB_TOKEN"),
            os.environ.get("FLARE_RELEASE_ACCESS_CLIENT_ID"),
            os.environ.get("FLARE_RELEASE_ACCESS_CLIENT_SECRET"),
        ]
        print(
            f"gitcode_mirror {args.command} failed: {redact(str(error), secrets)}",
            file=sys.stderr,
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
