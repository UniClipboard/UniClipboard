#!/usr/bin/env python3
"""Render v1-style Desktop pages and stage immutable GitHub Release assets."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from urllib.parse import quote, urlparse

import desktop_release as release

REPO = "UniClipboard/UniClipboard"
WEB = "https://github.com/" + REPO
API = "https://api.github.com"
TEMPLATE = release.ROOT / ".github/release-notes/desktop.md.tmpl"
PLATFORMS = {"darwin": "macOS", "linux": "Linux", "windows": "Windows"}
ARCHES = {"amd64": "x86_64", "arm64": "ARM64"}
INSTALLERS = {"dmg", "setup.exe", "portable.zip", "deb", "rpm", "archlinux", "AppImage"}


def write(path, text):
    Path(path).write_bytes(text.encode("utf-8"))


class GitHub:
    def __init__(self, base):
        self.base = base

    def api(self, method, path, body=None, upload=None, download=None, missing=False):
        url = self.base + path
        command = ["gh", "api", url, "--method", method]
        with tempfile.TemporaryDirectory(prefix="unic-gh-request-") as temporary:
            if body is not None:
                request = Path(temporary) / "request.json"
                write(request, json.dumps(body))
                command += ["--input", str(request)]
            if upload is not None:
                command += [
                    "--input",
                    str(upload),
                    "-H",
                    "Content-Type: application/octet-stream",
                ]
            if download is not None:
                command += ["-H", "Accept: application/octet-stream"]
                with Path(download).open("wb") as stream:
                    result = subprocess.run(
                        command, stdout=stream, stderr=subprocess.PIPE
                    )
            else:
                result = subprocess.run(command, capture_output=True)
            if result.returncode:
                if missing and b"HTTP 404" in result.stderr:
                    return None
                raise ValueError("GitHub request failed: " + method + " " + path)
            if download is None:
                return json.loads(result.stdout)
        return None


def page(m, cli, notes, channel, previous, generated):
    tag = f"desktop/v{m['version']}-build.{m['build_number']}"
    download = WEB + "/releases/download/" + quote(tag, safe="") + "/"
    rows = ["| Platform | Architecture | Download |", "| --- | --- | --- |"]
    for a in sorted(m["artifacts"], key=lambda a: (a["target"], a["format"])):
        if a["format"] not in INSTALLERS:
            continue
        system, arch = a["target"].split("-")
        rows.append(
            f"| {PLATFORMS[system]} | {ARCHES[arch]} ({a['format']}) | [{a['file']}]({download + quote(a['file'])}) |"
        )
    cli_tag = f"cli/v{cli['version']}-build.{cli['build_number']}"
    cli_rows = [
        f"Standalone CLI: `{cli_tag}`. Desktop already includes its own private CLI and service.",
        "",
        "These downloads are npm platform packages. Install through npm to use the launcher.",
        "",
        f"```sh\nnpm install -g @uniclipboard/cli@{cli['npm_version']}\n```",
        "",
        "| Platform | Architecture | Download |",
        "| --- | --- | --- |",
    ]
    by_name = {p["name"]: p for p in cli["packages"]}
    for target in release.TARGETS:
        p = by_name[release.package.cli.package_name(target)]
        system, arch = target.split("-")
        cli_rows.append(
            f"| {PLATFORMS[system]} | {ARCHES[arch]} | [{p['file']}]({WEB}/releases/download/{quote(cli_tag, safe='')}/{quote(p['file'])}) |"
        )
    changelog = (
        f"## {m['version']} — Desktop build {m['build_number']}\n\n{notes['en']}"
    )
    source = (
        f'**Full Changelog**: {WEB}/compare/{quote(previous, safe="")}...{m["source_sha"]}'
        if previous
        else f'**Source**: {WEB}/commit/{m["source_sha"]}'
    )
    warnings = [
        "All paired devices must use v2. Quit v1 before starting v2. Import v1 history explicitly with `unic migrate`."
    ]
    if channel == "alpha":
        warnings.insert(
            0, "## Prerelease Warning\n\nThis is an alpha release for testing."
        )
    if m.get("development") or not m.get("source_clean"):
        warnings.append(
            "This preview uses development inputs and is not a production release."
        )
    if any(
        a["target"].startswith("windows-") and not a.get("signed")
        for a in m["artifacts"]
    ):
        warnings.append(
            "Windows packages are not code-signed. Verify the attachment checksums before installation."
        )
    contributors = ""
    if generated:
        match = re.search(
            r"^## New Contributors\s*\n(.*?)(?=^## |\Z)", generated, re.M | re.S
        )
        if match:
            contributors = "\n## New Contributors\n\n" + match[1].strip() + "\n"
    values = {
        "CHANGELOG_SECTION": changelog,
        "SOURCE_SECTION": source,
        "NOTES_URL": download + "desktop-notes.json",
        "WARNINGS": "\n\n".join(warnings),
        "INSTALLER_TABLE": "\n".join(rows),
        "CLI_INSTALLER_TABLE": "\n".join(cli_rows),
        "SOURCE_SHA": m["source_sha"],
        "CLI_SHA256": m["cli_manifest_sha256"],
        "NEW_CONTRIBUTORS_SECTION": contributors,
    }
    text = TEMPLATE.read_text(encoding="utf-8")
    for key, value in values.items():
        text = text.replace("{{" + key + "}}", value)
    if re.search(r"{{[A-Z_]+}}", text):
        raise ValueError("unresolved Release template field")
    return text


def bundle(args):
    manifest, m = release.load(args.manifest, not (args.fixture or args.development))
    cli_bytes = Path(args.cli_manifest).read_bytes()
    if hashlib.sha256(cli_bytes).hexdigest() != m["cli_manifest_sha256"]:
        raise ValueError("Release page CLI lock mismatch")
    with tempfile.TemporaryDirectory(prefix="unic-cli-metadata-") as temporary:
        snapshot = Path(temporary) / "cli.json"
        snapshot.write_bytes(cli_bytes)
        cli = release.package.cli.validate_manifest(snapshot)
    if sorted(cli["targets"]) != release.TARGETS:
        raise ValueError("Release page needs all six CLI targets")
    notes_bytes = Path(args.notes).read_bytes()
    notes = json.loads(notes_bytes)
    if set(notes) != {"en", "zh"} or not all(
        isinstance(v, str) and v.strip() for v in notes.values()
    ):
        raise ValueError("English and Chinese changelog strings are required")
    output = Path(args.output).resolve()
    if output == manifest.parent:
        raise ValueError("page output must not replace the catalogue")
    output.mkdir(parents=True, exist_ok=True)
    files = {}
    for a in m["artifacts"]:
        expected_updater = a["format"] == (
            "app.tar.gz" if a["target"].startswith("darwin-") else "update.tar.gz"
        )
        if a["updater"] != expected_updater:
            raise ValueError("invalid updater attachment selection")
        name = a["file"]
        if not re.fullmatch(r"[A-Za-z0-9._-]+", name) or name in files:
            raise ValueError("unsafe or duplicate Release attachment name")
        shutil.copyfile(manifest.parent / name, output / name)
        files[name] = output / name
        if (
            release.package.sha(files[name]) != a["sha256"]
            or files[name].stat().st_size != a["size"]
        ):
            raise ValueError("payload changed during Release staging")
        if a["updater"]:
            write(output / (name + ".sig"), a["signature"] + "\n")
            files[name + ".sig"] = output / (name + ".sig")
    shutil.copyfile(manifest, output / "release-manifest.json")
    if json.loads((output / "release-manifest.json").read_bytes()) != m:
        raise ValueError("catalogue changed during Release staging")
    (output / "desktop-notes.json").write_bytes(notes_bytes)
    files["release-manifest.json"] = output / "release-manifest.json"
    files["desktop-notes.json"] = output / "desktop-notes.json"
    generated = (
        Path(args.generated_notes).read_text(encoding="utf-8")
        if args.generated_notes
        else ""
    )
    bodies = {
        c: page(m, cli, notes, c, args.previous_tag, generated)
        for c in ("alpha", "stable")
    }
    write(output / "release-notes.md", bodies[args.channel])
    context = {
        "schema": 1,
        "manifest_sha256": release.package.sha(files["release-manifest.json"]),
        "notes_sha256": release.package.sha(files["desktop-notes.json"]),
        "previous_tag": args.previous_tag,
        "generated_notes": generated,
    }
    write(output / "release-page.json", json.dumps(context, indent=2) + "\n")
    files["release-page.json"] = output / "release-page.json"
    write(
        output / "SHA256SUMS.txt",
        "".join(
            release.package.sha(p) + "  " + name + "\n"
            for name, p in sorted(files.items())
        ),
    )
    files["SHA256SUMS.txt"] = output / "SHA256SUMS.txt"
    return m, files, bodies


def matching_receipt(path, m, args, status, manifest_sha):
    if not path:
        raise ValueError("required availability or promotion receipt is missing")
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    if (
        data.get("status") != status
        or data.get("manifest_sha256") != manifest_sha
        or bool(data.get("fixture")) != args.fixture
    ):
        raise ValueError("Release availability/promotion receipt mismatch")
    if status == "available" and (
        data.get("product"),
        data.get("version"),
        data.get("build_number"),
    ) != ("desktop", m["version"], m["build_number"]):
        raise ValueError("Release availability identity mismatch")
    if status == "promoted" and data.get("channel") != "desktop/" + args.channel:
        raise ValueError("Release promotion channel mismatch")


def run(args):
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
    m, files, bodies = bundle(args)
    tag = f"desktop/v{m['version']}-build.{m['build_number']}"
    receipt = {
        "status": "preview",
        "tag": tag,
        "source_sha": m["source_sha"],
        "manifest_sha256": release.package.sha(files["release-manifest.json"]),
        "fixture": args.fixture,
        "assets": {
            name: release.package.sha(path) for name, path in sorted(files.items())
        },
    }
    if args.operation == "preview":
        return receipt
    if args.operation == "publish":
        matching_receipt(
            args.publication, m, args, "available", receipt["manifest_sha256"]
        )
        if args.channel == "stable" or args.promotion:
            matching_receipt(
                args.promotion, m, args, "promoted", receipt["manifest_sha256"]
            )
    gh = GitHub(args.api_base)
    prefix = "/repos/" + REPO
    ref = gh.api("GET", prefix + "/git/ref/tags/" + quote(tag, safe=""), missing=True)
    if ref and (ref["object"].get("type"), ref["object"].get("sha")) != (
        "commit",
        m["source_sha"],
    ):
        raise ValueError("Desktop tag source conflict")
    existing = gh.api(
        "GET", prefix + "/releases/tags/" + quote(tag, safe=""), missing=True
    )
    title = f"UniClipboard Desktop {m['version']} (build {m['build_number']})"
    if (
        existing
        and args.channel == "alpha"
        and not existing["draft"]
        and not existing["prerelease"]
    ):
        raise ValueError("cannot downgrade stable Release to alpha")
    assets = {}
    if existing:
        items = gh.api(
            "GET", prefix + f"/releases/{existing['id']}/assets?per_page=100"
        )
        # GitHub Release currently has 32 assets; 100 entries also exposes foreign assets.
        if len(items) >= 100 or len({a["name"] for a in items}) != len(items):
            raise ValueError("ambiguous Release asset catalogue")
        assets = {a["name"]: a for a in items}
        if set(assets) - set(files):
            raise ValueError("unexpected existing Release attachment")
        if "release-page.json" in assets:
            with tempfile.TemporaryDirectory(prefix="unic-gh-context-") as temporary:
                context_path = Path(temporary) / "release-page.json"
                gh.api(
                    "GET",
                    prefix + f"/releases/assets/{assets['release-page.json']['id']}",
                    download=context_path,
                )
                context = json.loads(context_path.read_text(encoding="utf-8"))
                if (
                    context.get("schema") != 1
                    or context.get("manifest_sha256") != receipt["manifest_sha256"]
                    or context.get("notes_sha256")
                    != release.package.sha(files["desktop-notes.json"])
                    or not isinstance(context.get("previous_tag"), str)
                    or not isinstance(context.get("generated_notes"), str)
                ):
                    raise ValueError("Release page context identity mismatch")
                if args.previous_tag and args.previous_tag != context["previous_tag"]:
                    raise ValueError("Release comparison tag conflict")
                if (
                    args.generated_notes
                    and Path(args.generated_notes).read_text(encoding="utf-8").strip()
                    and Path(args.generated_notes).read_text(encoding="utf-8")
                    != context["generated_notes"]
                ):
                    raise ValueError("Release generated notes conflict")
                args.previous_tag = context["previous_tag"]
                args.generated_notes = str(Path(temporary) / "generated.md")
                write(args.generated_notes, context["generated_notes"])
                m, files, bodies = bundle(args)
                receipt["assets"] = {
                    name: release.package.sha(path)
                    for name, path in sorted(files.items())
                }
        if existing["name"] != title or existing["body"] not in bodies.values():
            raise ValueError("existing Release notes differ from accepted inputs")
        with tempfile.TemporaryDirectory(prefix="unic-gh-readback-") as temporary:
            for name, a in assets.items():
                downloaded = Path(temporary) / name
                gh.api(
                    "GET", prefix + f"/releases/assets/{a['id']}", download=downloaded
                )
                if release.package.sha(downloaded) != receipt["assets"][name]:
                    raise ValueError("existing Release attachment byte conflict")
    if existing and not existing["draft"] and set(assets) != set(files):
        raise ValueError("published Release attachment set is incomplete")
    if args.operation == "publish" and (not existing or set(assets) != set(files)):
        raise ValueError("complete draft staging is required before publication")
    if not ref:
        gh.api(
            "POST",
            prefix + "/git/refs",
            {"ref": "refs/tags/" + tag, "sha": m["source_sha"]},
        )
    if not existing:
        existing = gh.api(
            "POST",
            prefix + "/releases",
            {
                "tag_name": tag,
                "target_commitish": m["source_sha"],
                "name": title,
                "body": bodies[args.channel],
                "draft": True,
                "prerelease": args.channel == "alpha",
                "make_latest": "false",
            },
        )
    upload = GitHub(args.api_base if args.fixture else "https://uploads.github.com")
    for name in sorted(set(files) - set(assets)):
        a = upload.api(
            "POST",
            prefix + f"/releases/{existing['id']}/assets?name={quote(name)}",
            upload=files[name],
        )
        assets[name] = a
        with tempfile.TemporaryDirectory(prefix="unic-gh-upload-") as temporary:
            downloaded = Path(temporary) / name
            gh.api("GET", prefix + f"/releases/assets/{a['id']}", download=downloaded)
            if release.package.sha(downloaded) != receipt["assets"][name]:
                raise ValueError("uploaded Release attachment read-back mismatch")
    current_assets = gh.api(
        "GET", prefix + f"/releases/{existing['id']}/assets?per_page=100"
    )
    expected_assets = {
        name: (assets[name]["id"], path.stat().st_size) for name, path in files.items()
    }
    observed_assets = {a["name"]: (a["id"], a["size"]) for a in current_assets}
    if len(current_assets) != len(files) or observed_assets != expected_assets:
        raise ValueError("Release attachment set changed during staging")
    current_ref = gh.api("GET", prefix + "/git/ref/tags/" + quote(tag, safe=""))
    if (current_ref["object"].get("type"), current_ref["object"].get("sha")) != (
        "commit",
        m["source_sha"],
    ):
        raise ValueError("Desktop tag source changed during staging")
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
        raise ValueError("published Release state read-back mismatch")
    receipt.update(
        status="draft" if final["draft"] else "published",
        url=final["html_url"],
        channel=args.channel,
    )
    return receipt


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("operation", choices=["preview", "draft", "publish"])
    for name in ["manifest", "cli-manifest", "notes", "output"]:
        p.add_argument("--" + name, required=True)
    p.add_argument("--channel", choices=["alpha", "stable"], default="alpha")
    p.add_argument("--previous-tag", default="")
    p.add_argument("--generated-notes", default="")
    p.add_argument("--publication")
    p.add_argument("--promotion")
    p.add_argument("--api-base", default=API)
    p.add_argument("--fixture", action="store_true")
    p.add_argument("--development", action="store_true")
    args = p.parse_args()
    try:
        result = run(args)
    except BaseException as error:
        if Path(args.output).is_dir():
            write(
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
    write(
        Path(args.output) / "github-receipt.json", json.dumps(result, indent=2) + "\n"
    )
    print(json.dumps({"status": result["status"], "tag": result["tag"]}))


if __name__ == "__main__":
    main()
