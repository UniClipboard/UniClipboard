#!/usr/bin/env python3
"""Fetch and validate iOS production App Store profiles and ASC build identity."""

import argparse
import base64
import json
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
from urllib.parse import quote
from urllib.request import Request, urlopen

from mobile import METADATA, mobile_metadata

ASC_API = "https://api.appstoreconnect.apple.com"
PREFERRED_PROFILE_NAMES = {
    "": "UniClipboard App Store",
    ".Share": "UniClipboard Share App Store",
    ".Keyboard": "UniClipboard Keyboard App Store",
}


def decode_private_key(raw: str) -> bytes:
    stripped = raw.strip()
    if stripped.startswith("-----BEGIN "):
        data = (stripped + "\n").encode("utf-8")
    else:
        data = base64.b64decode(stripped)
    if b"-----BEGIN PRIVATE KEY-----" not in data:
        raise ValueError("ASC private key must be a PKCS#8 PEM or base64-encoded PEM")
    return data


def make_asc_jwt(key_id: str, issuer_id: str, key_path: Path) -> str:
    ruby_script = r"""
require 'openssl'
require 'json'
require 'base64'
key_id, issuer_id, key_path = ARGV
key = OpenSSL::PKey.read(File.read(key_path))
b64 = ->(d) { Base64.urlsafe_encode64(d).delete('=') }
header = b64.({ alg: 'ES256', kid: key_id, typ: 'JWT' }.to_json)
now = Time.now.to_i
payload = b64.({ iss: issuer_id, iat: now, exp: now + 1200, aud: 'appstoreconnect-v1' }.to_json)
input = "#{header}.#{payload}"
der = key.sign(OpenSSL::Digest::SHA256.new, input)
r, s = OpenSSL::ASN1.decode(der).value.map { |i| i.value.to_s(2) }
raw = [r, s].map { |c| c.length > 32 ? c[-32..-1] : c.rjust(32, "\x00") }.join
print "#{input}.#{b64.(raw)}"
"""
    return subprocess.check_output(
        ["ruby", "-e", ruby_script, key_id, issuer_id, str(key_path)],
        text=True,
    ).strip()


def asc_get(base: str, path: str, token: str) -> dict:
    req = Request(
        base.rstrip("/") + path,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/json",
            "User-Agent": "UniClipboard-Release",
        },
    )
    with urlopen(req, timeout=60) as response:
        data = response.read((8 << 20) + 1)
    if len(data) > 8 << 20:
        raise ValueError("App Store Connect response exceeds size limit")
    return json.loads(data)


def decode_mobileprovision(path: Path) -> dict:
    raw = subprocess.check_output(["security", "cms", "-D", "-i", str(path)])
    return plistlib.loads(raw)


def validate_profile_plist(profile: dict, app: str, bundle: str, team: str | None) -> tuple[str, str]:
    group = f"group.{app}.incoming-share"
    legacy_group = f"group.{app}"
    (identifier,) = profile["TeamIdentifier"]
    if team is not None and identifier != team:
        raise ValueError("iOS production profiles must use the same team")
    entitlements = profile["Entitlements"]
    if entitlements["application-identifier"] != identifier + "." + bundle:
        raise ValueError(f"iOS production profile application identifier mismatch for {bundle}")
    groups = entitlements.get("com.apple.security.application-groups", [])
    if group not in groups:
        raise ValueError(f"iOS production share group ({group}) is missing from profile for {bundle}")
    if bundle == app and legacy_group not in groups:
        raise ValueError(f"iOS production legacy history App Group ({legacy_group}) is missing from app profile")
    if entitlements.get("get-task-allow") or profile.get("ProvisionedDevices") or profile.get("ProvisionsAllDevices"):
        raise ValueError(f"iOS archive export requires App Store distribution profile for {bundle}")
    return identifier, profile["Name"]


def install_profile_bytes(uuid: str, content: bytes, destinations: list[Path] | None = None) -> None:
    targets = destinations or [
        Path.home() / "Library/Developer/Xcode/UserData/Provisioning Profiles",
        Path.home() / "Library/MobileDevice/Provisioning Profiles",
    ]
    for directory in targets:
        directory.mkdir(parents=True, exist_ok=True)
        (directory / f"{uuid}.mobileprovision").write_bytes(content)


def validate_local_files(paths: list[str], metadata_path: Path = METADATA, destinations: list[Path] | None = None) -> dict:
    app = mobile_metadata("ios", metadata_path)["application_id"]
    bundles = (app, app + ".Share", app + ".Keyboard")
    if len(paths) != len(bundles):
        raise ValueError("expected three mobileprovision paths (app, share, keyboard)")
    profiles = {}
    team = None
    for bundle, raw_path in zip(bundles, paths, strict=True):
        path = Path(raw_path)
        profile = decode_mobileprovision(path)
        team, name = validate_profile_plist(profile, app, bundle, team)
        profiles[bundle] = name
        install_profile_bytes(profile["UUID"], path.read_bytes(), destinations)
    return {"team": team, "profiles": profiles}


def fetch_asc_profiles(
    key_id: str,
    issuer_id: str,
    key_path: Path,
    api_base: str = ASC_API,
    metadata_path: Path = METADATA,
    destinations: list[Path] | None = None,
    decode_fn=decode_mobileprovision,
) -> dict:
    app = mobile_metadata("ios", metadata_path)["application_id"]
    bundles = (app, app + ".Share", app + ".Keyboard")
    token = make_asc_jwt(key_id, issuer_id, key_path)
    payload = asc_get(
        api_base,
        "/v1/profiles?filter[profileType]=IOS_APP_STORE&filter[profileState]=ACTIVE&include=bundleId&limit=200",
        token,
    )
    bundle_ids_by_resource = {
        item["id"]: item["attributes"]["identifier"]
        for item in payload.get("included", [])
        if item.get("type") == "bundleIds"
    }
    candidates_by_bundle: dict[str, list[dict]] = {bundle: [] for bundle in bundles}
    for item in payload.get("data", []):
        rel = (item.get("relationships") or {}).get("bundleId", {}).get("data") or {}
        identifier = bundle_ids_by_resource.get(rel.get("id", ""))
        if identifier in candidates_by_bundle:
            candidates_by_bundle[identifier].append(item)

    profiles = {}
    team = None
    with tempfile.TemporaryDirectory(prefix="unic-ios-profiles-") as temporary:
        temp_dir = Path(temporary)
        for bundle in bundles:
            suffix = bundle[len(app):]
            preferred_name = PREFERRED_PROFILE_NAMES.get(suffix, "")
            items = candidates_by_bundle[bundle]
            if not items:
                raise ValueError(f"no active IOS_APP_STORE provisioning profile found in ASC for {bundle}")
            items.sort(
                key=lambda item: (
                    item.get("attributes", {}).get("name") == preferred_name,
                    item.get("attributes", {}).get("expirationDate", ""),
                ),
                reverse=True,
            )
            chosen = None
            last_error = None
            for index, item in enumerate(items):
                encoded = (item.get("attributes") or {}).get("profileContent")
                if not encoded:
                    continue
                content = base64.b64decode(encoded)
                probe = temp_dir / f"{bundle}-{index}.mobileprovision"
                probe.write_bytes(content)
                try:
                    plist = decode_fn(probe)
                    next_team, name = validate_profile_plist(plist, app, bundle, team)
                except Exception as exc:  # noqa: BLE001 - try remaining active profiles
                    last_error = exc
                    continue
                chosen = (next_team, name, plist["UUID"], content)
                break
            if chosen is None:
                raise ValueError(
                    f"no active ASC profile for {bundle} satisfied production entitlements"
                ) from last_error
            team, name, uuid, content = chosen
            profiles[bundle] = name
            install_profile_bytes(uuid, content, destinations)
    return {"team": team, "profiles": profiles}


def check_asc_build(
    key_id: str,
    issuer_id: str,
    key_path: Path,
    api_base: str = ASC_API,
    metadata_path: Path = METADATA,
) -> dict:
    product = mobile_metadata("ios", metadata_path)
    app_id = product["application_id"]
    build_number = str(product["build_number"])
    token = make_asc_jwt(key_id, issuer_id, key_path)
    apps = asc_get(api_base, f"/v1/apps?filter[bundleId]={quote(app_id, safe='')}&limit=10", token).get("data", [])
    app_record = next(
        (item for item in apps if (item.get("attributes") or {}).get("bundleId") == app_id),
        None,
    )
    if not app_record:
        raise ValueError(f"App Store Connect app not found for bundle ID {app_id}")
    builds = asc_get(
        api_base,
        f"/v1/builds?filter[app]={quote(app_record['id'], safe='')}&filter[version]={quote(build_number, safe='')}&limit=5",
        token,
    ).get("data", [])
    if builds:
        raise ValueError(
            f"iOS build number {build_number} already exists in App Store Connect for {app_id}; bump build_number in release/products.toml"
        )
    return {"app_id": app_record["id"], "bundle_id": app_id, "build_number": product["build_number"], "available": True}


def main():
    # Preserve backwards-compatible positional invocation:
    #   ios_profiles.py <app.mobileprovision> <share.mobileprovision> <keyboard.mobileprovision>
    if len(sys.argv) == 4 and not sys.argv[1].startswith("-") and sys.argv[1] not in {"validate", "fetch", "check-build", "decode-key"}:
        print(json.dumps(validate_local_files(sys.argv[1:])))
        return

    cli = argparse.ArgumentParser(description=__doc__)
    sub = cli.add_subparsers(dest="command", required=True)

    val = sub.add_parser("validate")
    val.add_argument("profiles", nargs=3)
    val.add_argument("--metadata", type=Path, default=METADATA)

    dec = sub.add_parser("decode-key")
    dec.add_argument("--env", default="ASC_PRIVATE_KEY")
    dec.add_argument("--output", type=Path, required=True)

    for name in ("fetch", "check-build"):
        cmd = sub.add_parser(name)
        cmd.add_argument("--key-id", required=True)
        cmd.add_argument("--issuer-id", required=True)
        cmd.add_argument("--key-path", type=Path, required=True)
        cmd.add_argument("--api-base", default=ASC_API)
        cmd.add_argument("--metadata", type=Path, default=METADATA)

    args = cli.parse_args()
    if args.command == "validate":
        print(json.dumps(validate_local_files(args.profiles, args.metadata)))
    elif args.command == "decode-key":
        raw = os.environ.get(args.env, "")
        if not raw.strip():
            raise ValueError(f"environment variable {args.env} is empty")
        data = decode_private_key(raw)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(data)
        os.chmod(args.output, 0o600)
    elif args.command == "fetch":
        print(json.dumps(fetch_asc_profiles(args.key_id, args.issuer_id, args.key_path, args.api_base, args.metadata)))
    elif args.command == "check-build":
        print(json.dumps(check_asc_build(args.key_id, args.issuer_id, args.key_path, args.api_base, args.metadata)))


if __name__ == "__main__":
    main()
