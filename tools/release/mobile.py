#!/usr/bin/env python3
"""Resolve mobile release identity and record native candidate artifacts."""

import argparse
import json
from pathlib import Path
import re
import subprocess
import xml.etree.ElementTree as ET
import zipfile

from release import ROOT, digest, metadata, save


METADATA = ROOT / "release/products.toml"
PRODUCTS = {"android": "android-aab", "ios": "ios-arm64"}


def mobile_metadata(platform):
    product = metadata(METADATA, platform)
    app_id = product["application_id"]
    if not re.fullmatch(r"[A-Za-z][A-Za-z0-9]*(?:\.[A-Za-z][A-Za-z0-9]*)+", app_id):
        raise ValueError("invalid mobile application identifier")
    if product["targets"] != [PRODUCTS[platform]]:
        raise ValueError("invalid mobile release target")
    if not 0 <= product["legacy_build_floor"] < product["build_number"]:
        raise ValueError("mobile build number must advance beyond the legacy build floor")
    return product


def candidate(platform, artifact, output, bundletool):
    product = mobile_metadata(platform)
    artifact = Path(artifact)
    expected = ".ipa" if platform == "ios" else ".aab"
    if not artifact.is_file() or artifact.suffix != expected:
        raise ValueError(f"expected a native {expected} file")
    with zipfile.ZipFile(artifact) as archive:
        if archive.testzip() is not None:
            raise ValueError("candidate archive failed its CRC check")
        entries = set(archive.namelist())
        if platform == "android":
            if not {"BundleConfig.pb", "base/manifest/AndroidManifest.xml"} <= entries:
                raise ValueError("Android candidate must be an app bundle")
            if not bundletool:
                raise ValueError("Android candidate verification requires bundletool")
            manifest = ET.fromstring(subprocess.check_output([
                "java", "-jar", bundletool, "dump", "manifest", f"--bundle={artifact}", "--module=base",
            ]))
            ns = "{http://schemas.android.com/apk/res/android}"
            if (manifest.get("package"), manifest.get(ns + "versionName"), manifest.get(ns + "versionCode")) != (
                product["application_id"], product["version"], str(product["build_number"])
            ):
                raise ValueError("app bundle identity differs from release metadata")
        else:
            from plistlib import loads

            names = [n for n in entries if re.fullmatch(r"Payload/[^/]+\.app/Info.plist", n)]
            if len(names) != 1:
                raise ValueError("iOS candidate must contain one application")
            info = loads(archive.read(names[0]))
            if (info.get("CFBundleIdentifier"), info.get("CFBundleShortVersionString"), str(info.get("CFBundleVersion"))) != (
                product["application_id"], product["version"], str(product["build_number"])
            ):
                raise ValueError("exported app identity differs from release metadata")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip():
        raise ValueError("candidate receipt requires a clean source checkout")
    revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    save(output, {
        "schema": 1,
        "platform": platform,
        "source_sha": revision,
        "application_id": product["application_id"],
        "version": product["version"],
        "build_number": product["build_number"],
        "artifact": {"name": artifact.name, "bytes": artifact.stat().st_size, "sha256": digest(artifact)},
    })


def main():
    cli = argparse.ArgumentParser(description=__doc__)
    sub = cli.add_subparsers(dest="command", required=True)
    sub.add_parser("config")
    record = sub.add_parser("record-candidate")
    record.add_argument("platform", choices=PRODUCTS)
    record.add_argument("artifact")
    record.add_argument("output")
    record.add_argument("--bundletool")
    args = cli.parse_args()
    if args.command == "config":
        print(json.dumps({platform: mobile_metadata(platform) for platform in PRODUCTS}))
    else:
        candidate(args.platform, args.artifact, args.output, args.bundletool)


if __name__ == "__main__":
    main()
