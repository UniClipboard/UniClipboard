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
import sys
import tempfile
import tomllib
from urllib.parse import quote, urlparse

sys.path.insert(0, str(Path(__file__).resolve().parent))
import desktop_release as release  # noqa: E402
import prepare_release  # noqa: E402

REPO = "UniClipboard/UniClipboard"
WEB = "https://github.com/" + REPO
API = "https://api.github.com"
TEMPLATE = release.ROOT / ".github/release-notes/desktop.md.tmpl"
NEW_TEMPLATE_TOKEN = "{{CHANGELOG_FILES_SECTION}}"
LEGACY_TEMPLATE = (
    "{{CHANGELOG_SECTION}}\n\n"
    "{{SOURCE_SECTION}}\n\n"
    "**Changelog Files**: [English and Chinese notes]({{NOTES_URL}})\n\n"
    "{{WARNINGS}}\n\n"
    "## App Installation\n\n"
    "{{INSTALLER_TABLE}}\n\n"
    "## CLI Downloads\n\n"
    "{{CLI_INSTALLER_TABLE}}\n\n"
    "## Verification\n\n"
    "Download `SHA256SUMS.txt` and verify the downloaded attachment bytes against it.\n"
    "Native updater archives have separate `.sig` files. They contain raw Ed25519 signatures over the archive SHA-256 digest.\n"
    "Operating-system code signing and updater signatures are separate checks.\n\n"
    "Desktop source: `{{SOURCE_SHA}}`.\n"
    "Locked CLI manifest SHA-256: `{{CLI_SHA256}}`.\n"
    "{{NEW_CONTRIBUTORS_SECTION}}\n"
)
# Pinned template identity for deterministic backfill and promotion of pre-migration
# Desktop releases (such as desktop/v2.0.0-build.15) regardless of future edits to TEMPLATE.
LEGACY_BACKFILL_TEMPLATE = (
    "{{CHANGELOG_SECTION}}\n\n"
    "{{SOURCE_SECTION}}\n\n"
    "{{CHANGELOG_FILES_SECTION}}\n\n"
    "{{WARNINGS}}\n\n"
    "## App Installation\n\n"
    "{{INSTALLER_TABLE}}\n\n"
    "## CLI Downloads\n\n"
    "{{CLI_INSTALLER_TABLE}}\n\n"
    "## Verification\n\n"
    "Download `SHA256SUMS.txt` and verify the downloaded attachment bytes against it.\n"
    "Native updater archives have separate `.sig` files. They contain raw Ed25519 signatures over the archive SHA-256 digest.\n"
    "Operating-system code signing and updater signatures are separate checks.\n\n"
    "Desktop source: `{{SOURCE_SHA}}`.\n"
    "Locked CLI manifest SHA-256: `{{CLI_SHA256}}`.\n"
    "{{NEW_CONTRIBUTORS_SECTION}}\n"
)
PLATFORMS = {"darwin": "macOS", "linux": "Linux", "windows": "Windows"}
ARCHES = {"amd64": "x86_64", "arm64": "ARM64"}
INSTALLERS = {"dmg", "setup.exe", "portable.zip", "deb", "rpm", "archlinux", "AppImage"}


def write(path, text):
    Path(path).write_bytes(text.encode("utf-8"))


class GitHubRequestError(ValueError):
    def __init__(self, method, path, status_code=None):
        self.method = method
        self.path = path
        self.status_code = status_code
        self.transient = status_code is None or status_code == 429 or status_code >= 500
        suffix = f" (HTTP {status_code})" if status_code is not None else ""
        super().__init__(f"GitHub request failed: {method} {path}{suffix}")


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
                stderr_text = result.stderr.decode("utf-8", errors="ignore")
                match = re.search(r"\bHTTP (\d{3})\b", stderr_text)
                status_code = int(match.group(1)) if match else None
                if missing and status_code == 404:
                    return None
                raise GitHubRequestError(method, path, status_code=status_code)
            if download is None:
                return json.loads(result.stdout)
        return None


def _git_toplevel(source_root):
    probe = subprocess.run(
        ["git", "-C", str(source_root), "rev-parse", "--show-toplevel"],
        capture_output=True,
        text=True,
    )
    if probe.returncode != 0:
        return None
    return Path(probe.stdout.strip()).resolve()


def _is_exported_source_dir(source_root, source_sha):
    source_root = Path(source_root).resolve()
    if (source_root / ".git").exists():
        return False
    marker = source_root / ".source-commit"
    if marker.is_file():
        recorded = marker.read_text(encoding="utf-8").strip()
        if recorded != source_sha:
            raise ValueError(
                f"exported source directory commit {recorded!r} does not match {source_sha}"
            )
        return True
    toplevel = _git_toplevel(source_root)
    if toplevel is None:
        return True
    if source_root == toplevel:
        return False
    # Inside a Git worktree: only treat an untracked directory (such as dist/catalogue/source)
    # as an exported source directory; reject tracked subdirectories of a wrong checkout.
    rel = source_root.relative_to(toplevel).as_posix()
    tracked = subprocess.run(
        ["git", "-C", str(toplevel), "ls-files", "--", rel],
        capture_output=True,
        text=True,
    )
    if tracked.returncode == 0 and tracked.stdout.strip():
        return False
    return True


def has_git_commit(source_root, source_sha):
    source_root = Path(source_root)
    if not (source_root / ".git").exists():
        return False
    probe = subprocess.run(
        ["git", "-C", str(source_root), "cat-file", "-e", f"{source_sha}^{{commit}}"],
        capture_output=True,
    )
    return probe.returncode == 0


def _source_mode(source_root, source_sha):
    source_root = Path(source_root)
    if not source_root.exists():
        return False
    if _is_exported_source_dir(source_root, source_sha):
        return False
    if not has_git_commit(source_root, source_sha):
        raise ValueError(
            f"source commit {source_sha} is not available in Git checkout {source_root}"
        )
    return True


def read_source_file(source_root, source_sha, relpath, use_git=None):
    source_root = Path(source_root)
    if use_git is None:
        use_git = _source_mode(source_root, source_sha)
    if use_git:
        result = subprocess.run(
            ["git", "-C", str(source_root), "show", f"{source_sha}:{relpath}"],
            capture_output=True,
        )
        if result.returncode != 0:
            return None
        return result.stdout.decode("utf-8")
    candidate = source_root / relpath
    if not candidate.is_file():
        return None
    return candidate.read_text(encoding="utf-8")


def _extract_changelog_body(raw, rel, version, build_num):
    text = raw.replace("\r\n", "\n").replace("\r", "\n")
    for marker in prepare_release.PLACEHOLDER_MARKERS:
        if marker in text:
            raise ValueError(f"changelog {rel} contains forbidden marker {marker!r}")
    prefix = f"## v{version}-build.{build_num} (desktop) - "
    if not text.startswith(prefix):
        raise ValueError(f"unexpected changelog heading in {rel}")
    body = text.split("\n", 1)[1].strip() if "\n" in text else ""
    if not body:
        raise ValueError(f"changelog {rel} has no content")
    return body


def resolve_changelog(m, source_root):
    source_root = Path(source_root)
    source_sha = m["source_sha"]
    version = m["version"]
    build_number = m["build_number"]
    use_git = _source_mode(source_root, source_sha)

    def read_pair(candidate_build):
        en_rel = f"release/changelogs/desktop/v{version}-build.{candidate_build}.md"
        zh_rel = f"release/changelogs/desktop/v{version}-build.{candidate_build}.zh.md"
        en_raw = read_source_file(source_root, source_sha, en_rel, use_git=use_git)
        zh_raw = read_source_file(source_root, source_sha, zh_rel, use_git=use_git)
        if (en_raw is None) != (zh_raw is None):
            raise ValueError(
                f"incomplete bilingual Desktop changelog for v{version}-build.{candidate_build}"
            )
        if en_raw is None:
            return None
        return candidate_build, en_rel, en_raw, zh_rel, zh_raw

    pair = read_pair(build_number)
    if pair is None:
        toml_raw = read_source_file(
            source_root, source_sha, "release/products.toml", use_git=use_git
        )
        if toml_raw is not None:
            desktop_meta = tomllib.loads(toml_raw).get("products", {}).get("desktop", {})
            toml_build = desktop_meta.get("build_number")
            if (
                desktop_meta.get("version") == version
                and type(toml_build) is int
                and 1 <= toml_build < build_number
            ):
                pair = read_pair(toml_build)
    if pair is None:
        return None
    candidate_build, en_rel, en_raw, zh_rel, zh_raw = pair
    return {
        "build_number": candidate_build,
        "en_rel": en_rel,
        "zh_rel": zh_rel,
        "en": _extract_changelog_body(en_raw, en_rel, version, candidate_build),
        "zh": _extract_changelog_body(zh_raw, zh_rel, version, candidate_build),
    }


def page(m, cli, notes, channel, previous, generated, template, changelog=None):
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
    notes_url = download + "desktop-notes.json"
    if NEW_TEMPLATE_TOKEN in template:
        if changelog is None:
            raise ValueError("bilingual Desktop markdown changelog is required")
        changelog_section = (
            f"## {m['version']} — Desktop build {m['build_number']}\n\n"
            f"{changelog['en']}\n\n"
            "<details>\n"
            "<summary>中文变更日志</summary>\n\n"
            f"{changelog['zh']}\n\n"
            "</details>"
        )
        en_url = f"{WEB}/blob/{m['source_sha']}/{changelog['en_rel']}"
        zh_url = f"{WEB}/blob/{m['source_sha']}/{changelog['zh_rel']}"
        changelog_files_section = (
            f"**Changelog Files**: [English]({en_url}) · [中文]({zh_url}) · "
            f"[Updater Notes JSON]({notes_url})"
        )
    else:
        changelog_section = (
            f"## {m['version']} — Desktop build {m['build_number']}\n\n{notes['en']}"
        )
        changelog_files_section = (
            f"**Changelog Files**: [English and Chinese notes]({notes_url})"
        )
    source = (
        f"**Full Changelog**: {WEB}/compare/{quote(previous, safe='')}...{m['source_sha']}"
        if previous
        else f"**Source**: {WEB}/commit/{m['source_sha']}"
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
            "Windows packages are unsigned (no production Authenticode signature). Verify the attachment checksums before installation."
        )
    contributors = ""
    if generated:
        match = re.search(
            r"^## New Contributors\s*\n(.*?)(?=^## |\Z)", generated, re.M | re.S
        )
        if match:
            contributors = "\n## New Contributors\n\n" + match[1].strip() + "\n"
    values = {
        "CHANGELOG_SECTION": changelog_section,
        "CHANGELOG_FILES_SECTION": changelog_files_section,
        "SOURCE_SECTION": source,
        "NOTES_URL": notes_url,
        "WARNINGS": "\n\n".join(warnings),
        "INSTALLER_TABLE": "\n".join(rows),
        "CLI_INSTALLER_TABLE": "\n".join(cli_rows),
        "SOURCE_SHA": m["source_sha"],
        "CLI_SHA256": m["cli_manifest_sha256"],
        "NEW_CONTRIBUTORS_SECTION": contributors,
    }
    unresolved = set()

    def replace_token(match):
        key = match.group(1)
        if key not in values:
            unresolved.add(key)
            return match.group(0)
        return values[key]

    text = re.sub(r"\{\{([A-Z0-9_]+)\}\}", replace_token, template)
    if unresolved:
        raise ValueError("unresolved Release template field")
    return text


def bundle(args, force_template=None):
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
    changelog = resolve_changelog(m, args.source_root)
    template = (
        force_template
        if force_template is not None
        else args.template.read_text(encoding="utf-8")
    )
    if NEW_TEMPLATE_TOKEN in template:
        bodies = {
            c: page(
                m,
                cli,
                notes,
                c,
                args.previous_tag,
                generated,
                template,
                changelog=changelog,
            )
            for c in ("alpha", "stable")
        }
        backfilled_bodies = None
    else:
        bodies = {
            c: page(
                m, cli, notes, c, args.previous_tag, generated, template, changelog=None
            )
            for c in ("alpha", "stable")
        }
        backfilled_bodies = (
            {
                c: page(
                    m,
                    cli,
                    notes,
                    c,
                    args.previous_tag,
                    generated,
                    LEGACY_BACKFILL_TEMPLATE,
                    changelog=changelog,
                )
                for c in ("alpha", "stable")
            }
            if changelog is not None
            else None
        )
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
    return m, files, bodies, backfilled_bodies, (cli, notes, generated, changelog)


def restore_page_context(gh, prefix, existing, files, args, receipt):
    items = gh.api("GET", prefix + f"/releases/{existing['id']}/assets?per_page=100")
    # GitHub Release currently has 32 assets; 100 entries also exposes foreign assets.
    if len(items) >= 100 or len({a["name"] for a in items}) != len(items):
        raise ValueError("ambiguous Release asset catalogue")
    assets = {a["name"]: a for a in items}
    if set(assets) - set(files):
        raise ValueError("unexpected existing Release attachment")
    bundle_result = None
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
            bundle_result = bundle(args)
            receipt["assets"] = {
                name: release.package.sha(path)
                for name, path in sorted(bundle_result[1].items())
            }
    return assets, items, bundle_result


def verify_existing_asset_bytes(gh, prefix, assets, receipt):
    with tempfile.TemporaryDirectory(prefix="unic-gh-readback-") as temporary:
        for name, a in assets.items():
            if a.get("digest"):
                if a["digest"] != "sha256:" + receipt["assets"][name]:
                    raise ValueError("existing Release attachment byte conflict")
                continue
            downloaded = Path(temporary) / name
            gh.api("GET", prefix + f"/releases/assets/{a['id']}", download=downloaded)
            if release.package.sha(downloaded) != receipt["assets"][name]:
                raise ValueError("existing Release attachment byte conflict")


def record_first_failure(output, stage, error):
    failure_path = Path(output) / "first-failure.json"
    if failure_path.exists():
        return
    write(
        failure_path,
        json.dumps(
            {
                "stage": stage,
                "error_type": type(error).__name__,
                "status_code": getattr(error, "status_code", None),
                "message": str(error),
            },
            indent=2,
        )
        + "\n",
    )


def _release_snapshot(release_obj, items):
    return {
        "id": release_obj["id"],
        "tag_name": release_obj["tag_name"],
        "name": release_obj["name"],
        "draft": release_obj["draft"],
        "prerelease": release_obj["prerelease"],
        "target_commitish": release_obj["target_commitish"],
        "body": release_obj["body"],
        "assets": [
            {
                "id": a["id"],
                "name": a["name"],
                "size": a["size"],
                "digest": a.get("digest"),
            }
            for a in sorted(items, key=lambda item: item["name"])
        ],
    }


def _legacy_template_for_backfill(args, source_sha):
    if args.legacy_template:
        template = args.legacy_template.read_text(encoding="utf-8")
    else:
        template = read_source_file(
            args.source_root, source_sha, ".github/release-notes/desktop.md.tmpl"
        )
        if template is None or NEW_TEMPLATE_TOKEN in template:
            template = LEGACY_TEMPLATE
    if NEW_TEMPLATE_TOKEN in template:
        raise ValueError("legacy template must not use markdown changelog section")
    return template


def _verify_live_release_for_patch(
    gh, prefix, tag, title, m, files, args, receipt, before, legacy_body, target_body
):
    cur_ref = gh.api("GET", prefix + "/git/ref/tags/" + quote(tag, safe=""), missing=True)
    if not cur_ref or (cur_ref["object"].get("type"), cur_ref["object"].get("sha")) != (
        "commit",
        m["source_sha"],
    ):
        raise ValueError("Desktop tag ref changed before body update")
    cur_rel = gh.api("GET", prefix + f"/releases/{before['id']}")
    if (
        cur_rel.get("id") != before["id"]
        or cur_rel.get("tag_name") != tag
        or cur_rel.get("name") != title
        or cur_rel.get("draft") is not False
        or cur_rel.get("prerelease") != (args.channel == "alpha")
        or cur_rel.get("target_commitish") != m["source_sha"]
    ):
        raise ValueError("Release identity changed before body update")
    cur_items = gh.api(
        "GET", prefix + f"/releases/{before['id']}/assets?per_page=100"
    )
    if len(cur_items) >= 100 or len({a["name"] for a in cur_items}) != len(cur_items):
        raise ValueError("ambiguous Release asset catalogue before body update")
    cur_snap = _release_snapshot(cur_rel, cur_items)
    if cur_snap["assets"] != before["assets"] or {
        a["name"] for a in cur_items
    } != set(files):
        raise ValueError("Release attachment catalogue changed before body update")
    if any(
        a.get("digest") and a["digest"] != "sha256:" + receipt["assets"][a["name"]]
        for a in cur_items
    ):
        raise ValueError("Release attachment digest changed before body update")
    if cur_rel["body"] == target_body:
        return "already_applied", cur_rel, cur_items
    if cur_rel["body"] != before["body"]:
        raise ValueError("existing Release body changed since initial read")
    if cur_rel["body"] != legacy_body:
        raise ValueError("existing Release notes differ from accepted inputs")
    return "ready", cur_rel, cur_items


def run_update_body(gh, prefix, tag, title, m, files, bundle_meta, ref, existing, args, receipt):
    output = Path(args.output).resolve()
    cli, notes, generated, changelog = bundle_meta
    try:
        if not existing:
            raise ValueError("published Release is required for update-body")
        if existing["draft"] or existing["prerelease"] != (args.channel == "alpha"):
            raise ValueError("Release publication state mismatch for update-body")
        if (
            not ref
            or (ref["object"].get("type"), ref["object"].get("sha"))
            != ("commit", m["source_sha"])
            or existing.get("target_commitish") != m["source_sha"]
            or existing.get("name") != title
            or existing.get("tag_name") != tag
        ):
            raise ValueError("Release source or title mismatch for update-body")
        initial_release_snapshot = dict(existing)
        assets, items, restored = restore_page_context(
            gh, prefix, existing, files, args, receipt
        )
        before = _release_snapshot(initial_release_snapshot, items)
        if not (output / "before.json").exists():
            write(output / "before.json", json.dumps(before, indent=2) + "\n")
        if restored is not None:
            m, files, _, _, (cli, notes, generated, changelog) = restored
        if set(assets) != set(files):
            raise ValueError("published Release attachment set is incomplete")
        verify_existing_asset_bytes(gh, prefix, assets, receipt)
        if changelog is None:
            raise ValueError("bilingual Desktop markdown changelog is required for update-body")
        legacy_template = _legacy_template_for_backfill(args, m["source_sha"])
        legacy_body = page(
            m, cli, notes, args.channel, args.previous_tag, generated, legacy_template, changelog=None
        )
        target_body = page(
            m,
            cli,
            notes,
            args.channel,
            args.previous_tag,
            generated,
            LEGACY_BACKFILL_TEMPLATE,
            changelog=changelog,
        )
        write(output / "release-notes.md", target_body)
        if before["body"] not in (legacy_body, target_body):
            raise ValueError("existing Release notes differ from accepted inputs")
    except BaseException as error:
        record_first_failure(output, "preflight", error)
        raise

    # GitHub Releases REST API does not offer atomic conditional PATCH (no server-side
    # ETag/CAS guarantee). We enforce a client-side re-check after slow asset checks
    # and before every write attempt, skipping duplicate writes when target_body is live.
    max_attempts = 3
    write_attempts = 0
    already_applied = False
    after_release = None
    after_items = None
    for attempt in range(1, max_attempts + 1):
        try:
            state_kind, cur_rel, cur_items = _verify_live_release_for_patch(
                gh,
                prefix,
                tag,
                title,
                m,
                files,
                args,
                receipt,
                before,
                legacy_body,
                target_body,
            )
            if state_kind == "already_applied":
                already_applied = True
                after_release, after_items = cur_rel, cur_items
                break
        except BaseException as error:
            record_first_failure(output, "preflight", error)
            raise

        try:
            write_attempts += 1
            gh.api(
                "PATCH",
                prefix + f"/releases/{existing['id']}",
                {"body": target_body},
            )
            break
        except GitHubRequestError as error:
            record_first_failure(output, "patch", error)
            if not error.transient or attempt == max_attempts:
                raise

    try:
        if after_release is None:
            after_release = gh.api("GET", prefix + f"/releases/{existing['id']}")
            after_items = gh.api(
                "GET", prefix + f"/releases/{existing['id']}/assets?per_page=100"
            )
        after = _release_snapshot(after_release, after_items)
        expected_after = dict(before, body=target_body)
        if after != expected_after:
            raise ValueError("updated Release state read-back mismatch")
    except BaseException as error:
        record_first_failure(output, "readback", error)
        raise

    changed = write_attempts > 0
    receipt.update(
        status="updated" if changed else "unchanged",
        changed=changed,
        already_applied=already_applied,
        write_attempts=write_attempts,
        server_atomic_cas=False,
        url=after_release["html_url"],
        channel=args.channel,
        body_sha256=hashlib.sha256(target_body.encode("utf-8")).hexdigest(),
    )
    return receipt


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
    m, files, bodies, backfilled_bodies, bundle_meta = bundle(args)
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
    existing = release.package.cli.find_github_release(
        REPO, tag, read=lambda endpoint: gh.api("GET", "/" + endpoint)
    )
    title = f"UniClipboard Desktop {m['version']} (build {m['build_number']})"
    if args.operation == "update-body":
        return run_update_body(
            gh,
            prefix,
            tag,
            title,
            m,
            files,
            bundle_meta,
            ref,
            existing,
            args,
            receipt,
        )
    if (
        existing
        and args.channel == "alpha"
        and not existing["draft"]
        and not existing["prerelease"]
    ):
        raise ValueError("cannot downgrade stable Release to alpha")
    assets = {}
    selected_bodies = bodies
    if existing:
        assets, _, restored = restore_page_context(
            gh, prefix, existing, files, args, receipt
        )
        if restored is not None:
            m, files, bodies, backfilled_bodies, _ = restored
        if existing["name"] != title:
            raise ValueError("existing Release notes differ from accepted inputs")
        if existing["body"] in bodies.values():
            selected_bodies = bodies
        elif backfilled_bodies is not None and existing["body"] in backfilled_bodies.values():
            selected_bodies = backfilled_bodies
            write(Path(args.output) / "release-notes.md", selected_bodies[args.channel])
        else:
            raise ValueError("existing Release notes differ from accepted inputs")
        verify_existing_asset_bytes(gh, prefix, assets, receipt)
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
                "body": selected_bodies[args.channel],
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
    if any(
        a.get("digest") and a["digest"] != "sha256:" + receipt["assets"][a["name"]]
        for a in current_assets
    ):
        raise ValueError("Release attachment digest changed during staging")
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
                "body": selected_bodies[args.channel],
                "make_latest": "true" if args.channel == "stable" else "false",
            },
        )
    final = gh.api("GET", prefix + f"/releases/{existing['id']}")
    if args.operation == "publish" and (
        final["draft"]
        or final["prerelease"] != (args.channel == "alpha")
        or final["body"] != selected_bodies[args.channel]
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
    p.add_argument("operation", choices=["preview", "draft", "publish", "update-body"])
    for name in ["manifest", "cli-manifest", "notes", "output"]:
        p.add_argument("--" + name, required=True)
    p.add_argument("--template", type=Path, default=TEMPLATE)
    p.add_argument("--legacy-template", type=Path, default=None)
    p.add_argument("--source-root", type=Path, default=release.ROOT)
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
