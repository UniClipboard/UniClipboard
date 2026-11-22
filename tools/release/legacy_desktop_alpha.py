"""Prepare a legacy alpha offer from exact release assets and v1 signer receipts."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import urllib.request

TARGETS = {
    "darwin-aarch64": "darwin-arm64.app.tar.gz",
    "darwin-x86_64": "darwin-amd64.app.tar.gz",
    "linux-aarch64": "linux-arm64.AppImage.tar.gz",
    "linux-x86_64": "linux-amd64.AppImage.tar.gz",
    "windows-aarch64": "windows-arm64-setup.exe",
    "windows-x86_64": "windows-amd64-setup.exe",
}
OLD_SOURCE = "0a72be0fc1532824b72d8d3828462c205ad348f5"
REPOSITORY = "UniClipboard/UniClipboard"
PUBLIC = "https://release.uniclipboard.app"


def digest(path):
    with path.open("rb") as data:
        return hashlib.file_digest(data, "sha256").hexdigest()


def public_json(path):
    with urllib.request.urlopen(PUBLIC + path, timeout=30) as response:
        return json.load(response)


def prepare(build, legacy_source, output):
    if not re.fullmatch(r"[1-9][0-9]{0,8}", build):
        raise ValueError("invalid Desktop build number")
    commit = subprocess.check_output(
        ["git", "-C", legacy_source, "rev-parse", "HEAD"], text=True
    ).strip()
    if commit != OLD_SOURCE:
        raise ValueError("unexpected v1 updater source")
    selected = public_json("/v1/desktop/alpha.json")
    version = selected.get("version", "")
    registration_digest = selected.get("manifestDigest", "")
    if (
        not re.fullmatch(r"2\.0\.(?:0|[1-9][0-9]*)", version)
        or selected.get("buildNumber") != build
        or not re.fullmatch(r"[a-f0-9]{64}", registration_digest)
    ):
        raise ValueError("numbered alpha is not the requested release")
    tag = f"desktop/v{version}-build.{build}"
    release = json.loads(subprocess.check_output(
        ["gh", "release", "view", tag, "--repo", REPOSITORY,
         "--json", "isDraft,isPrerelease,tagName"], text=True
    ))
    if release != {"isDraft": False, "isPrerelease": True, "tagName": tag}:
        raise ValueError("expected an exact published GitHub prerelease")
    output.mkdir(parents=True, exist_ok=False)
    assets = output / "assets"
    assets.mkdir()
    filenames = {
        platform: f"uniclipboard-{version}-build.{build}-{suffix}"
        for platform, suffix in TARGETS.items()
    }
    subprocess.run(
        ["gh", "release", "download", tag, "--repo", REPOSITORY,
         "--dir", output, "--pattern", "release-manifest.json"]
        + [arg for name in filenames.values() for arg in ("--pattern", name)],
        check=True,
    )
    catalogue = json.loads((output / "release-manifest.json").read_text())
    if (
        catalogue["version"] != version
        or catalogue["build_number"] != int(build)
        or catalogue["product"] != "desktop"
        or catalogue["development"]
        or not catalogue["source_clean"]
    ):
        raise ValueError("unexpected GitHub release catalogue")
    records = {record["file"]: record for record in catalogue["artifacts"]}
    for name in filenames.values():
        path = output / name
        record = records[name]
        if path.stat().st_size != record["size"] or digest(path) != record["sha256"]:
            raise ValueError(f"release payload does not match catalogue: {name}")
        if "-darwin-" in name and not (record["payload_signed"] and record["notarized"]):
            raise ValueError(f"macOS payload requires signing and notarization: {name}")
        # The pinned legacy signer expects the historical capitalized prefix.
        path.rename(assets / ("UniClipboard" + name.removeprefix("uniclipboard")))
    verification = output / "v1-signature-verification.json"
    subprocess.run(
        ["go", "run", "./cmd/updater-sign", "--artifacts-dir", assets,
         "--evidence", verification],
        cwd=legacy_source / "apps/gui-go",
        check=True,
    )
    signed = {
        item["filename"]: item for item in
        json.loads(verification.read_text())["artifacts"]
    }
    signatures = {}
    for platform, name in filenames.items():
        legacy_name = "UniClipboard" + name.removeprefix("uniclipboard")
        record = signed[legacy_name]
        sidecar = assets / (legacy_name + ".sig")
        if (
            not record["verifiedByClient"]
            or record["sha256"] != records[name]["sha256"]
            or record["signatureSha256"] != digest(sidecar)
        ):
            raise ValueError(f"legacy verifier did not accept {name}")
        signatures[platform] = sidecar.read_text().strip()
    record = {
        "version": version,
        "buildNumber": build,
        "manifestDigest": registration_digest,
        "signatures": signatures,
    }
    (output / "desktop-legacy-alpha.json").write_text(
        json.dumps(record, indent=2) + "\n"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build", required=True)
    parser.add_argument("--legacy-source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    prepare(args.build, args.legacy_source.resolve(), args.output.resolve())


if __name__ == "__main__":
    main()
