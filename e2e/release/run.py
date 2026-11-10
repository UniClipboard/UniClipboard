#!/usr/bin/env python3
"""Real packager/npm processes with a disposable loopback registry."""

import argparse
import base64
import copy
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile
import threading
from urllib.parse import unquote, urlparse

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser()
p.add_argument("--output", default=str(ROOT / ".cache/release-e2e"))
a = p.parse_args()
out = Path(a.output).resolve()
out.mkdir(parents=True, exist_ok=True)
report = {
    "checks": [],
    "commands": [],
    "status": "running",
    "not_run": [
        "production npm/OIDC/provenance",
        "native Windows/Linux hosts",
        "GUI",
        "Store",
        "GitCode",
        "real GitHub release APIs (command fixture only)",
    ],
}


def source_identity():
    paths = [
        p
        for directory in [
            "tools/release",
            "tools/package",
            "internal/releaseinfo",
            "e2e/release",
            "release",
        ]
        for p in (ROOT / directory).rglob("*")
        if p.is_file() and "__pycache__" not in str(p)
    ]
    paths.extend(
        ROOT / name
        for name in [
            "go.mod",
            "go.sum",
            "LICENSE",
            "app/service/cmd/unic/main.go",
            "app/service/cmd/unicd/main.go",
            "app/desktop/cmd/unic-desktop/main.go",
            ".github/workflows/build-cli.yml",
            ".github/workflows/release-cli.yml",
            ".github/workflows/promote.yml",
        ]
    )
    return {
        str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
        for p in sorted(set(paths))
    }


initial_source = source_identity()
packages = {}
blobs = {}
uploads = []
tag_writes = []
fail_tag = None


class Registry(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def send(self, value, status=200, binary=False):
        data = value if binary else json.dumps(value).encode()
        self.send_response(status)
        self.send_header(
            "Content-Type", "application/octet-stream" if binary else "application/json"
        )
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        key = unquote(urlparse(self.path).path.lstrip("/"))
        if key.startswith("tarballs/"):
            data = blobs.get(key)
            self.send(
                data if data is not None else {},
                200 if data is not None else 404,
                binary=data is not None,
            )
        elif key.startswith("-/package/") and key.endswith("/dist-tags"):
            name = key[len("-/package/") : -len("/dist-tags")]
            self.send(packages.get(name, {}).get("dist-tags", {}))
        elif key in packages:
            self.send(packages[key])
        else:
            self.send({"error": "not found"}, 404)

    def do_PUT(self):
        global fail_tag
        key = unquote(urlparse(self.path).path.lstrip("/"))
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        if key.startswith("-/package/") and "/dist-tags/" in key:
            name, tag = key[len("-/package/") :].split("/dist-tags/")
            if fail_tag == tag:
                fail_tag = None
                return self.send({"error": "injected tag failure"}, 503)
            if body not in packages.get(name, {}).get("versions", {}):
                return self.send({"error": "version missing"}, 404)
            packages[name]["dist-tags"][tag] = body
            tag_writes.append((name, tag, body))
            return self.send({"ok": True})
        if fail_tag == "publish":
            fail_tag = None
            return self.send({"error": "injected package upload failure"}, 503)
        name = body["name"]
        entry = packages.setdefault(
            name, {"name": name, "versions": {}, "dist-tags": {}}
        )
        for version, manifest in body["versions"].items():
            if version in entry["versions"]:
                return self.send({"error": "immutable version exists"}, 409)
            payload = base64.b64decode(
                next(iter(body["_attachments"].values()))["data"]
            )
            integrity = (
                "sha512-" + base64.b64encode(hashlib.sha512(payload).digest()).decode()
            )
            if manifest["dist"]["integrity"] != integrity:
                return self.send({"error": "integrity mismatch"}, 400)
            blob_key = f"tarballs/{name}/{version}.tgz"
            blobs[blob_key] = payload
            manifest["dist"]["tarball"] = base + "/" + blob_key
            entry["versions"][version] = manifest
            uploads.append((name, version))
        entry["dist-tags"].update(body["dist-tags"])
        self.send({"ok": True}, 201)


server = ThreadingHTTPServer(("127.0.0.1", 0), Registry)
base = f"http://127.0.0.1:{server.server_port}"
threading.Thread(target=server.serve_forever, daemon=True).start()


def check(label, condition):
    report["checks"].append({"name": label, "passed": bool(condition)})
    if not condition:
        raise AssertionError(label)
    print("PASS " + label, flush=True)


def execute(argv, expected=0):
    r = subprocess.run(
        [str(x) for x in argv],
        cwd=ROOT,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    report["commands"].append(
        {
            "argv": [str(x).replace(str(temp), "<TEMP>") for x in argv],
            "exit_code": r.returncode,
            "stdout": r.stdout[-2000:],
            "stderr": r.stderr[-2000:],
        }
    )
    if (r.returncode == 0) != (expected == 0):
        raise AssertionError(f"command exit {r.returncode}: {argv}\n{r.stderr}")
    return r


try:
    with tempfile.TemporaryDirectory(prefix="unic-release-e2e-") as temporary:
        temp = Path(temporary)
        env = {
            k: v
            for k, v in os.environ.items()
            if not k.startswith(
                ("UNIC_", "NPM_TOKEN", "NODE_AUTH_TOKEN", "npm_config_", "NPM_CONFIG_")
            )
        }
        config = temp / "npmrc"
        config.write_text(
            f"registry={base}\n//127.0.0.1:{server.server_port}/:_authToken=local-fixture-only\nfetch-retries=0\naudit=false\nfund=false\ncache={temp}/npm-cache\n"
        )
        env["NPM_CONFIG_USERCONFIG"] = str(config)
        tool = ROOT / "tools/release/release.py"
        builder = temp / "packager"
        execute(["go", "build", "-o", builder, "./tools/package"])
        operating_system = {"Darwin": "darwin", "Linux": "linux", "Windows": "windows"}[
            platform.system()
        ]
        architecture = {
            "arm64": "arm64",
            "aarch64": "arm64",
            "x86_64": "amd64",
            "AMD64": "amd64",
        }[platform.machine()]
        target = operating_system + "-" + architecture

        def build(number):
            metadata = temp / f"products-{number}.toml"
            metadata.write_text(
                f'schema = 1\n[products.cli]\nversion = "9.7.0"\nbuild_number = {number}\ntargets = ["{target}"]\n'
            )
            native = temp / f"native-{number}"
            execute(
                [builder, "-product", "cli", "-metadata", metadata, "-output", native]
            )
            execute([sys.executable, tool, "smoke", "--directory", native])
            dist = temp / f"npm-{number}"
            execute(
                [
                    sys.executable,
                    tool,
                    "assemble",
                    "--metadata",
                    metadata,
                    "--input",
                    native,
                    "--output",
                    dist,
                ]
            )
            return dist / "release-manifest.json"

        first = build(41)
        manifest = json.loads(first.read_text())
        report["candidate"] = manifest

        def distribute(command, path, channel, expected=0):
            receipt = temp / f"{command}-{channel}-receipt.json"
            execute(
                [
                    sys.executable,
                    tool,
                    command,
                    "--manifest",
                    path,
                    "--channel",
                    channel,
                    "--registry",
                    base,
                    "--receipt",
                    receipt,
                ],
                expected,
            )
            return json.loads(receipt.read_text())

        check(
            "alpha publication available",
            distribute("publish", first, "alpha")["status"] == "available",
        )
        check(
            "platform package publishes before main",
            [name for name, v in uploads] == [x["name"] for x in manifest["packages"]],
        )
        installed = temp / "installed with spaces"
        execute(
            [
                "npm",
                "install",
                "--prefix",
                installed,
                "@uniclipboard/cli@alpha",
                "--registry",
                base,
                "--ignore-scripts",
                "--no-audit",
                "--no-fund",
            ]
        )
        for command in ["unic", "uniclip"]:
            executable = installed / "node_modules" / ".bin" / command
            value = json.loads(execute([executable, "--version", "--json"]).stdout)
            check(
                command + " executes installed exact build",
                value["version"] == "9.7.0" and value["build_number"] == "41",
            )
            failure = execute([executable, "daemon"], 1)
            check(command + " preserves failing exit code", failure.returncode == 1)
        before = len(uploads)
        alpha = copy.deepcopy(
            {name: p["dist-tags"]["alpha"] for name, p in packages.items()}
        )
        check(
            "stable promotion available",
            distribute("promote", first, "stable")["status"] == "available",
        )
        check("promotion does not upload packages", len(uploads) == before)
        check(
            "promotion preserves alpha",
            all(
                packages[name]["dist-tags"]["alpha"] == version
                for name, version in alpha.items()
            ),
        )
        check(
            "idempotent promotion reuses packages",
            distribute("promote", first, "stable")["status"] == "available"
            and len(uploads) == before,
        )
        changed = copy.deepcopy(manifest)
        changed["packages"][0]["integrity"] = "sha512-conflict"
        bad = temp / "changed.json"
        bad.write_text(json.dumps(changed))
        before_tags = len(tag_writes)
        check(
            "integrity conflict rejects before tag mutation",
            distribute("promote", bad, "stable", 1)["status"] == "failed"
            and len(tag_writes) == before_tags,
        )
        name = manifest["packages"][0]["name"]
        removed = packages[name]["versions"].pop(manifest["npm_version"])
        check(
            "missing target rejects complete promotion",
            distribute("promote", first, "stable", 1)["status"] == "failed"
            and len(tag_writes) == before_tags,
        )
        packages[name]["versions"][manifest["npm_version"]] = removed
        second = build(42)
        changed = json.loads(second.read_text())
        changed["packages"][0]["integrity"] = "sha512-wrong-unpublished-integrity"
        bad = second.parent / "wrong-integrity.json"
        bad.write_text(json.dumps(changed))
        before_uploads = len(uploads)
        check(
            "invalid local integrity rejects before any upload",
            distribute("publish", bad, "stable", 1)["status"] == "failed"
            and len(uploads) == before_uploads,
        )
        fail_tag = "publish"
        check(
            "partial destination failure is failed",
            distribute("publish", second, "stable", 1)["status"] == "failed",
        )
        check(
            "main stable stays on previous build after platform failure",
            packages["@uniclipboard/cli"]["dist-tags"]["latest"] == "9.7.0-build.41",
        )
        check(
            "retry completes direct stable publication",
            distribute("publish", second, "stable")["status"] == "available",
        )
        before_tags = len(tag_writes)
        check(
            "older build cannot replace stable",
            distribute("promote", first, "stable", 1)["status"] == "failed"
            and len(tag_writes) == before_tags,
        )
        # Isolated gh processes exercise orchestration; they do not contact GitHub.
        fixture = temp / "github"
        fixture.mkdir()
        (fixture / "bin").mkdir()
        shim = fixture / "bin" / "gh"
        shim.write_text(
            "#!"
            + sys.executable
            + "\n"
            + (ROOT / "e2e/release/github_fixture.py").read_text()
        )
        shim.chmod(0o755)
        env["PATH"] = str(shim.parent) + os.pathsep + env["PATH"]
        env["UNIC_GITHUB_FIXTURE"] = str(fixture)
        staged = temp / "staged"
        shutil.copytree(first.parent, staged)
        staged_manifest = staged / "release-manifest.json"
        value = json.loads(staged_manifest.read_text())
        value["source_clean"] = (
            True  # Fixture only; never submitted to a public registry.
        )
        staged_manifest.write_text(json.dumps(value))
        output = temp / "github-output"
        resumed = temp / "resumed"
        resume = [
            sys.executable,
            tool,
            "resume",
            "--metadata",
            temp / "products-41.toml",
            "--output",
            resumed,
            "--github-output",
            output,
        ]
        execute(resume)
        check(
            "missing durable record selects a new build",
            output.read_text().strip() == "staged=false",
        )
        record = [sys.executable, tool, "record", "--manifest", staged_manifest]
        execute(record)
        state = json.loads((fixture / "state.json").read_text())
        check(
            "durable staging creates the verified source tag first",
            state["tag"]["sha"] == value["source_sha"] and state["release"],
        )
        execute(record)
        check(
            "durable staging retry accepts identical existing assets",
            staged_manifest.read_bytes()
            == (fixture / "assets/release-manifest.json").read_bytes(),
        )
        execute(resume)
        check(
            "release retry restores exact staged package bytes",
            output.read_text().splitlines()[-1] == "staged=true"
            and all(
                (resumed / p["file"]).read_bytes() == (staged / p["file"]).read_bytes()
                for p in value["packages"]
            ),
        )
        bad_asset = fixture / "assets" / value["packages"][0]["file"]
        bad_asset.write_bytes(b"changed")
        execute(record, 1)
        check(
            "durable record rejects changed existing bytes",
            bad_asset.read_bytes() == b"changed",
        )
        report["status"] = "passed"
except BaseException as error:
    report["status"] = "failed"
    report["error"] = str(error)
    raise
finally:
    server.shutdown()
    report["source_sha"] = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    report["source_files"] = source_identity()
    report["source_unchanged"] = report["source_files"] == initial_source
    if not report["source_unchanged"]:
        report["status"] = "failed"
        report["error"] = "source changed during acceptance"
    text = json.dumps(report, indent=2).replace(base, "http://127.0.0.1:<PORT>") + "\n"
    (out / "report.json").write_text(text)
    (out / "SHA256SUMS").write_text(
        hashlib.sha256(text.encode()).hexdigest() + "  report.json\n"
    )
    print(
        json.dumps(
            {
                "status": report["status"],
                "checks": len(report["checks"]),
                "artifact": str(out),
            }
        )
    )

if report["status"] != "passed":
    sys.exit(1)
