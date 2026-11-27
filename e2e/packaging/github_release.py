#!/usr/bin/env python3
"""Run the actual GitHub CLI and page controller against an isolated HTTP service."""

import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
from urllib.parse import parse_qs, unquote, urlparse

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools/release"))
import desktop_release as release  # noqa: E402

out = ROOT / ".cache/github-release-e2e"
out.mkdir(parents=True, exist_ok=True)
report = {
    "status": "running",
    "scope": "actual gh and Python processes; isolated HTTP; no real Release",
    "checks": [],
}


def check(name, value):
    assert value, name
    report["checks"].append(name)


try:
    with tempfile.TemporaryDirectory(prefix="unic-github-release-") as work:
        temp = Path(work)
        catalogue = temp / "catalogue"
        catalogue.mkdir()
        cli = {
            "schema": 1,
            "product": "cli",
            "version": "2.0.0",
            "build_number": 2,
            "npm_version": "2.0.0-build.2",
            "targets": release.TARGETS,
            "packages": [],
        }
        for target in release.TARGETS:
            cli["packages"].append(
                {
                    "name": release.package.cli.package_name(target),
                    "version": cli["npm_version"],
                    "file": target + ".tgz",
                }
            )
        cli["packages"].append(
            {
                "name": "@uniclipboard/cli",
                "version": cli["npm_version"],
                "file": "cli.tgz",
            }
        )
        cli_path = temp / "cli.json"
        release.save(cli_path, cli)
        manifest = {
            "schema": 1,
            "product": "desktop",
            "version": "2.0.0",
            "build_number": 1,
            "source_sha": "a" * 40,
            "cli_manifest_sha256": release.package.sha(cli_path),
            "development": True,
            "source_clean": False,
            "artifacts": [],
        }
        for target in release.TARGETS:
            for kind in sorted(release.FORMATS[target.split("-")[0]]):
                name = target + "." + kind
                blob = ("fixture " + name).encode()
                (catalogue / name).write_bytes(blob)
                manifest["artifacts"].append(
                    {
                        "file": name,
                        "format": kind,
                        "target": target,
                        "sha256": hashlib.sha256(blob).hexdigest(),
                        "size": len(blob),
                        "acceptance": {},
                        "updater": kind
                        == (
                            "app.tar.gz"
                            if target.startswith("darwin")
                            else "update.tar.gz"
                        ),
                        "signature": "fixture-signature",
                    }
                )
        manifest_path = catalogue / "release-manifest.json"
        release.save(manifest_path, manifest)
        notes = temp / "notes.json"
        release.save(notes, {"en": "Independent v2 products.", "zh": "V2 notes."})
        source_root = temp / "source"
        changelog_dir = source_root / "release/changelogs/desktop"
        changelog_dir.mkdir(parents=True)
        (changelog_dir / "v2.0.0-build.1.md").write_text(
            "## v2.0.0-build.1 (desktop) - 2026-11-27\n\n### Features\n\n- Detailed English item for build 1. (#101)\n",
            encoding="utf-8",
        )
        (changelog_dir / "v2.0.0-build.1.zh.md").write_text(
            "## v2.0.0-build.1 (desktop) - 2026-11-27\n\n### Features\n\n- 构建 1 的中文详细条目。 (#101)\n",
            encoding="utf-8",
        )
        generated = temp / "generated.md"
        generated.write_text(
            "## What changed\nIgnored generated text\n## New Contributors\n* @example contributed.\n",
            encoding="utf-8",
        )
        availability = temp / "publication.json"
        release.save(
            availability,
            {
                "status": "available",
                "product": "desktop",
                "version": "2.0.0",
                "build_number": 1,
                "manifest_sha256": release.package.sha(manifest_path),
                "fixture": True,
            },
        )
        promotion = temp / "promotion.json"
        release.save(
            promotion,
            {
                "status": "promoted",
                "channel": "desktop/stable",
                "manifest_sha256": release.package.sha(manifest_path),
                "fixture": True,
            },
        )
        state = {
            "tag": None,
            "release": None,
            "assets": {},
            "writes": 0,
            "patch_attempts": 0,
            "corrupt_upload": False,
            "patch_failures": [],
            "patch_payloads": [],
            "probe_arm_get": False,
            "probe_arm_body": None,
            "probe_patch_body": None,
            "probe_retag": False,
            "probe_applied": False,
        }

        class Server(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def send(self, status, body):
                binary = isinstance(body, bytes)
                data = body if binary else json.dumps(body).encode()
                self.send_response(status)
                self.send_header(
                    "Content-Type",
                    "application/octet-stream" if binary else "application/json",
                )
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def current(self):
                return dict(
                    state["release"],
                    assets=[
                        {
                            "id": i,
                            "name": a["name"],
                            "size": len(a["data"]),
                            "digest": None
                            if a.get("legacy")
                            else "sha256:" + hashlib.sha256(a["data"]).hexdigest(),
                        }
                        for i, a in state["assets"].items()
                    ],
                )

            def do_GET(self):
                if state.get("probe_arm_get") and "/releases/1/assets" in self.path:
                    state["probe_arm_get"] = False
                    state["release"]["body"] = state.pop("probe_arm_body", None) or "Concurrent maintainer edit after Release lookup"
                path = unquote(urlparse(self.path).path)
                if "/git/ref/tags/" in path:
                    return (
                        self.send(
                            200, {"object": {"type": "commit", "sha": state["tag"]}}
                        )
                        if state["tag"]
                        else self.send(404, {"message": "Not Found"})
                    )
                if path.endswith("/releases/1/assets"):
                    return self.send(200, self.current()["assets"])
                if "/releases/assets/" in path:
                    asset = state["assets"][int(path.rsplit("/", 1)[1])]
                    asset["downloads"] = asset.get("downloads", 0) + 1
                    return self.send(200, asset["data"])
                if path.endswith("/releases"):
                    page = parse_qs(urlparse(self.path).query).get("page", ["1"])[0]
                    releases = (
                        [self.current()] if page == "1" and state["release"] else []
                    )
                    return self.send(200, releases)
                if "/releases/tags/" in path:
                    return (
                        self.send(200, self.current())
                        if state["release"] and not state["release"].get("draft")
                        else self.send(404, {"message": "Not Found"})
                    )
                if path.endswith("/releases/1"):
                    return (
                        self.send(200, self.current())
                        if state["release"]
                        else self.send(404, {"message": "Not Found"})
                    )
                self.send(404, {"message": "Not Found"})

            def do_POST(self):
                path = urlparse(self.path).path
                data = self.rfile.read(int(self.headers.get("Content-Length", 0)))
                state["writes"] += 1
                if path.endswith("/git/refs"):
                    state["tag"] = json.loads(data)["sha"]
                    return self.send(
                        201, {"object": {"type": "commit", "sha": state["tag"]}}
                    )
                if path.endswith("/releases"):
                    state["release"] = dict(
                        json.loads(data),
                        id=1,
                        html_url="https://github.com/UniClipboard/UniClipboard/releases/tag/fixture",
                    )
                    return self.send(201, self.current())
                if path.endswith("/releases/1/assets"):
                    name = parse_qs(urlparse(self.path).query)["name"][0]
                    i = max(state["assets"], default=0) + 1
                    state["assets"][i] = {
                        "name": name,
                        "data": data + (b"bad" if state["corrupt_upload"] else b""),
                    }
                    return self.send(201, {"id": i, "name": name})
                self.send(404, {"message": "Not Found"})

            def do_PATCH(self):
                payload = json.loads(
                    self.rfile.read(int(self.headers["Content-Length"]))
                )
                state["writes"] += 1
                state["patch_attempts"] += 1
                if state["patch_failures"]:
                    code = state["patch_failures"].pop(0)
                    if state.get("probe_patch_body") is not None:
                        state["release"]["body"] = state.pop("probe_patch_body")
                    if state.pop("probe_retag", False):
                        state["release"]["tag_name"] = "desktop/v2.0.0-build.999"
                    if state.pop("probe_applied", False):
                        state["release"].update(payload)
                    return self.send(code, {"message": f"injected {code}"})
                state["patch_payloads"].append(payload)
                state["release"].update(payload)
                self.send(200, self.current())

        server = ThreadingHTTPServer(("127.0.0.1", 0), Server)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            base = f"http://127.0.0.1:{server.server_port}"
            bundle = temp / "page"
            env = dict(
                os.environ,
                GH_TOKEN="fixture-only",
                GH_ENTERPRISE_TOKEN="fixture-only",
                GH_PROMPT_DISABLED="1",
            )
            common = [
                "--manifest",
                str(manifest_path),
                "--cli-manifest",
                str(cli_path),
                "--notes",
                str(notes),
                "--source-root",
                str(source_root),
                "--output",
                str(bundle),
                "--previous-tag",
                "v1.6.2",
                "--generated-notes",
                str(generated),
                "--fixture",
                "--api-base",
                base,
            ]

            def run(action, *extra, passed=True, error=None):
                p = subprocess.run(
                    [
                        sys.executable,
                        str(ROOT / "tools/release/github_desktop.py"),
                        action,
                        *common,
                        *extra,
                    ],
                    env=env,
                    text=True,
                    capture_output=True,
                )
                if (p.returncode == 0) != passed:
                    raise AssertionError(p.stdout + p.stderr)
                if error:
                    assert error in p.stderr, p.stderr

            run(
                "draft",
                "--api-base",
                "https://api.github.com",
                passed=False,
                error="fixture requires loopback",
            )
            run(
                "draft",
                "--development",
                passed=False,
                error="only generate a local preview",
            )
            check(
                "fixture and development modes cannot mutate production",
                state["writes"] == 0,
            )
            run("preview")
            body = (bundle / "release-notes.md").read_text(encoding="utf-8")
            check(
                "v1 page sections, bilingual markdown details, and independent CLI reference",
                all(
                    x in body
                    for x in [
                        "Detailed English item for build 1. (#101)",
                        "<details>\n<summary>中文变更日志</summary>",
                        "构建 1 的中文详细条目。 (#101)",
                        f"blob/{'a' * 40}/release/changelogs/desktop/v2.0.0-build.1.md",
                        f"blob/{'a' * 40}/release/changelogs/desktop/v2.0.0-build.1.zh.md",
                        "Updater Notes JSON",
                        "App Installation",
                        "CLI Downloads",
                        "Verification",
                        "New Contributors",
                        "@example",
                        "cli/v2.0.0-build.2",
                        "v1.6.2",
                        manifest["cli_manifest_sha256"],
                    ]
                )
                and "{{CLI_SHA256}}" not in body
                and "## v2.0.0-build.1 (desktop)" not in body
                and "Independent v2 products." not in body
                and body.count("All paired devices must use v2.") == 1,
            )
            check(
                "no unsupported mobile links or blanket signing claim",
                "TestFlight" not in body and "All releases are signed" not in body,
            )
            run("draft")
            check(
                "draft stages exact source and all attachments",
                state["tag"] == "a" * 40
                and state["release"]["draft"]
                and len(state["assets"]) == 32,
            )
            for line in (bundle / "SHA256SUMS.txt").read_text().splitlines():
                want, name = line.split("  ", 1)
                assert release.package.sha(bundle / name) == want
            check(
                "downloaded attachment checksums match local bundle",
                all(
                    a["data"] == (bundle / a["name"]).read_bytes()
                    for a in state["assets"].values()
                ),
            )
            writes = state["writes"]
            run("draft")
            check("idempotent draft has no writes", state["writes"] == writes)
            removed_id = next(iter(state["assets"]))
            del state["assets"][removed_id]
            run("draft")
            check("interrupted draft resumes missing asset", len(state["assets"]) == 32)
            run("publish", passed=False, error="receipt is missing")
            check(
                "no public page without availability receipt", state["release"]["draft"]
            )
            run("publish", "--publication", str(availability))
            check(
                "alpha publishes prerelease without latest",
                not state["release"]["draft"]
                and state["release"]["prerelease"]
                and state["release"]["make_latest"] == "false",
            )
            run(
                "publish",
                "--channel",
                "stable",
                "--publication",
                str(availability),
                passed=False,
                error="receipt is missing",
            )
            check("stable requires matching promotion", state["release"]["prerelease"])
            run(
                "publish",
                "--channel",
                "stable",
                "--publication",
                str(availability),
                "--promotion",
                str(promotion),
                "--previous-tag",
                "",
                "--generated-notes",
                "",
            )
            check(
                "stable restores immutable page context and publishes after verified promotion",
                not state["release"]["prerelease"]
                and state["release"]["make_latest"] == "true",
            )
            run(
                "publish",
                "--publication",
                str(availability),
                passed=False,
                error="cannot downgrade",
            )
            check(
                "stable cannot be downgraded to alpha",
                not state["release"]["prerelease"],
            )
            first = next(iter(state["assets"]))
            downloads = state["assets"][first]["downloads"]
            run("draft", "--channel", "stable")
            check(
                "matching GitHub digest reuses staged bytes",
                state["assets"][first]["downloads"] == downloads,
            )
            state["assets"][first]["legacy"] = True
            run("draft", "--channel", "stable")
            check(
                "missing GitHub digest uses byte read-back",
                state["assets"][first]["downloads"] == downloads + 1,
            )
            del state["assets"][first]["legacy"]
            original = state["assets"][first]["data"]
            state["assets"][first]["data"] += b"changed"
            writes = state["writes"]
            run(
                "draft",
                "--channel",
                "stable",
                passed=False,
                error="attachment byte conflict",
            )
            check(
                "changed remote asset rejected before writes", state["writes"] == writes
            )
            state["assets"][first]["data"] = original
            state["tag"] = "c" * 40
            run(
                "draft",
                "--channel",
                "stable",
                passed=False,
                error="tag source conflict",
            )
            check("source tag conflict rejected", state["writes"] == writes)
            state["tag"] = "a" * 40
            state["assets"][999] = {"name": "unexpected.txt", "data": b"extra"}
            run(
                "draft",
                "--channel",
                "stable",
                passed=False,
                error="unexpected existing",
            )
            check("extra asset rejected", state["writes"] == writes)
            del state["assets"][999]
            state.update(release=None, tag=None, assets={}, corrupt_upload=True)
            run("draft", passed=False, error="read-back mismatch")
            check(
                "upload corruption fails while page stays draft",
                state["release"]["draft"],
            )
            (catalogue / manifest["artifacts"][0]["file"]).write_bytes(b"corrupt")
            writes = state["writes"]
            run("draft", passed=False, error="digest mismatch")
            check(
                "modified local payload rejected before writes",
                state["writes"] == writes,
            )
            (catalogue / manifest["artifacts"][0]["file"]).write_bytes(
                ("fixture " + manifest["artifacts"][0]["file"]).encode()
            )
            shutil.copyfile(bundle / "release-notes.md", out / "release-notes.md")
            shutil.copyfile(
                bundle / "SHA256SUMS.txt", out / "attachment-SHA256SUMS.txt"
            )

            def run_custom(argv, passed=True, error=None):
                p = subprocess.run(
                    [
                        sys.executable,
                        str(ROOT / "tools/release/github_desktop.py"),
                        *map(str, argv),
                    ],
                    env=env,
                    text=True,
                    capture_output=True,
                )
                if (p.returncode == 0) != passed:
                    raise AssertionError(p.stdout + p.stderr)
                if error:
                    assert error in p.stderr, p.stderr
                return p

            # Build 14 -> 15 offset using exported source fixture (works in depth=1 CI
            # checkouts), update-body backfill, concurrent/retry race safety, transient
            # retry + first-failure preservation, pinned template promotion, and
            # single-byte tamper rejection.
            sha_362 = "362d312f8e43f464a10bc19ca899ac7f714b8f73"
            cat15 = temp / "cat15"
            cat15.mkdir()
            manifest15 = dict(manifest, build_number=15, source_sha=sha_362, development=False, source_clean=True)
            manifest15["artifacts"] = [dict(a) for a in manifest["artifacts"]]
            for a in manifest15["artifacts"]:
                shutil.copyfile(catalogue / a["file"], cat15 / a["file"])
            # Restore original uncorrupted first artifact bytes in cat15
            first_art = manifest15["artifacts"][0]
            first_blob = ("fixture " + first_art["file"]).encode()
            (cat15 / first_art["file"]).write_bytes(first_blob)
            first_art["sha256"] = hashlib.sha256(first_blob).hexdigest()
            first_art["size"] = len(first_blob)
            m15_path = cat15 / "release-manifest.json"
            release.save(m15_path, manifest15)
            pub15 = temp / "publication15.json"
            release.save(
                pub15,
                {
                    "status": "available",
                    "product": "desktop",
                    "version": "2.0.0",
                    "build_number": 15,
                    "manifest_sha256": release.package.sha(m15_path),
                    "fixture": True,
                },
            )
            promo15 = temp / "promotion15.json"
            release.save(
                promo15,
                {
                    "status": "promoted",
                    "channel": "desktop/stable",
                    "manifest_sha256": release.package.sha(m15_path),
                    "fixture": True,
                },
            )
            legacy_tmpl = temp / "legacy.md.tmpl"
            legacy_tmpl.write_text(
                (ROOT / ".github/release-notes/desktop.md.tmpl")
                .read_text(encoding="utf-8")
                .replace(
                    "{{CHANGELOG_FILES_SECTION}}",
                    "**Changelog Files**: [English and Chinese notes]({{NOTES_URL}})",
                ),
                encoding="utf-8",
            )
            export15 = temp / "export15"
            (export15 / "release/changelogs/desktop").mkdir(parents=True)
            (export15 / ".source-commit").write_text(sha_362 + "\n", encoding="utf-8")
            (export15 / "release/products.toml").write_text(
                '[products.desktop]\nversion = "2.0.0"\nbuild_number = 14\n',
                encoding="utf-8",
            )
            shutil.copyfile(
                ROOT / "release/changelogs/desktop/v2.0.0-build.14.md",
                export15 / "release/changelogs/desktop/v2.0.0-build.14.md",
            )
            shutil.copyfile(
                ROOT / "release/changelogs/desktop/v2.0.0-build.14.zh.md",
                export15 / "release/changelogs/desktop/v2.0.0-build.14.zh.md",
            )
            base15 = [
                "--manifest",
                m15_path,
                "--cli-manifest",
                cli_path,
                "--notes",
                notes,
                "--source-root",
                export15,
                "--fixture",
                "--api-base",
                base,
            ]
            state.update(
                tag=None,
                release=None,
                assets={},
                writes=0,
                patch_attempts=0,
                corrupt_upload=False,
                patch_failures=[],
                patch_payloads=[],
                probe_arm_get=False,
                probe_patch_body=None,
                probe_retag=False,
                probe_applied=False,
            )
            page15 = temp / "page15"
            run_custom(["draft", *base15, "--template", legacy_tmpl, "--output", page15])
            run_custom(
                [
                    "publish",
                    *base15,
                    "--template",
                    legacy_tmpl,
                    "--output",
                    page15,
                    "--publication",
                    pub15,
                ]
            )
            check(
                "legacy build 15 alpha publishes with legacy notes link",
                "[English and Chinese notes]" in state["release"]["body"],
            )
            initial_legacy_body = state["release"]["body"]

            # Concurrent/retry race safety probes on update-body before clean backfill
            import copy

            race_baseline = copy.deepcopy(state)
            for kind, status_code in [
                ("preflight_body_changed", None),
                ("retry_body_changed", 503),
                ("retry_target_retagged", 503),
                ("retry_first_patch_applied", 503),
                ("patch_404", 404),
                ("patch_403", 403),
            ]:
                state.clear()
                state.update(copy.deepcopy(race_baseline))
                if status_code is not None:
                    state["patch_failures"] = [status_code]
                if kind == "preflight_body_changed":
                    state["probe_arm_get"] = True
                elif kind == "retry_body_changed":
                    state["probe_patch_body"] = "Concurrent maintainer edit after HTTP 503"
                elif kind == "retry_target_retagged":
                    state["probe_retag"] = True
                elif kind == "retry_first_patch_applied":
                    state["probe_applied"] = True
                race_out = temp / ("race-" + kind)
                race_out.mkdir()
                expected_ok = kind == "retry_first_patch_applied"
                run_custom(
                    [
                        "update-body",
                        *base15,
                        "--channel",
                        "alpha",
                        "--output",
                        race_out,
                    ],
                    passed=expected_ok,
                )
                attempts = state["patch_attempts"] - race_baseline["patch_attempts"]
                if kind == "preflight_body_changed":
                    assert attempts == 0 and state["release"]["body"].startswith("Concurrent maintainer edit")
                    assert json.loads((race_out / "before.json").read_text())["body"] == race_baseline["release"]["body"]
                elif kind == "retry_body_changed":
                    assert attempts == 1 and state["release"]["body"].startswith("Concurrent maintainer edit")
                elif kind == "retry_target_retagged":
                    assert attempts == 1 and state["release"]["tag_name"] == "desktop/v2.0.0-build.999"
                elif kind == "retry_first_patch_applied":
                    assert attempts == 1 and "(#2427)" in state["release"]["body"]
                    rcpt = json.loads((race_out / "github-receipt.json").read_text())
                    assert rcpt["status"] == "updated" and rcpt["already_applied"] is True and rcpt["server_atomic_cas"] is False
                else:
                    assert attempts == 1 and state["release"]["body"] == race_baseline["release"]["body"]
            check("update-body preflight/retry recheck, already-applied readback, and non-retryable 4xx abort pass", True)
            state.clear()
            state.update(copy.deepcopy(race_baseline))

            backfill_out = temp / "backfill15"
            backfill_out.mkdir()
            state["patch_failures"] = [503]
            state["patch_payloads"].clear()
            run_custom(
                [
                    "update-body",
                    *base15,
                    "--channel",
                    "alpha",
                    "--output",
                    backfill_out,
                ]
            )
            first_failure_bytes = (backfill_out / "first-failure.json").read_bytes()
            first_failure = json.loads(first_failure_bytes)
            backfilled_body = state["release"]["body"]
            check(
                "update-body retries transient 503, preserves first-failure, sends single-key body PATCH, and extracts 362d312 build.14 markdown",
                first_failure.get("status_code") == 503
                and first_failure.get("stage") == "patch"
                and (backfill_out / "before.json").is_file()
                and len(state["patch_payloads"]) == 1
                and list(state["patch_payloads"][0].keys()) == ["body"]
                and "(#2427)" in backfilled_body
                and "(#2429)" in backfilled_body
                and "插件安装和更新现在会在后台进行" in backfilled_body
                and "<summary>中文变更日志</summary>" in backfilled_body
                and f"blob/{sha_362}/release/changelogs/desktop/v2.0.0-build.14.md" in backfilled_body
                and f"blob/{sha_362}/release/changelogs/desktop/v2.0.0-build.14.zh.md" in backfilled_body
                and "## v2.0.0-build.14 (desktop)" not in backfilled_body
                and backfilled_body.count("All paired devices must use v2.") == 1
                and len(state["assets"]) == 32,
            )
            writes_before_idempotent = state["writes"]
            run_custom(
                [
                    "update-body",
                    *base15,
                    "--channel",
                    "alpha",
                    "--output",
                    backfill_out,
                ]
            )
            check(
                "idempotent update-body performs no writes and preserves first-failure.json",
                state["writes"] == writes_before_idempotent
                and (backfill_out / "first-failure.json").read_bytes() == first_failure_bytes,
            )
            # Initial target -> operator restores legacy body during preflight must abort with 0 writes.
            backfilled_state = copy.deepcopy(state)
            rollback_out = temp / "rollback15"
            rollback_out.mkdir()
            attempts_before_rollback = state["patch_attempts"]
            state["probe_arm_get"] = True
            state["probe_arm_body"] = initial_legacy_body
            run_custom(
                [
                    "update-body",
                    *base15,
                    "--channel",
                    "alpha",
                    "--output",
                    rollback_out,
                ],
                passed=False,
                error="changed since initial read",
            )
            check(
                "operator restore from target to legacy body aborts with zero writes and preserves restored legacy body",
                state["patch_attempts"] == attempts_before_rollback
                and state["release"]["body"] == initial_legacy_body
                and json.loads((rollback_out / "before.json").read_text())["body"] == backfilled_body,
            )
            state.clear()
            state.update(backfilled_state)
            # Verify pinned backfill template identity even if main template changes later.
            modified_main_tmpl = temp / "modified-main.md.tmpl"
            modified_main_tmpl.write_text(
                (ROOT / ".github/release-notes/desktop.md.tmpl").read_text(encoding="utf-8")
                + "\nFuture main template revision.\n",
                encoding="utf-8",
            )
            mod_preview = temp / "mod-preview"
            run_custom(
                [
                    "preview",
                    *base15,
                    "--template",
                    modified_main_tmpl,
                    "--output",
                    mod_preview,
                ]
            )
            check(
                "modified main template changes new releases",
                "Future main template revision." in (mod_preview / "release-notes.md").read_text(encoding="utf-8"),
            )
            run_custom(
                [
                    "publish",
                    *base15,
                    "--channel",
                    "stable",
                    "--template",
                    legacy_tmpl,
                    "--output",
                    temp / "promote15",
                    "--publication",
                    pub15,
                    "--promotion",
                    promo15,
                ]
            )
            check(
                "backfilled build 15 promotes to stable using pinned template identity",
                not state["release"]["prerelease"]
                and state["release"]["make_latest"] == "true"
                and "Future main template revision." not in state["release"]["body"]
                and "## Prerelease Warning" not in state["release"]["body"]
                and "(#2427)" in state["release"]["body"],
            )
            # Single-byte tamper must fail closed.
            state["release"]["body"] = state["release"]["body"] + " "
            run_custom(
                [
                    "publish",
                    *base15,
                    "--channel",
                    "stable",
                    "--template",
                    legacy_tmpl,
                    "--output",
                    temp / "tamper15",
                    "--publication",
                    pub15,
                    "--promotion",
                    promo15,
                ],
                passed=False,
                error="differ from accepted inputs",
            )
            check("single-byte tampered backfilled body rejected", True)

            # Unbackfilled legacy alpha promotion compatibility.
            state.update(
                tag=None,
                release=None,
                assets={},
                writes=0,
                corrupt_upload=False,
                patch_failures=[],
                patch_payloads=[],
            )
            legacy_page = temp / "legacy-only"
            run_custom(["draft", *base15, "--template", legacy_tmpl, "--output", legacy_page])
            run_custom(
                [
                    "publish",
                    *base15,
                    "--template",
                    legacy_tmpl,
                    "--output",
                    legacy_page,
                    "--publication",
                    pub15,
                ]
            )
            run_custom(
                [
                    "publish",
                    *base15,
                    "--channel",
                    "stable",
                    "--template",
                    legacy_tmpl,
                    "--output",
                    legacy_page,
                    "--publication",
                    pub15,
                    "--promotion",
                    promo15,
                ]
            )
            check(
                "unbackfilled legacy alpha promotes cleanly to legacy stable",
                not state["release"]["prerelease"]
                and "[English and Chinese notes]" in state["release"]["body"],
            )

            # Fail-closed negative checks and single-pass {{WARNINGS}} literal protection.
            neg_src = temp / "neg-source"
            neg_dir = neg_src / "release/changelogs/desktop"
            neg_dir.mkdir(parents=True)
            # Missing markdown for build 1
            run_custom(
                [
                    "preview",
                    "--manifest",
                    manifest_path,
                    "--cli-manifest",
                    cli_path,
                    "--notes",
                    notes,
                    "--fixture",
                    "--api-base",
                    base,
                    "--source-root",
                    neg_src,
                    "--output",
                    temp / "neg-missing",
                ],
                passed=False,
                error="bilingual Desktop markdown changelog is required",
            )
            # Half-missing (English only)
            (neg_dir / "v2.0.0-build.1.md").write_text(
                "## v2.0.0-build.1 (desktop) - 2026-11-27\n\n- Only English.\n",
                encoding="utf-8",
            )
            run_custom(
                [
                    "preview",
                    "--manifest",
                    manifest_path,
                    "--cli-manifest",
                    cli_path,
                    "--notes",
                    notes,
                    "--fixture",
                    "--api-base",
                    base,
                    "--source-root",
                    neg_src,
                    "--output",
                    temp / "neg-half",
                ],
                passed=False,
                error="incomplete bilingual Desktop changelog",
            )
            # Placeholder marker rejected
            (neg_dir / "v2.0.0-build.1.zh.md").write_text(
                "## v2.0.0-build.1 (desktop) - 2026-11-27\n\n<!-- placeholder -->\n",
                encoding="utf-8",
            )
            run_custom(
                [
                    "preview",
                    "--manifest",
                    manifest_path,
                    "--cli-manifest",
                    cli_path,
                    "--notes",
                    notes,
                    "--fixture",
                    "--api-base",
                    base,
                    "--source-root",
                    neg_src,
                    "--output",
                    temp / "neg-placeholder",
                ],
                passed=False,
                error="contains forbidden marker",
            )
            # Wrong heading rejected
            (neg_dir / "v2.0.0-build.1.zh.md").write_text(
                "## Wrong heading\n\n- 中文。\n",
                encoding="utf-8",
            )
            run_custom(
                [
                    "preview",
                    "--manifest",
                    manifest_path,
                    "--cli-manifest",
                    cli_path,
                    "--notes",
                    notes,
                    "--fixture",
                    "--api-base",
                    base,
                    "--source-root",
                    neg_src,
                    "--output",
                    temp / "neg-heading",
                ],
                passed=False,
                error="unexpected changelog heading",
            )
            # products.toml build > manifest build rejected (no cross-version/future fallback)
            toml_bad = temp / "bad-toml-src"
            (toml_bad / "release/changelogs/desktop").mkdir(parents=True)
            (toml_bad / "release/products.toml").write_text(
                '[products.desktop]\nversion = "2.0.0"\nbuild_number = 16\n',
                encoding="utf-8",
            )
            (toml_bad / "release/changelogs/desktop/v2.0.0-build.16.md").write_text(
                "## v2.0.0-build.16 (desktop) - 2026-11-27\n\n- Future item.\n",
                encoding="utf-8",
            )
            (toml_bad / "release/changelogs/desktop/v2.0.0-build.16.zh.md").write_text(
                "## v2.0.0-build.16 (desktop) - 2026-11-27\n\n- 未来条目。\n",
                encoding="utf-8",
            )
            run_custom(
                [
                    "preview",
                    *base15,
                    "--source-root",
                    toml_bad,
                    "--output",
                    temp / "neg-future-build",
                ],
                passed=False,
                error="bilingual Desktop markdown changelog is required",
            )
            check("fail-closed negative changelog cases rejected", True)
            # Single-pass literal {{WARNINGS}} in markdown body is not re-expanded
            lit_src = temp / "lit-src"
            lit_dir = lit_src / "release/changelogs/desktop"
            lit_dir.mkdir(parents=True)
            (lit_dir / "v2.0.0-build.1.md").write_text(
                "## v2.0.0-build.1 (desktop) - 2026-11-27\n\n- Literal {{WARNINGS}} token stays literal.\n",
                encoding="utf-8",
            )
            (lit_dir / "v2.0.0-build.1.zh.md").write_text(
                "## v2.0.0-build.1 (desktop) - 2026-11-27\n\n- 保留字面量 {{WARNINGS}}。\n",
                encoding="utf-8",
            )
            lit_out = temp / "lit-out"
            run_custom(
                [
                    "preview",
                    "--manifest",
                    manifest_path,
                    "--cli-manifest",
                    cli_path,
                    "--notes",
                    notes,
                    "--fixture",
                    "--api-base",
                    base,
                    "--source-root",
                    lit_src,
                    "--output",
                    lit_out,
                ]
            )
            lit_body = (lit_out / "release-notes.md").read_text(encoding="utf-8")
            check(
                "single-pass template replacement preserves literal {{WARNINGS}} in changelog body",
                "Literal {{WARNINGS}} token stays literal." in lit_body
                and "保留字面量 {{WARNINGS}}。" in lit_body
                and "## Prerelease Warning" in lit_body,
            )

            # Git history offset read, shallow checkout missing-object rejection, and
            # exported source directory nested inside a shallow Git checkout.
            hist_repo = temp / "hist-repo"
            (hist_repo / "release/changelogs/desktop").mkdir(parents=True)
            subprocess.check_call(["git", "init", "-b", "main", str(hist_repo)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            subprocess.check_call(["git", "-C", str(hist_repo), "config", "user.name", "Test"], stdout=subprocess.DEVNULL)
            subprocess.check_call(["git", "-C", str(hist_repo), "config", "user.email", "test@example.com"], stdout=subprocess.DEVNULL)
            (hist_repo / "release/products.toml").write_text(
                '[products.desktop]\nversion = "2.0.0"\nbuild_number = 14\n',
                encoding="utf-8",
            )
            shutil.copyfile(
                ROOT / "release/changelogs/desktop/v2.0.0-build.14.md",
                hist_repo / "release/changelogs/desktop/v2.0.0-build.14.md",
            )
            shutil.copyfile(
                ROOT / "release/changelogs/desktop/v2.0.0-build.14.zh.md",
                hist_repo / "release/changelogs/desktop/v2.0.0-build.14.zh.md",
            )
            subprocess.check_call(["git", "-C", str(hist_repo), "add", "."], stdout=subprocess.DEVNULL)
            subprocess.check_call(["git", "-C", str(hist_repo), "commit", "-m", "build 14"], stdout=subprocess.DEVNULL)
            hist_sha = subprocess.check_output(["git", "-C", str(hist_repo), "rev-parse", "HEAD"], text=True).strip()
            (hist_repo / "release/products.toml").write_text(
                '[products.desktop]\nversion = "2.0.0"\nbuild_number = 15\n',
                encoding="utf-8",
            )
            (hist_repo / "release/changelogs/desktop/v2.0.0-build.15.md").write_text(
                "## v2.0.0-build.15 (desktop) - 2026-11-27\n\n- WRONG HEAD ITEM\n",
                encoding="utf-8",
            )
            (hist_repo / "release/changelogs/desktop/v2.0.0-build.15.zh.md").write_text(
                "## v2.0.0-build.15 (desktop) - 2026-11-27\n\n- 错误工作区条目\n",
                encoding="utf-8",
            )
            subprocess.check_call(["git", "-C", str(hist_repo), "add", "."], stdout=subprocess.DEVNULL)
            subprocess.check_call(["git", "-C", str(hist_repo), "commit", "-m", "build 15"], stdout=subprocess.DEVNULL)

            hist_cat = temp / "hist-cat"
            hist_cat.mkdir()
            hist_manifest = dict(manifest15, source_sha=hist_sha)
            for a in hist_manifest["artifacts"]:
                shutil.copyfile(cat15 / a["file"], hist_cat / a["file"])
            hist_m_path = hist_cat / "release-manifest.json"
            release.save(hist_m_path, hist_manifest)

            hist_preview = temp / "hist-preview"
            run_custom(
                [
                    "preview",
                    "--manifest",
                    hist_m_path,
                    "--cli-manifest",
                    cli_path,
                    "--notes",
                    notes,
                    "--source-root",
                    hist_repo,
                    "--fixture",
                    "--api-base",
                    base,
                    "--output",
                    hist_preview,
                ]
            )
            hist_body = (hist_preview / "release-notes.md").read_text(encoding="utf-8")
            assert "(#2427)" in hist_body and "WRONG HEAD ITEM" not in hist_body

            shallow_repo = temp / "shallow-repo"
            subprocess.check_call(
                ["git", "clone", "--depth", "1", "file://" + str(hist_repo), str(shallow_repo)],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            run_custom(
                [
                    "preview",
                    "--manifest",
                    hist_m_path,
                    "--cli-manifest",
                    cli_path,
                    "--notes",
                    notes,
                    "--source-root",
                    shallow_repo,
                    "--fixture",
                    "--api-base",
                    base,
                    "--output",
                    temp / "shallow-rejected",
                ],
                passed=False,
                error="is not available in Git checkout",
            )
            nested_export = shallow_repo / "dist/catalogue/source"
            (nested_export / "release/changelogs/desktop").mkdir(parents=True)
            (nested_export / ".source-commit").write_text(hist_sha + "\n", encoding="utf-8")
            (nested_export / "release/products.toml").write_text(
                '[products.desktop]\nversion = "2.0.0"\nbuild_number = 14\n',
                encoding="utf-8",
            )
            shutil.copyfile(
                ROOT / "release/changelogs/desktop/v2.0.0-build.14.md",
                nested_export / "release/changelogs/desktop/v2.0.0-build.14.md",
            )
            shutil.copyfile(
                ROOT / "release/changelogs/desktop/v2.0.0-build.14.zh.md",
                nested_export / "release/changelogs/desktop/v2.0.0-build.14.zh.md",
            )
            nested_out = temp / "nested-export-out"
            run_custom(
                [
                    "preview",
                    "--manifest",
                    hist_m_path,
                    "--cli-manifest",
                    cli_path,
                    "--notes",
                    notes,
                    "--source-root",
                    nested_export,
                    "--fixture",
                    "--api-base",
                    base,
                    "--output",
                    nested_out,
                ]
            )
            nested_body = (nested_out / "release-notes.md").read_text(encoding="utf-8")
            assert "(#2427)" in nested_body and "WRONG HEAD ITEM" not in nested_body
            (nested_export / ".source-commit").write_text("b" * 40 + "\n", encoding="utf-8")
            run_custom(
                [
                    "preview",
                    "--manifest",
                    hist_m_path,
                    "--cli-manifest",
                    cli_path,
                    "--notes",
                    notes,
                    "--source-root",
                    nested_export,
                    "--fixture",
                    "--api-base",
                    base,
                    "--output",
                    temp / "nested-bad-commit",
                ],
                passed=False,
                error="does not match",
            )
            check(
                "git historical build offset read, shallow checkout rejection, and nested exported source dir pass",
                True,
            )
            report["status"] = "passed"
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)
            check("isolated HTTP server stopped", not thread.is_alive())
except BaseException:
    report["status"] = "failed"
    raise
finally:
    report["source_files"] = {
        str(p.relative_to(ROOT)): release.package.sha(p)
        for p in [
            Path(__file__),
            ROOT / "tools/release/github_desktop.py",
            ROOT / ".github/release-notes/desktop.md.tmpl",
        ]
        if p.exists()
    }
    (out / "report.json").write_text(
        json.dumps(report, indent=2) + "\n", encoding="utf-8"
    )
    (out / "SHA256SUMS").write_text(
        "".join(
            release.package.sha(p) + "  " + p.name + "\n"
            for p in sorted(out.iterdir())
            if p.is_file() and p.name != "SHA256SUMS"
        ),
        encoding="utf-8",
    )
