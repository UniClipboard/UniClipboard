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
            "corrupt_upload": False,
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
                        {"id": i, "name": a["name"], "size": len(a["data"])}
                        for i, a in state["assets"].items()
                    ],
                )

            def do_GET(self):
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
                    return self.send(
                        200, state["assets"][int(path.rsplit("/", 1)[1])]["data"]
                    )
                if "/releases/tags/" in path or path.endswith("/releases/1"):
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
                state["writes"] += 1
                state["release"].update(
                    json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                )
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
                "v1 page sections and independent CLI reference",
                all(
                    x in body
                    for x in [
                        "App Installation",
                        "CLI Downloads",
                        "Verification",
                        "New Contributors",
                        "@example",
                        "cli/v2.0.0-build.2",
                        "v1.6.2",
                    ]
                ),
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
            shutil.copyfile(bundle / "release-notes.md", out / "release-notes.md")
            shutil.copyfile(
                bundle / "SHA256SUMS.txt", out / "attachment-SHA256SUMS.txt"
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
