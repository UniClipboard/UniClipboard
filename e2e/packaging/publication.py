#!/usr/bin/env python3
"""Exercise real release subprocesses and boto3 against disposable HTTP/S3 services."""

import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
from urllib.parse import unquote, urlparse

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser()
p.add_argument("--output", default=str(ROOT / ".cache/publication-e2e"))
a = p.parse_args()
out = Path(a.output).resolve()
out.mkdir(parents=True, exist_ok=True)
report = {
    "status": "running",
    "checks": [],
    "scope": "real boto3 conditional writes and release HTTP process fixture; no production credentials or mutations",
}
sys.path.insert(0, str(ROOT / "tools/release"))
import desktop_release as release  # noqa: E402 - repository tool path

report["source_sha"] = subprocess.check_output(
    ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
).strip()
report["source_files"] = {
    str(f.relative_to(ROOT)): hashlib.sha256(f.read_bytes()).hexdigest()
    for f in [
        ROOT / "tools/release/desktop_release.py",
        ROOT / "tools/release/requirements.txt",
        Path(__file__),
    ]
}

try:
    with tempfile.TemporaryDirectory(prefix="unic-publication-e2e-") as temporary:
        temp = Path(temporary)
        records = temp / "records"
        records.mkdir()
        metadata = temp / "products.toml"
        metadata.write_text(
            'schema=1\n[products.desktop]\nversion="2.0.0"\nbuild_number=1\ncli_manifest_sha256="'
            + "b" * 64
            + '"\ntargets='
            + json.dumps(release.TARGETS)
            + "\n"
        )
        for target in release.TARGETS:
            for kind in release.FORMATS[target.split("-")[0]]:
                directory = records / target / kind
                directory.mkdir(parents=True)
                name = target + "." + kind
                blob = ("fixture " + name).encode()
                (directory / name).write_bytes(blob)
                release.save(
                    directory / "artifacts.json",
                    {
                        "schema": 1,
                        "product": "desktop",
                        "version": "2.0.0",
                        "build_number": 1,
                        "source_sha": "a" * 40,
                        "source_clean": False,
                        "development": True,
                        "cli": {"manifest_sha256": "b" * 64},
                        "target": target,
                        "acceptance": {"native": True, "version_smoke": True},
                        "artifacts": [
                            {
                                "file": name,
                                "format": kind,
                                "sha256": hashlib.sha256(blob).hexdigest(),
                                "size": len(blob),
                                "signature": "fixture-only",
                            }
                        ],
                    },
                )
        objects, registrations, writes = {}, [], []
        revision = 0
        channel_build = None
        contract = "product-version-build-v1"
        inject = None
        npm_tag = "alpha"
        npm_packages = {}

        class Server(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def send(self, status, data, content_type="application/json"):
                if isinstance(data, (dict, list)):
                    data = json.dumps(data).encode()
                self.send_response(status)
                self.send_header("Content-Type", content_type)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def access(self):
                return (
                    self.headers.get("CF-Access-Client-Id") == "fixture-id"
                    and self.headers.get("CF-Access-Client-Secret") == "fixture-secret"
                )

            def do_GET(self):
                path = unquote(urlparse(self.path).path)
                if path.startswith("/api/") and not self.access():
                    return self.send(403, {})
                if path in ("/api/contract", "/contract"):
                    return self.send(
                        200, {"buildIdentity": contract, "products": ["desktop"]}
                    )
                if path[1:] in npm_packages:
                    item = npm_packages[path[1:]]
                    return self.send(
                        200,
                        {
                            "versions": {
                                item["version"]: {
                                    "dist": {"integrity": item["integrity"]}
                                }
                            },
                            "dist-tags": {npm_tag: item["version"]},
                        },
                    )
                if path in (
                    "/api/channels/desktop/alpha",
                    "/api/channels/desktop/stable",
                ):
                    return self.send(
                        200,
                        {
                            "data": {
                                "internalRevision": revision,
                                "currentVersion": "2.0.0" if channel_build else None,
                                "currentBuildNumber": channel_build,
                            }
                        },
                    )
                if path in ("/v1/desktop/alpha.json", "/v1/desktop/stable.json"):
                    m = registrations[-1]
                    return self.send(
                        200,
                        {
                            k: m[k]
                            for k in ["product", "version", "buildNumber", "artifacts"]
                        },
                    )
                key = (
                    path.removeprefix("/bucket/")
                    if path.startswith("/bucket/")
                    else path.lstrip("/")
                )
                if key not in objects:
                    return self.send(
                        404, b"<Error><Code>NoSuchKey</Code></Error>", "application/xml"
                    )
                return self.send(200, objects[key], "application/octet-stream")

            def do_PUT(self):
                global inject
                assert self.headers.get("Authorization", "").startswith(
                    "AWS4-HMAC-SHA256 "
                )
                assert self.headers.get("If-None-Match") == "*"
                key = unquote(urlparse(self.path).path).removeprefix("/bucket/")
                body = self.rfile.read(int(self.headers["Content-Length"]))
                if inject == key:
                    objects[key] = b"racing-conflict"
                    inject = None
                if key in objects:
                    return self.send(
                        412,
                        b"<Error><Code>PreconditionFailed</Code></Error>",
                        "application/xml",
                    )
                objects[key] = body
                writes.append(key)
                return self.send(200, b"", "application/xml")

            def do_POST(self):
                global revision
                if not self.access():
                    return self.send(403, {})
                data = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                path = unquote(self.path)
                if path == "/api/releases/register":
                    assert len(data["artifacts"]) == 6
                    if registrations and data != registrations[0]:
                        return self.send(409, {})
                    if not registrations:
                        registrations.append(data)
                    return self.send(201, {"data": {"id": "desktop/2.0.0/build.1"}})
                if path in (
                    "/api/channels/desktop/alpha/promote",
                    "/api/channels/desktop/stable/promote",
                ):
                    channel = path.removeprefix("/api/channels/").removesuffix(
                        "/promote"
                    )
                    if data["expectedRevisions"][channel] != revision:
                        return self.send(409, {})
                    revision += 1
                    return self.send(200, {"data": {"internalRevision": revision}})
                return self.send(404, {})

        server = ThreadingHTTPServer(("127.0.0.1", 0), Server)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f"http://127.0.0.1:{server.server_port}"
        env = dict(
            os.environ,
            AWS_ACCESS_KEY_ID="fixture-id",
            AWS_SECRET_ACCESS_KEY="fixture-secret",
            AWS_EC2_METADATA_DISABLED="true",
            CF_ACCESS_CLIENT_ID="fixture-id",
            CF_ACCESS_CLIENT_SECRET="fixture-secret",
        )
        tool = ROOT / "tools/release/desktop_release.py"

        def run(args, passed=True, error=None):
            r = subprocess.run(
                [sys.executable, str(tool), *map(str, args)],
                cwd=ROOT,
                env=env,
                text=True,
                capture_output=True,
                timeout=90,
            )
            if (r.returncode == 0) != passed:
                raise RuntimeError(r.stderr[-3000:])
            if error and error not in r.stderr:
                raise RuntimeError(r.stderr[-3000:])

        catalogue = temp / "catalogue"
        run(
            [
                "assemble",
                "--input",
                records,
                "--output",
                catalogue,
                "--metadata",
                metadata,
                "--development",
            ]
        )
        manifest = catalogue / "release-manifest.json"
        report["checks"].append("complete target/format catalogue assembled")
        notes = temp / "notes.json"
        release.save(notes, {"en": "Fixture", "zh": "Fixture"})
        publication = temp / "publication.json"
        command = [
            "publish",
            "--manifest",
            manifest,
            "--fixture",
            "--admin",
            base,
            "--public",
            base,
            "--endpoint",
            base,
            "--bucket",
            "bucket",
            "--notes",
            notes,
            "--published-at",
            "2026-11-13T00:00:00Z",
            "--receipt",
            publication,
        ]
        # The production path must reject this development fixture before contacting services.
        production = command.copy()
        production.remove("--fixture")
        run(production, passed=False)
        assert not writes and not registrations
        report["checks"].append(
            "development candidate rejected before production writes"
        )
        contract = "legacy"
        run(command, passed=False)
        assert not writes
        report["checks"].append("legacy service contract rejected before writes")
        contract = "product-version-build-v1"
        run(command)
        assert (
            len(writes)
            == 2 + sum(len(release.FORMATS[t.split("-")[0]]) for t in release.TARGETS)
            and len(registrations) == 1
        )
        report["checks"].append(
            "all 22 packages, catalogue and publication receipt uploaded and read back"
        )
        previous = len(writes)
        run(command)
        assert len(writes) == previous
        report["checks"].append(
            "retry preserves exact immutable bytes without overwrite"
        )
        first = next(iter(objects))
        original = objects[first]
        objects[first] = b"conflict"
        run(command, passed=False)
        assert len(writes) == previous
        report["checks"].append("existing conflicting object rejected before mutation")
        objects[first] = original
        run(command)
        promote = [
            "promote",
            "--manifest",
            manifest,
            "--publication",
            publication,
            "--fixture",
            "--admin",
            base,
            "--public",
            base,
            "--channel",
            "alpha",
            "--expected-revision",
            "0",
            "--receipt",
            temp / "promotion.json",
        ]
        # Only a current successful read-back can authorize promotion.
        run(promote)
        assert revision == 1
        report["checks"].append(
            "revision-bound promotion and public numbered identity read-back"
        )
        run(promote, passed=False)
        assert revision == 1
        report["checks"].append("stale revision rejected")
        cli_path = temp / "cli.json"
        cli = {
            "schema": 1,
            "product": "cli",
            "version": "2.0.0",
            "build_number": 1,
            "npm_version": "2.0.0-build.1",
            "targets": release.TARGETS,
            "packages": [
                {
                    "name": name,
                    "version": "2.0.0-build.1",
                    "integrity": "sha512-fixture",
                }
                for name in [
                    *(release.package.cli.package_name(t) for t in release.TARGETS),
                    "@uniclipboard/cli",
                ]
            ],
        }
        release.save(cli_path, cli)
        npm_packages.update((a["name"], a) for a in cli["packages"])
        catalogue_json = release.package.cli.read_json(manifest)
        catalogue_json["cli_manifest_sha256"] = release.package.sha(cli_path)
        release.save(manifest, catalogue_json)
        availability = release.package.cli.read_json(publication)
        availability["manifest_sha256"] = release.package.sha(manifest)
        release.save(publication, availability)
        acceptance_path = temp / "acceptance.json"
        acceptance = {
            platform: {
                "manifest_sha256": availability["manifest_sha256"],
                "evidence": "https://example.com/native-acceptance/" + platform,
                "artifacts": [
                    {
                        "file": a["file"],
                        "sha256": a["sha256"],
                        "package_lifecycle": "passed",
                        "native_gui": "passed",
                    }
                    for a in catalogue_json["artifacts"]
                    if a["target"].split("-")[0] == platform
                ],
            }
            for platform in release.FORMATS
        }
        stable = promote.copy()
        stable[stable.index("--channel") + 1] = "stable"
        stable[stable.index("--expected-revision") + 1] = "1"
        run(
            stable,
            passed=False,
            error="stable requires three native acceptance receipts",
        )
        stable += [
            "--cli-manifest",
            cli_path,
            "--registry",
            base,
            "--acceptance",
            acceptance_path,
        ]
        release.save(
            acceptance_path, {k: v for k, v in acceptance.items() if k != "linux"}
        )
        run(stable, passed=False, error="stable requires one receipt each")
        release.save(acceptance_path, acceptance)
        run(
            stable,
            passed=False,
            error="npm latest does not select the locked CLI package",
        )
        npm_tag = "latest"
        acceptance["linux"]["artifacts"][0]["sha256"] = "0" * 64
        release.save(acceptance_path, acceptance)
        run(stable, passed=False, error="invalid stable acceptance artifact")
        assert revision == 1
        acceptance["linux"]["artifacts"][0]["sha256"] = next(
            a["sha256"]
            for a in catalogue_json["artifacts"]
            if a["file"] == acceptance["linux"]["artifacts"][0]["file"]
        )
        release.save(acceptance_path, acceptance)
        run(stable)
        assert revision == 2
        report["checks"].append(
            "stable requires exact receipts for all platforms and latest on all locked CLI packages"
        )
        joint = stable + ["--forward-only"]
        joint[joint.index("--expected-revision") + 1] = "auto"
        run(joint)
        assert revision == 3
        channel_build = "1"
        run(joint)
        assert revision == 3
        channel_build = "2"
        run(joint, passed=False, error="joint release cannot downgrade")
        assert revision == 3
        report["checks"].append(
            "joint promotion reads the revision, resumes an exact selection and rejects a downgrade"
        )
        objects.clear()
        writes.clear()
        registrations.clear()
        catalogue_file = catalogue_json["artifacts"][0]["file"]
        conflict_key = f"artifacts/desktop/2.0.0/build.1/{catalogue_file}"
        inject = conflict_key
        publication.unlink()
        run(command, passed=False, error="conflicting immutable object")
        assert objects[conflict_key] == b"racing-conflict"
        assert (
            conflict_key not in writes
            and not registrations
            and not any(key.endswith("/publication.json") for key in objects)
            and release.package.cli.read_json(publication)["status"] == "failed"
        )
        report["checks"].append("racing conditional writer cannot overwrite an object")
        server.shutdown()
        server.server_close()
        report["status"] = "passed"
except BaseException as error:
    report["status"] = "failed"
    report["error"] = str(error)
    raise
finally:
    release.save(out / "report.json", report)
    (out / "SHA256SUMS").write_text(
        hashlib.sha256((out / "report.json").read_bytes()).hexdigest()
        + "  report.json\n"
    )
print(out / "report.json")
