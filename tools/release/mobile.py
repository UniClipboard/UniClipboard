#!/usr/bin/env python3
"""Resolve mobile release identity and record native candidate artifacts."""

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET
import zipfile

from release import ROOT, digest, metadata, save


METADATA = ROOT / "release/products.toml"
ALLOWED_TARGETS = {
    "android": {"android-apk-arm64", "android-aab"},
    "ios": {"ios-arm64"},
}
REQUIRED_TARGETS = {
    "android": {"android-apk-arm64"},
    "ios": {"ios-arm64"},
}
PRODUCTS = tuple(ALLOWED_TARGETS)


def mobile_metadata(platform, metadata_path=METADATA):
    product = metadata(metadata_path, platform)
    app_id = product["application_id"]
    if not re.fullmatch(r"[A-Za-z][A-Za-z0-9]*(?:\.[A-Za-z][A-Za-z0-9]*)+", app_id):
        raise ValueError("invalid mobile application identifier")
    targets = set(product["targets"])
    if not targets or not targets <= ALLOWED_TARGETS[platform] or not REQUIRED_TARGETS[platform] <= targets:
        raise ValueError("invalid mobile release targets")
    if not 0 <= product["legacy_build_floor"] < product["build_number"]:
        raise ValueError("mobile build number must advance beyond the legacy build floor")
    return product


def _verify_android_apk(artifact, entries, product, aapt2, apksigner, architecture):
    if "AndroidManifest.xml" not in entries:
        raise ValueError("Android APK candidate is missing AndroidManifest.xml")
    if not aapt2:
        raise ValueError("Android APK verification requires --aapt2")
    abis = sorted({
        parts[1]
        for name in entries
        if name.startswith("lib/") and len((parts := name.split("/"))) >= 3 and parts[2].endswith(".so")
    })
    if architecture and abis != [architecture]:
        raise ValueError(f"Android APK ABIs {abis!r} do not match expected [{architecture!r}]")
    badging = subprocess.check_output([aapt2, "dump", "badging", str(artifact)], text=True)
    match = re.search(
        r"^package:\s+name='([^']+)'\s+versionCode='([^']+)'\s+versionName='([^']+)'",
        badging,
        re.M,
    )
    if not match:
        raise ValueError("failed to parse Android APK package badging")
    if match.groups() != (product["application_id"], str(product["build_number"]), product["version"]):
        raise ValueError("APK identity differs from release metadata")
    if apksigner:
        output = subprocess.check_output(
            [apksigner, "verify", "--verbose", str(artifact)],
            text=True,
            stderr=subprocess.STDOUT,
        )
        if not (
            "Verified using v2 scheme (APK Signature Scheme v2): true" in output
            or "Verified using v3 scheme (APK Signature Scheme v3): true" in output
        ):
            raise ValueError("APK signature must include v2 or v3 scheme")


def _verify_android_aab(artifact, entries, product, bundletool):
    if not {"BundleConfig.pb", "base/manifest/AndroidManifest.xml"} <= entries:
        raise ValueError("Android candidate must be an app bundle")
    if not bundletool:
        raise ValueError("Android AAB verification requires --bundletool")
    manifest = ET.fromstring(subprocess.check_output([
        "java", "-jar", bundletool, "dump", "manifest", f"--bundle={artifact}", "--module=base",
    ]))
    ns = "{http://schemas.android.com/apk/res/android}"
    if (manifest.get("package"), manifest.get(ns + "versionName"), manifest.get(ns + "versionCode")) != (
        product["application_id"], product["version"], str(product["build_number"])
    ):
        raise ValueError("app bundle identity differs from release metadata")


def _verify_ios_ipa(artifact, archive, entries, product):
    from plistlib import loads

    names = [n for n in entries if re.fullmatch(r"Payload/[^/]+\.app/Info.plist", n)]
    if len(names) != 1:
        raise ValueError("iOS candidate must contain one application")
    info = loads(archive.read(names[0]))
    if (info.get("CFBundleIdentifier"), info.get("CFBundleShortVersionString"), str(info.get("CFBundleVersion"))) != (
        product["application_id"], product["version"], str(product["build_number"])
    ):
        raise ValueError("exported app identity differs from release metadata")


def candidate(
    platform,
    artifact,
    output,
    bundletool=None,
    aapt2=None,
    apksigner=None,
    architecture=None,
    metadata_path=METADATA,
    published_at=None,
    source_sha=None,
    fixture=False,
):
    product = mobile_metadata(platform, metadata_path)
    artifact = Path(artifact)
    if not artifact.is_file():
        raise ValueError("candidate artifact file does not exist")
    if platform == "ios":
        if artifact.suffix != ".ipa":
            raise ValueError("expected a native .ipa file")
        fmt = "ipa"
        arch = architecture or "arm64"
    else:
        if artifact.suffix == ".apk":
            fmt = "apk"
            arch = architecture or "arm64-v8a"
        elif artifact.suffix == ".aab":
            fmt = "aab"
            arch = architecture or "universal"
        else:
            raise ValueError("expected a native .apk or .aab file")
    with zipfile.ZipFile(artifact) as archive:
        if archive.testzip() is not None:
            raise ValueError("candidate archive failed its CRC check")
        entries = set(archive.namelist())
        if platform == "android":
            if fmt == "apk":
                _verify_android_apk(artifact, entries, product, aapt2, apksigner, arch)
            else:
                _verify_android_aab(artifact, entries, product, bundletool)
        else:
            _verify_ios_ipa(artifact, archive, entries, product)
    source_clean = True
    if fixture:
        revision = source_sha or subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
        source_clean = not bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip())
    else:
        if subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip():
            raise ValueError("candidate receipt requires a clean source checkout")
        revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    if not re.fullmatch(r"[a-f0-9]{40}", revision):
        raise ValueError("invalid candidate source SHA")
    timestamp = published_at or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    save(output, {
        "schema": 1,
        "platform": platform,
        "product": platform,
        "format": fmt,
        "architecture": arch,
        "source_sha": revision,
        "source_clean": source_clean,
        "published_at": timestamp,
        "application_id": product["application_id"],
        "version": product["version"],
        "build_number": product["build_number"],
        "artifact": {"name": artifact.name, "bytes": artifact.stat().st_size, "sha256": digest(artifact)},
    })


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    sub = cli.add_subparsers(dest="command", required=True)
    cfg = sub.add_parser("config")
    cfg.add_argument("--metadata", type=Path, default=METADATA)
    record = sub.add_parser("record-candidate")
    record.add_argument("platform", choices=PRODUCTS)
    record.add_argument("artifact")
    record.add_argument("output")
    record.add_argument("--bundletool")
    record.add_argument("--aapt2")
    record.add_argument("--apksigner")
    record.add_argument("--architecture")
    record.add_argument("--metadata", type=Path, default=METADATA)
    record.add_argument("--published-at")
    record.add_argument("--source-sha")
    record.add_argument("--fixture", action="store_true")
    args = cli.parse_args()
    if args.command == "config":
        print(json.dumps({platform: mobile_metadata(platform, args.metadata) for platform in PRODUCTS}))
    else:
        candidate(
            args.platform,
            args.artifact,
            args.output,
            bundletool=args.bundletool,
            aapt2=args.aapt2,
            apksigner=args.apksigner,
            architecture=args.architecture,
            metadata_path=args.metadata,
            published_at=args.published_at,
            source_sha=args.source_sha,
            fixture=args.fixture,
        )


if __name__ == "__main__":
    main()
