#!/usr/bin/env python3
"""Native Apple signing prerequisites and notarization, without secret logging."""

import argparse
import base64
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile


def run(args):
    r = subprocess.run([str(x) for x in args], capture_output=True, text=True)
    if r.returncode:
        detail = ""
        if str(args[0]) == "xcrun":
            detail = (r.stderr + "\n" + r.stdout)[-1800:]
            for name in [
                "APPLE_ID",
                "APPLE_TEAM_ID",
                "APPLE_PASSWORD",
                "APPLE_APP_SPECIFIC_PASSWORD",
            ]:
                value = os.environ.get(name)
                if value:
                    detail = detail.replace(value, "[redacted]")
        raise RuntimeError(
            "native signing command failed: "
            + " ".join(map(str, args[:3]))
            + "\n"
            + detail
        )
    return r.stdout


def keychain(args):
    path = Path(args.path).resolve()
    if path.exists():
        raise ValueError("keychain already exists")
    certificate = os.environ.get("APPLE_CERTIFICATE", "")
    if not certificate:
        raise ValueError("APPLE_CERTIFICATE is required")
    password = secrets.token_urlsafe(32)
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="unic-certificate-") as temporary:
        cert = Path(temporary) / "certificate.p12"
        cert.write_bytes(base64.b64decode(certificate, validate=True))
        cert.chmod(0o600)
        try:
            run(["security", "create-keychain", "-p", password, path])
            run(["security", "set-keychain-settings", "-lut", "21600", path])
            run(["security", "unlock-keychain", "-p", password, path])
            run(
                [
                    "security",
                    "import",
                    cert,
                    "-k",
                    path,
                    "-P",
                    os.environ.get("APPLE_CERTIFICATE_PASSWORD", ""),
                    "-T",
                    "/usr/bin/codesign",
                ]
            )
            run(
                [
                    "security",
                    "set-key-partition-list",
                    "-S",
                    "apple-tool:,apple:,codesign:",
                    "-s",
                    "-k",
                    password,
                    path,
                ]
            )
            identities = run(
                ["security", "find-identity", "-v", "-p", "codesigning", path]
            )
            import re

            matches = re.findall(
                r'\b([A-F0-9]{40})\s+"Developer ID Application:', identities
            )
            if len(matches) != 1:
                raise ValueError(
                    "expected exactly one Developer ID Application identity"
                )
            # Explicit keychain search preserves access to Apple's system roots.
            previous = run(["security", "list-keychains", "-d", "user"])
            import shlex

            run(
                [
                    "security",
                    "list-keychains",
                    "-d",
                    "user",
                    "-s",
                    path,
                    *shlex.split(previous),
                ]
            )
            with Path(args.env_output).open("a") as env:
                env.write("UNIC_MACOS_SIGNING_IDENTITY=" + matches[0] + "\n")
                env.write("UNIC_SIGNING_KEYCHAIN=" + str(path) + "\n")
        except BaseException:
            if path.exists():
                run(["security", "delete-keychain", path])
            raise


def notarize(args):
    artifact = Path(args.file).resolve()
    if artifact.suffix not in [".zip", ".dmg", ".pkg"]:
        raise ValueError("Apple notarization requires ZIP, DMG or PKG")
    required = ["APPLE_ID", "APPLE_TEAM_ID"]
    if not all(os.environ.get(k) for k in required):
        raise ValueError("Apple notarization identity is missing")
    password = os.environ.get("APPLE_APP_SPECIFIC_PASSWORD") or os.environ.get(
        "APPLE_PASSWORD"
    )
    if not password:
        raise ValueError("Apple app-specific password is missing")
    profile = "unic-package-notary"
    run(
        [
            "xcrun",
            "notarytool",
            "store-credentials",
            profile,
            "--apple-id",
            os.environ["APPLE_ID"],
            "--team-id",
            os.environ["APPLE_TEAM_ID"],
            "--password",
            password,
            "--keychain",
            os.environ["UNIC_SIGNING_KEYCHAIN"],
        ]
    )
    result = json.loads(
        run(
            [
                "xcrun",
                "notarytool",
                "submit",
                artifact,
                "--keychain-profile",
                profile,
                "--keychain",
                os.environ["UNIC_SIGNING_KEYCHAIN"],
                "--wait",
                "--output-format",
                "json",
            ]
        )
    )
    if result.get("status") != "Accepted":
        raise ValueError("Apple notarization did not accept the artifact")
    if args.staple:
        run(["xcrun", "stapler", "staple", args.staple])
        run(["xcrun", "stapler", "validate", args.staple])
    Path(args.receipt).write_text(
        json.dumps(
            {
                "status": "passed",
                "notarization_id": result["id"],
                "artifact": artifact.name,
                "stapled": bool(args.staple),
            },
            indent=2,
        )
        + "\n"
    )


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest="command", required=True)
    s = sub.add_parser("keychain")
    s.add_argument("--path", required=True)
    s.add_argument("--env-output", required=True)
    s = sub.add_parser("notarize")
    s.add_argument("--file", required=True)
    s.add_argument("--staple")
    s.add_argument("--receipt", required=True)
    a = p.parse_args()
    try:
        {"keychain": keychain, "notarize": notarize}[a.command](a)
    except (ValueError, RuntimeError) as error:
        p.exit(1, str(error) + "\n")
