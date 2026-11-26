#!/usr/bin/env python3
"""Offline process E2E and unit tests for tools/release/gitcode_mirror.py."""

from __future__ import annotations

import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import unittest
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools/release/gitcode_mirror.py"
sys.path.insert(0, str(ROOT / "tools/release"))
import gitcode_mirror as mirror  # noqa: E402

ACCESS_ID = "access-client-id-secret"
ACCESS_SECRET = "access-client-secret-value"
GH_TOKEN = "gh-fixture-token"
REPO = "UniClipboard/UniClipboard"

DESKTOP_UPDATERS = [
    ("darwin-amd64", "app.tar.gz", "uniclipboard-2.0.0-build.11-darwin-amd64.app.tar.gz", 101, "1" * 64),
    ("darwin-arm64", "app.tar.gz", "uniclipboard-2.0.0-build.11-darwin-arm64.app.tar.gz", 102, "2" * 64),
    ("linux-amd64", "update.tar.gz", "uniclipboard-2.0.0-build.11-linux-amd64.update.tar.gz", 103, "3" * 64),
    ("linux-arm64", "update.tar.gz", "uniclipboard-2.0.0-build.11-linux-arm64.update.tar.gz", 104, "4" * 64),
    ("windows-amd64", "update.tar.gz", "uniclipboard-2.0.0-build.11-windows-amd64.update.tar.gz", 105, "5" * 64),
    ("windows-arm64", "update.tar.gz", "uniclipboard-2.0.0-build.11-windows-arm64.update.tar.gz", 106, "6" * 64),
]

ANDROID_APK = ("unic-android-3.0.0-206-arm64-v8a.apk", 20600, "a" * 64)


def run_cli(args: list[str], env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    merged_env = {"PATH": os.environ.get("PATH", "")}
    if env:
        merged_env.update(env)
    return subprocess.run(
        [sys.executable, "-B", str(SCRIPT), *args],
        text=True,
        capture_output=True,
        env=merged_env,
        check=False,
    )


def make_desktop_release_fixture(
    *,
    tag: str = "desktop/v2.0.0-build.11",
    version: str = "2.0.0",
    build_number: int = 11,
    draft: bool = False,
    prerelease: bool = True,
    development: bool = False,
    source_clean: bool = True,
    updaters=DESKTOP_UPDATERS,
    tamper_asset_digest: str | None = None,
) -> tuple[dict, dict[int, bytes]]:
    manifest_artifacts = [
        {
            "target": "darwin-arm64",
            "format": "dmg",
            "file": f"uniclipboard-{version}-build.{build_number}-darwin-arm64.dmg",
            "size": 999,
            "sha256": "f" * 64,
            "updater": False,
        }
    ]
    for target, fmt, filename, size, sha in updaters:
        manifest_artifacts.append(
            {
                "target": target,
                "format": fmt,
                "file": filename,
                "size": size,
                "sha256": sha,
                "updater": True,
            }
        )
    manifest = {
        "schema": 1,
        "product": "desktop",
        "version": version,
        "build_number": build_number,
        "source_sha": "b" * 40,
        "cli_manifest_sha256": "c" * 64,
        "source_clean": source_clean,
        "development": development,
        "artifacts": manifest_artifacts,
    }
    manifest_bytes = json.dumps(manifest).encode("utf-8")
    manifest_sha = hashlib.sha256(manifest_bytes).hexdigest()
    assets = [
        {
            "id": 1000,
            "name": "release-manifest.json",
            "size": len(manifest_bytes),
            "digest": f"sha256:{manifest_sha}",
        }
    ]
    asset_blobs = {1000: manifest_bytes}
    for idx, (_target, _fmt, filename, size, sha) in enumerate(updaters, start=1001):
        assets.append(
            {
                "id": idx,
                "name": filename,
                "size": size,
                "digest": f"sha256:{'e' * 64 if filename == tamper_asset_digest else sha}",
            }
        )
    release = {
        "tag_name": tag,
        "draft": draft,
        "prerelease": prerelease,
        "assets": assets,
    }
    return release, asset_blobs


def make_android_release_fixture(
    *,
    tag: str = "android/v3.0.0-build.206",
    version: str = "3.0.0",
    build_number: int = 206,
    draft: bool = False,
    prerelease: bool = True,
    source_clean: bool = True,
    tamper_apk_digest: bool = False,
) -> tuple[dict, dict[int, bytes]]:
    filename, size, sha = ANDROID_APK
    candidate = {
        "schema": 1,
        "platform": "android",
        "format": "apk",
        "version": version,
        "build_number": build_number,
        "source_sha": "d" * 40,
        "source_clean": source_clean,
        "application_id": "app.uniclipboard.android",
        "architecture": "arm64-v8a",
        "artifact": {
            "name": filename,
            "bytes": size,
            "sha256": sha,
        },
    }
    cand_bytes = json.dumps(candidate).encode("utf-8")
    cand_sha = hashlib.sha256(cand_bytes).hexdigest()
    assets = [
        {
            "id": 2000,
            "name": "candidate.json",
            "size": len(cand_bytes),
            "digest": f"sha256:{cand_sha}",
        },
        {
            "id": 2001,
            "name": filename,
            "size": size,
            "digest": f"sha256:{'9' * 64 if tamper_apk_digest else sha}",
        },
    ]
    release = {
        "tag_name": tag,
        "draft": draft,
        "prerelease": prerelease,
        "assets": assets,
    }
    return release, {2000: cand_bytes}


class FakeGitHubServer:
    def __init__(self, releases: dict[str, dict], asset_blobs: dict[int, bytes]):
        self.releases = releases
        self.asset_blobs = asset_blobs
        self.saw_redirect_auth = False

        outer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_GET(self):
                if self.path.startswith("/signed-download/"):
                    if self.headers.get("Authorization"):
                        outer.saw_redirect_auth = True
                    asset_id = int(self.path.rsplit("/", 1)[-1])
                    blob = outer.asset_blobs.get(asset_id)
                    if blob is None:
                        self.send_response(404)
                        self.end_headers()
                        return
                    self.send_response(200)
                    self.send_header("Content-Type", "application/octet-stream")
                    self.send_header("Content-Length", str(len(blob)))
                    self.end_headers()
                    self.wfile.write(blob)
                    return

                if self.headers.get("Authorization") != f"Bearer {GH_TOKEN}":
                    self.send_response(401)
                    self.end_headers()
                    return

                tag_prefix = f"/repos/{REPO}/releases/tags/"
                asset_prefix = f"/repos/{REPO}/releases/assets/"
                if self.path.startswith(tag_prefix):
                    tag = unquote(self.path[len(tag_prefix) :])
                    rel = outer.releases.get(tag)
                    if rel is None:
                        self.send_response(404)
                        self.end_headers()
                        return
                    body = json.dumps(rel).encode("utf-8")
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                    return

                if self.path.startswith(asset_prefix):
                    asset_id = int(self.path[len(asset_prefix) :])
                    if asset_id not in outer.asset_blobs:
                        self.send_response(404)
                        self.end_headers()
                        return
                    # Mimic GitHub redirecting asset downloads to object storage.
                    self.send_response(302)
                    self.send_header("Location", f"{outer.base_url}/signed-download/{asset_id}")
                    self.end_headers()
                    return

                self.send_response(404)
                self.end_headers()

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.base_url = f"http://127.0.0.1:{self.server.server_address[1]}"
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class FakeFlareReleaseServer:
    def __init__(self, status_for=None, state_for=None):
        self.status_for = status_for or (lambda _filename, _attempt: 200)
        self.state_for = state_for or (lambda _filename: "ready")
        self.registrations: list[dict] = []
        self.attempts: dict[str, int] = {}

        outer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_PUT(self):
                if self.path != "/api/mirrors":
                    self.send_response(404)
                    self.end_headers()
                    return
                if (
                    self.headers.get("CF-Access-Client-Id") != ACCESS_ID
                    or self.headers.get("CF-Access-Client-Secret") != ACCESS_SECRET
                ):
                    body = json.dumps({"error": "Cloudflare Access identity required"}).encode()
                    self.send_response(401)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                    return

                length = int(self.headers.get("Content-Length", "0"))
                payload = json.loads(self.rfile.read(length).decode("utf-8"))
                filename = str(payload["filename"])
                attempt = outer.attempts.get(filename, 0) + 1
                outer.attempts[filename] = attempt
                status = outer.status_for(filename, attempt)

                if status in (301, 302, 307, 308):
                    self.send_response(status)
                    self.send_header("Location", "https://evil.example.com/steal")
                    self.end_headers()
                    return
                if status != 200:
                    body = json.dumps({"error": "Mirror sha256 does not match the artifact"}).encode()
                    self.send_response(status)
                    self.send_header("Content-Type", "application/json")
                    self.send_header("Content-Length", str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                    return

                outer.registrations.append(payload)
                body = json.dumps({"data": {"state": outer.state_for(filename), **payload}}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.base_url = f"http://127.0.0.1:{self.server.server_address[1]}"
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()


class GitCodeMirrorUnitTests(unittest.TestCase):
    def test_tag_parsing_and_gitcode_mapping(self):
        self.assertEqual(mirror.parse_release_tag("desktop/v2.0.0-build.11"), ("desktop", "2.0.0", 11))
        self.assertEqual(mirror.parse_release_tag("android/v3.0.0-build.206"), ("android", "3.0.0", 206))
        self.assertEqual(mirror.to_gitcode_tag("desktop", "2.0.0", 11), "desktop-v2.0.0-build.11")
        self.assertEqual(mirror.to_gitcode_tag("android", "3.0.0", 206), "android-v3.0.0-build.206")
        for bad in (
            "cli/v2.0.0-build.10",
            "v1.6.2",
            "desktop/v2.0.0",
            "desktop/v2.0.0-build.0",
            "desktop/../v2.0.0-build.1",
            "nix/v2.0.0-build.1",
        ):
            with self.assertRaises(mirror.MirrorError):
                mirror.parse_release_tag(bad)

    def test_registrable_url_policy(self):
        self.assertTrue(
            mirror.is_registrable_mirror_url(
                "https://gitcode.com/UniClipboard/UniClipboard/releases/download/desktop-v2.0.0-build.11/a.app.tar.gz"
            )
        )
        self.assertTrue(
            mirror.is_registrable_mirror_url(
                "https://cdn.atomgit.com/UniClipboard/UniClipboard/releases/download/desktop-v2.0.0-build.11/a.app.tar.gz"
            )
        )
        for bad in (
            "http://gitcode.com/o/r/releases/download/t/a",
            "https://gitcode.com/o/r/releases/download/t/a?access_token=secret",
            "https://gitcode.com/o/r/releases/download/t/a#frag",
            "https://user:pass@gitcode.com/o/r/releases/download/t/a",
            "https://gitcode.com:8443/o/r/releases/download/t/a",
            "https://evil-gitcode.com/o/r/releases/download/t/a",
            "https://evil.example.com/a",
        ):
            self.assertFalse(mirror.is_registrable_mirror_url(bad), bad)


class GitCodeMirrorPrepareTests(unittest.TestCase):
    def test_prepare_desktop_release_extracts_six_updaters_and_strips_redirect_auth(self):
        rel, blobs = make_desktop_release_fixture()
        gh = FakeGitHubServer({"desktop/v2.0.0-build.11": rel}, blobs)
        self.addCleanup(gh.close)
        with tempfile.TemporaryDirectory(prefix="unic-mirror-prep-") as tmp:
            plan_path = Path(tmp) / "plan.json"
            out_path = Path(tmp) / "github_output.txt"
            res = run_cli(
                [
                    "prepare",
                    "--repo",
                    REPO,
                    "--tag-name",
                    "desktop/v2.0.0-build.11",
                    "--api-base",
                    gh.base_url,
                    "--output",
                    str(plan_path),
                    "--github-output",
                    str(out_path),
                    "--allow-local-http",
                ],
                env={"GH_TOKEN": GH_TOKEN},
            )
            self.assertEqual(res.returncode, 0, res.stderr)
            self.assertFalse(gh.saw_redirect_auth, "Authorization leaked to redirect host")

            plan = json.loads(plan_path.read_text(encoding="utf-8"))
            self.assertEqual(plan["product"], "desktop")
            self.assertEqual(plan["version"], "2.0.0")
            self.assertEqual(plan["buildNumber"], "11")
            self.assertEqual(plan["tagName"], "desktop/v2.0.0-build.11")
            self.assertEqual(plan["gitcodeTag"], "desktop-v2.0.0-build.11")
            self.assertTrue(plan["prerelease"])
            self.assertEqual(len(plan["artifacts"]), 6)
            self.assertEqual(
                [a["filename"] for a in plan["artifacts"]],
                [u[2] for u in DESKTOP_UPDATERS],
            )
            self.assertEqual(
                plan["artifacts"][2]["url"],
                "https://release.uniclipboard.app/artifacts/desktop/2.0.0/build.11/uniclipboard-2.0.0-build.11-linux-amd64.update.tar.gz",
            )

            outputs = dict(
                line.split("=", 1) for line in out_path.read_text(encoding="utf-8").splitlines()
            )
            self.assertEqual(outputs["product"], "desktop")
            self.assertEqual(outputs["gitcode_tag"], "desktop-v2.0.0-build.11")
            self.assertEqual(outputs["prerelease"], "true")
            sources = json.loads(outputs["sources"])
            self.assertEqual(len(sources), 6)
            for item in sources:
                self.assertEqual(set(item.keys()), {"url", "sha256"})

    def test_prepare_android_release_extracts_apk(self):
        rel, blobs = make_android_release_fixture(prerelease=False)
        gh = FakeGitHubServer({"android/v3.0.0-build.206": rel}, blobs)
        self.addCleanup(gh.close)
        with tempfile.TemporaryDirectory(prefix="unic-mirror-prep-android-") as tmp:
            plan_path = Path(tmp) / "plan.json"
            out_path = Path(tmp) / "github_output.txt"
            res = run_cli(
                [
                    "prepare",
                    "--repo",
                    REPO,
                    "--tag-name",
                    "android/v3.0.0-build.206",
                    "--api-base",
                    gh.base_url,
                    "--output",
                    str(plan_path),
                    "--github-output",
                    str(out_path),
                    "--allow-local-http",
                ],
                env={"GH_TOKEN": GH_TOKEN},
            )
            self.assertEqual(res.returncode, 0, res.stderr)
            plan = json.loads(plan_path.read_text(encoding="utf-8"))
            self.assertEqual(plan["product"], "android")
            self.assertEqual(plan["version"], "3.0.0")
            self.assertEqual(plan["buildNumber"], "206")
            self.assertEqual(plan["gitcodeTag"], "android-v3.0.0-build.206")
            self.assertFalse(plan["prerelease"])
            self.assertEqual(len(plan["artifacts"]), 1)
            self.assertEqual(plan["artifacts"][0]["filename"], ANDROID_APK[0])
            self.assertEqual(
                plan["artifacts"][0]["url"],
                f"https://release.uniclipboard.app/artifacts/android/3.0.0/build.206/{ANDROID_APK[0]}",
            )

    def test_prepare_rejects_draft_dirty_and_tampered_releases(self):
        cases = [
            ("draft", make_desktop_release_fixture(draft=True), "desktop/v2.0.0-build.11"),
            ("development", make_desktop_release_fixture(development=True), "desktop/v2.0.0-build.11"),
            ("dirty", make_desktop_release_fixture(source_clean=False), "desktop/v2.0.0-build.11"),
            (
                "incomplete",
                make_desktop_release_fixture(updaters=DESKTOP_UPDATERS[:5]),
                "desktop/v2.0.0-build.11",
            ),
            (
                "digest_mismatch",
                make_desktop_release_fixture(tamper_asset_digest=DESKTOP_UPDATERS[0][2]),
                "desktop/v2.0.0-build.11",
            ),
            ("android_draft", make_android_release_fixture(draft=True), "android/v3.0.0-build.206"),
            (
                "android_digest_mismatch",
                make_android_release_fixture(tamper_apk_digest=True),
                "android/v3.0.0-build.206",
            ),
        ]
        for label, (rel, blobs), tag in cases:
            with self.subTest(label=label):
                gh = FakeGitHubServer({tag: rel}, blobs)
                self.addCleanup(gh.close)
                with tempfile.TemporaryDirectory(prefix="unic-mirror-bad-") as tmp:
                    res = run_cli(
                        [
                            "prepare",
                            "--repo",
                            REPO,
                            "--tag-name",
                            tag,
                            "--api-base",
                            gh.base_url,
                            "--output",
                            str(Path(tmp) / "plan.json"),
                            "--allow-local-http",
                        ],
                        env={"GH_TOKEN": GH_TOKEN},
                    )
                    self.assertNotEqual(res.returncode, 0, f"expected failure for {label}")


class GitCodeMirrorRegisterTests(unittest.TestCase):
    def _setup_files(
        self,
        tmp: Path,
        receipt_files: list[dict],
        *,
        product: str = "desktop",
        version: str = "2.0.0",
        build_number: str = "11",
        artifacts: list[dict] | None = None,
    ) -> tuple[Path, Path, Path, Path]:
        if artifacts is None:
            artifacts = [
                {
                    "filename": DESKTOP_UPDATERS[0][2],
                    "size": DESKTOP_UPDATERS[0][3],
                    "sha256": DESKTOP_UPDATERS[0][4],
                },
                {
                    "filename": DESKTOP_UPDATERS[2][2],
                    "size": DESKTOP_UPDATERS[2][3],
                    "sha256": DESKTOP_UPDATERS[2][4],
                },
            ]
        receipt = tmp / "receipt.json"
        plan = tmp / "plan.json"
        provenance = tmp / "provenance.json"
        summary = tmp / "summary.md"
        receipt.write_text(json.dumps({"schema": 1, "files": receipt_files}), encoding="utf-8")
        plan.write_text(
            json.dumps(
                {
                    "schema": 1,
                    "product": product,
                    "version": version,
                    "buildNumber": build_number,
                    "tagName": f"{product}/v{version}-build.{build_number}",
                    "gitcodeTag": f"{product}-v{version}-build.{build_number}",
                    "artifacts": artifacts,
                }
            ),
            encoding="utf-8",
        )
        return receipt, plan, provenance, summary

    def _good_entries(self) -> tuple[dict, dict]:
        a_name, a_size, a_sha = DESKTOP_UPDATERS[0][2], DESKTOP_UPDATERS[0][3], DESKTOP_UPDATERS[0][4]
        b_name, b_size, b_sha = DESKTOP_UPDATERS[2][2], DESKTOP_UPDATERS[2][3], DESKTOP_UPDATERS[2][4]
        return (
            {
                "name": a_name,
                "status": "mirrored",
                "size": a_size,
                "sha256": a_sha,
                "downloadUrl": f"https://gitcode.com/UniClipboard/UniClipboard/releases/download/desktop-v2.0.0-build.11/{a_name}",
            },
            {
                "name": b_name,
                "status": "reused",
                "size": b_size,
                "sha256": b_sha,
                "downloadUrl": f"https://atomgit.com/UniClipboard/UniClipboard/releases/download/desktop-v2.0.0-build.11/{b_name}",
            },
        )

    def test_registers_mirrored_and_reused_desktop_and_android_with_build_number(self):
        fake = FakeFlareReleaseServer()
        self.addCleanup(fake.close)
        good_a, good_b = self._good_entries()
        with tempfile.TemporaryDirectory(prefix="unic-mirror-reg-") as tmp:
            receipt, plan, prov, summary = self._setup_files(Path(tmp), [good_a, good_b])
            res = run_cli(
                [
                    "register",
                    "--receipt",
                    str(receipt),
                    "--plan",
                    str(plan),
                    "--source",
                    "github-actions:UniClipboard/UniClipboard:42",
                    "--provenance",
                    str(prov),
                    "--retry-delay-ms",
                    "0",
                    "--allow-local-http",
                ],
                env={
                    "FLARE_RELEASE_ADMIN_URL": fake.base_url,
                    "FLARE_RELEASE_ACCESS_CLIENT_ID": ACCESS_ID,
                    "FLARE_RELEASE_ACCESS_CLIENT_SECRET": ACCESS_SECRET,
                    "GITHUB_STEP_SUMMARY": str(summary),
                },
            )
            self.assertEqual(res.returncode, 0, res.stderr + res.stdout)
            self.assertEqual(len(fake.registrations), 2)
            self.assertEqual(
                fake.registrations[0],
                {
                    "product": "desktop",
                    "version": "2.0.0",
                    "buildNumber": "11",
                    "filename": good_a["name"],
                    "provider": "gitcode",
                    "downloadUrl": good_a["downloadUrl"],
                    "size": good_a["size"],
                    "sha256": good_a["sha256"],
                    "source": "github-actions:UniClipboard/UniClipboard:42",
                },
            )
            self.assertEqual(fake.registrations[1]["buildNumber"], "11")
            provenance = json.loads(prov.read_text(encoding="utf-8"))
            self.assertEqual([r["status"] for r in provenance], ["registered", "registered"])
            self.assertIn("desktop 2.0.0 build 11", summary.read_text(encoding="utf-8"))

        # Android registration with buildNumber
        fake_android = FakeFlareReleaseServer()
        self.addCleanup(fake_android.close)
        apk_name, apk_size, apk_sha = ANDROID_APK
        apk_url = f"https://gitcode.com/UniClipboard/UniClipboard/releases/download/android-v3.0.0-build.206/{apk_name}"
        with tempfile.TemporaryDirectory(prefix="unic-mirror-reg-android-") as tmp:
            receipt, plan, prov, _ = self._setup_files(
                Path(tmp),
                [
                    {
                        "name": apk_name,
                        "status": "mirrored",
                        "size": apk_size,
                        "sha256": apk_sha,
                        "downloadUrl": apk_url,
                    }
                ],
                product="android",
                version="3.0.0",
                build_number="206",
                artifacts=[{"filename": apk_name, "size": apk_size, "sha256": apk_sha}],
            )
            res = run_cli(
                [
                    "register",
                    "--receipt",
                    str(receipt),
                    "--plan",
                    str(plan),
                    "--provenance",
                    str(prov),
                    "--retry-delay-ms",
                    "0",
                    "--allow-local-http",
                ],
                env={
                    "FLARE_RELEASE_ADMIN_URL": fake_android.base_url,
                    "FLARE_RELEASE_ACCESS_CLIENT_ID": ACCESS_ID,
                    "FLARE_RELEASE_ACCESS_CLIENT_SECRET": ACCESS_SECRET,
                },
            )
            self.assertEqual(res.returncode, 0, res.stderr + res.stdout)
            self.assertEqual(len(fake_android.registrations), 1)
            self.assertEqual(fake_android.registrations[0]["product"], "android")
            self.assertEqual(fake_android.registrations[0]["buildNumber"], "206")

    def test_partial_failure_still_registers_valid_entries_and_exits_nonzero(self):
        fake = FakeFlareReleaseServer()
        self.addCleanup(fake.close)
        good_a, good_b = self._good_entries()
        with tempfile.TemporaryDirectory(prefix="unic-mirror-partial-") as tmp:
            receipt, plan, prov, _ = self._setup_files(
                Path(tmp),
                [
                    good_a,
                    {"name": good_b["name"], "status": "failed", "stage": "upload", "error": "timed out"},
                ],
            )
            res = run_cli(
                [
                    "register",
                    "--receipt",
                    str(receipt),
                    "--plan",
                    str(plan),
                    "--provenance",
                    str(prov),
                    "--retry-delay-ms",
                    "0",
                    "--allow-local-http",
                ],
                env={
                    "FLARE_RELEASE_ADMIN_URL": fake.base_url,
                    "FLARE_RELEASE_ACCESS_CLIENT_ID": ACCESS_ID,
                    "FLARE_RELEASE_ACCESS_CLIENT_SECRET": ACCESS_SECRET,
                },
            )
            self.assertEqual(res.returncode, 1)
            self.assertEqual([r["filename"] for r in fake.registrations], [good_a["name"]])
            self.assertIn(good_b["name"], res.stdout)

    def test_refuses_mismatched_digest_size_missing_entry_and_unsafe_urls(self):
        fake = FakeFlareReleaseServer()
        self.addCleanup(fake.close)
        good_a, good_b = self._good_entries()

        bad_receipts = [
            ("bad_sha", [{**good_a, "sha256": "c" * 64}, good_b], [good_b["name"]]),
            ("bad_size", [{**good_a, "size": 999999}, good_b], [good_b["name"]]),
            ("missing_entry", [good_a], [good_a["name"]]),
            (
                "bad_urls",
                [
                    {**good_a, "downloadUrl": f"https://evil.example.com/{good_a['name']}"},
                    {**good_b, "downloadUrl": f"{good_b['downloadUrl']}?access_token=tok"},
                ],
                [],
            ),
        ]
        for label, entries, expected_registered in bad_receipts:
            with self.subTest(label=label):
                fake.registrations.clear()
                with tempfile.TemporaryDirectory(prefix="unic-mirror-refuse-") as tmp:
                    receipt, plan, _, _ = self._setup_files(Path(tmp), entries)
                    res = run_cli(
                        [
                            "register",
                            "--receipt",
                            str(receipt),
                            "--plan",
                            str(plan),
                            "--retry-delay-ms",
                            "0",
                            "--allow-local-http",
                        ],
                        env={
                            "FLARE_RELEASE_ADMIN_URL": fake.base_url,
                            "FLARE_RELEASE_ACCESS_CLIENT_ID": ACCESS_ID,
                            "FLARE_RELEASE_ACCESS_CLIENT_SECRET": ACCESS_SECRET,
                        },
                    )
                    self.assertEqual(res.returncode, 1)
                    self.assertEqual([r["filename"] for r in fake.registrations], expected_registered)

    def test_retries_transient_503_and_does_not_retry_permanent_422_or_redirects(self):
        good_a, good_b = self._good_entries()

        # Transient 503 -> succeeds on 3rd attempt
        fake_retry = FakeFlareReleaseServer(status_for=lambda _f, attempt: 503 if attempt < 3 else 200)
        self.addCleanup(fake_retry.close)
        with tempfile.TemporaryDirectory(prefix="unic-mirror-retry-") as tmp:
            receipt, plan, _, _ = self._setup_files(Path(tmp), [good_a, good_b])
            res = run_cli(
                [
                    "register",
                    "--receipt",
                    str(receipt),
                    "--plan",
                    str(plan),
                    "--retry-delay-ms",
                    "0",
                    "--allow-local-http",
                ],
                env={
                    "FLARE_RELEASE_ADMIN_URL": fake_retry.base_url,
                    "FLARE_RELEASE_ACCESS_CLIENT_ID": ACCESS_ID,
                    "FLARE_RELEASE_ACCESS_CLIENT_SECRET": ACCESS_SECRET,
                },
            )
            self.assertEqual(res.returncode, 0, res.stderr)
            self.assertEqual(fake_retry.attempts[good_a["name"]], 3)
            self.assertEqual(len(fake_retry.registrations), 2)

        # Permanent 422 -> no retry
        fake_perm = FakeFlareReleaseServer(
            status_for=lambda fname, _attempt: 422 if fname == good_a["name"] else 200
        )
        self.addCleanup(fake_perm.close)
        with tempfile.TemporaryDirectory(prefix="unic-mirror-perm-") as tmp:
            receipt, plan, _, _ = self._setup_files(Path(tmp), [good_a, good_b])
            res = run_cli(
                [
                    "register",
                    "--receipt",
                    str(receipt),
                    "--plan",
                    str(plan),
                    "--retry-delay-ms",
                    "0",
                    "--allow-local-http",
                ],
                env={
                    "FLARE_RELEASE_ADMIN_URL": fake_perm.base_url,
                    "FLARE_RELEASE_ACCESS_CLIENT_ID": ACCESS_ID,
                    "FLARE_RELEASE_ACCESS_CLIENT_SECRET": ACCESS_SECRET,
                },
            )
            self.assertEqual(res.returncode, 1)
            self.assertEqual(fake_perm.attempts[good_a["name"]], 1)
            self.assertEqual([r["filename"] for r in fake_perm.registrations], [good_b["name"]])

        # Redirect -> rejected without following
        fake_redir = FakeFlareReleaseServer(status_for=lambda _f, _attempt: 302)
        self.addCleanup(fake_redir.close)
        with tempfile.TemporaryDirectory(prefix="unic-mirror-redir-") as tmp:
            receipt, plan, _, _ = self._setup_files(Path(tmp), [good_a])
            res = run_cli(
                [
                    "register",
                    "--receipt",
                    str(receipt),
                    "--plan",
                    str(plan),
                    "--attempts",
                    "1",
                    "--retry-delay-ms",
                    "0",
                    "--allow-local-http",
                ],
                env={
                    "FLARE_RELEASE_ADMIN_URL": fake_redir.base_url,
                    "FLARE_RELEASE_ACCESS_CLIENT_ID": ACCESS_ID,
                    "FLARE_RELEASE_ACCESS_CLIENT_SECRET": ACCESS_SECRET,
                },
            )
            self.assertEqual(res.returncode, 1)
            self.assertEqual(fake_redir.registrations, [])

    def test_requires_https_credentials_valid_receipt_and_redacts_secrets(self):
        fake = FakeFlareReleaseServer(status_for=lambda _f, _a: 401)
        self.addCleanup(fake.close)
        good_a, good_b = self._good_entries()
        with tempfile.TemporaryDirectory(prefix="unic-mirror-sec-") as tmp:
            receipt, plan, prov, summary = self._setup_files(Path(tmp), [good_a, good_b])

            # Missing credentials -> fails before contacting server
            res = run_cli(
                ["register", "--receipt", str(receipt), "--plan", str(plan), "--allow-local-http"],
                env={"FLARE_RELEASE_ADMIN_URL": fake.base_url},
            )
            self.assertEqual(res.returncode, 1)
            self.assertIn("FLARE_RELEASE_ACCESS_CLIENT_ID", res.stderr)
            self.assertEqual(fake.attempts, {})

            # HTTP rejected without --allow-local-http
            res = run_cli(
                ["register", "--receipt", str(receipt), "--plan", str(plan)],
                env={
                    "FLARE_RELEASE_ADMIN_URL": fake.base_url,
                    "FLARE_RELEASE_ACCESS_CLIENT_ID": ACCESS_ID,
                    "FLARE_RELEASE_ACCESS_CLIENT_SECRET": ACCESS_SECRET,
                },
            )
            self.assertEqual(res.returncode, 1)
            self.assertEqual(fake.attempts, {})

            # Secrets redacted on failure
            res = run_cli(
                [
                    "register",
                    "--receipt",
                    str(receipt),
                    "--plan",
                    str(plan),
                    "--provenance",
                    str(prov),
                    "--retry-delay-ms",
                    "0",
                    "--allow-local-http",
                ],
                env={
                    "FLARE_RELEASE_ADMIN_URL": fake.base_url,
                    "FLARE_RELEASE_ACCESS_CLIENT_ID": "wrong-id",
                    "FLARE_RELEASE_ACCESS_CLIENT_SECRET": ACCESS_SECRET,
                    "GITHUB_STEP_SUMMARY": str(summary),
                },
            )
            self.assertEqual(res.returncode, 1)
            combined = res.stdout + res.stderr + prov.read_text(encoding="utf-8") + summary.read_text(encoding="utf-8")
            self.assertNotIn(ACCESS_SECRET, combined)

            # Empty and broken receipt fail without calling server
            fake.attempts.clear()
            receipt.write_text(json.dumps({"schema": 1, "files": []}), encoding="utf-8")
            self.assertEqual(
                run_cli(
                    ["register", "--receipt", str(receipt), "--plan", str(plan), "--allow-local-http"],
                    env={
                        "FLARE_RELEASE_ADMIN_URL": fake.base_url,
                        "FLARE_RELEASE_ACCESS_CLIENT_ID": ACCESS_ID,
                        "FLARE_RELEASE_ACCESS_CLIENT_SECRET": ACCESS_SECRET,
                    },
                ).returncode,
                1,
            )
            receipt.write_text("not json", encoding="utf-8")
            self.assertEqual(
                run_cli(
                    ["register", "--receipt", str(receipt), "--plan", str(plan), "--allow-local-http"],
                    env={
                        "FLARE_RELEASE_ADMIN_URL": fake.base_url,
                        "FLARE_RELEASE_ACCESS_CLIENT_ID": ACCESS_ID,
                        "FLARE_RELEASE_ACCESS_CLIENT_SECRET": ACCESS_SECRET,
                    },
                ).returncode,
                1,
            )
            self.assertEqual(fake.attempts, {})


if __name__ == "__main__":
    unittest.main()
