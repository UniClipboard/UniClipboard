#!/usr/bin/env python3
"""Real signed HTTP downloads and mygo installation in disposable directories."""

import argparse
import copy
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile
import threading
import time
from urllib.parse import parse_qs, urlsplit

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser()
p.add_argument("--output", default=str(ROOT / ".cache/update-e2e"))
p.add_argument(
    "--prebuilt",
    type=Path,
    help="Task fixture directory with cross-built signer/probe, fixture keys and input.json",
)
a = p.parse_args()
out = Path(a.output).resolve()
out.mkdir(parents=True, exist_ok=True)
report = {
    "status": "running",
    "checks": [],
    "scope": "signed fixture HTTP and actual mygo install; no GUI or public mutation",
}


def run(args, env=None, passed=True):
    r = subprocess.run(
        list(map(str, args)),
        cwd=ROOT,
        env=env,
        text=True,
        capture_output=True,
        timeout=180,
    )
    if (r.returncode == 0) != passed:
        raise RuntimeError(r.stderr[-3000:])
    return r.stdout


prebuilt = json.loads((a.prebuilt / "input.json").read_text()) if a.prebuilt else None
report["source_sha"] = (
    prebuilt["source_sha"] if prebuilt else run(["git", "rev-parse", "HEAD"]).strip()
)
report["source_files"] = {
    f.relative_to(ROOT).as_posix(): hashlib.sha256(f.read_bytes()).hexdigest()
    for d in [
        "app/desktop/internal/appupdates",
        "app/desktop/e2e/updateprobe",
        "tools/release/update-sign",
    ]
    for f in (ROOT / d).rglob("*.go")
}
report["source_files"]["e2e/packaging/updates.py"] = hashlib.sha256(
    Path(__file__).read_bytes()
).hexdigest()
if prebuilt:
    if prebuilt["source_files"] != report["source_files"]:
        raise ValueError("prebuilt updater fixture source mismatch")
    suffix = ".exe" if os.name == "nt" else ""
    if set(prebuilt["binaries_sha256"]) != {"sign" + suffix, "probe" + suffix}:
        raise ValueError("incomplete prebuilt updater fixture")
    for name, expected in prebuilt["binaries_sha256"].items():
        if (
            Path(name).name != name
            or hashlib.sha256((a.prebuilt / name).read_bytes()).hexdigest() != expected
        ):
            raise ValueError("prebuilt updater fixture binary mismatch")
    report["prebuilt_fixture"] = prebuilt
try:
    with tempfile.TemporaryDirectory(prefix="unic-update-e2e-") as temporary:
        temp = Path(temporary)
        signer = temp / ("sign.exe" if os.name == "nt" else "sign")
        if prebuilt:
            shutil.copyfile(a.prebuilt / signer.name, signer)
            shutil.copytree(a.prebuilt / "keys", temp / "keys")
        else:
            run(["go", "build", "-o", signer, "./tools/release/update-sign"])
            run([signer, "-keygen", temp / "keys"])
        key = (temp / "keys/public.key").read_text().strip()
        env = dict(
            os.environ,
            UNIC_UPDATE_PRIVATE_KEY=(temp / "keys/private.key").read_text().strip(),
        )
        target = (
            {"Darwin": "darwin", "Linux": "linux", "Windows": "windows"}[
                platform.system()
            ]
            + "-"
            + {
                "arm64": "arm64",
                "aarch64": "arm64",
                "ARM64": "arm64",
                "AMD64": "amd64",
                "x86_64": "amd64",
            }[platform.machine()]
        )
        report["target"] = target
        bundle = (
            temp / "Installed.app"
            if target.startswith("darwin")
            else temp / "installed"
        )
        payload = bundle / "Contents/MacOS" if target.startswith("darwin") else bundle
        payload.mkdir(parents=True)
        probe = payload / ("probe.exe" if os.name == "nt" else "probe")
        linker = f"-X github.com/egoist/mygo.production=1 -X github.com/egoist/mygo.packageUpdateKey={key} -X github.com/egoist/mygo.packageUpdateFeed=https://invalid.example/update.json"
        if prebuilt:
            if prebuilt["target"] != target:
                raise ValueError("prebuilt updater fixture target mismatch")
            shutil.copyfile(a.prebuilt / probe.name, probe)
        else:
            run(
                [
                    "go",
                    "build",
                    "-ldflags",
                    linker,
                    "-o",
                    probe,
                    "./app/desktop/e2e/updateprobe",
                ]
            )
        for marker, content, expected in [
            ("update-owner", "winget", "winget"),
            ("install.json", "{}", "Scoop"),
            ("portable.dat", "", "portable archive"),
        ]:
            (payload / marker).write_text(content)
            assert run([probe, "-owner"]).strip() == expected
            (payload / marker).unlink()
            report["checks"].append(expected + " update ownership preserved")
        # User data is outside a macOS bundle; portable data is an unowned app-directory entry.
        user = temp / "profile" if target.startswith("darwin") else payload / "data"
        user.mkdir()
        (user / "preserve.txt").write_text("synthetic-user-data")
        (payload / "obsolete.txt").write_text("old-owned-file")
        (payload / ".mygo-files").write_text(probe.name + "\nobsolete.txt\n")
        state = {}

        class Server(BaseHTTPRequestHandler):
            def log_message(self, *args):
                pass

            def do_GET(self):
                request = urlsplit(self.path)
                feed = state["feed"]
                if parse_qs(request.query).get("from") == ["2.0.0-build.1"]:
                    feed = {**feed, "notes": state.get("cumulative_notes", feed["notes"])}
                data = (
                    state["archive"]
                    if request.path == "/archive.tar.gz"
                    else json.dumps(feed).encode()
                )
                self.send_response(200)
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                try:
                    self.wfile.write(data)
                except (BrokenPipeError, ConnectionResetError):
                    pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), Server)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        base = f"http://127.0.0.1:{server.server_port}"
        identity = {
            "product": "desktop",
            "version": "2.0.0",
            "build_number": 2,
            "target": target,
        }

        def archive(identity_value=identity, duplicate=False, missing=False):
            data = io.BytesIO()
            with tarfile.open(fileobj=data, mode="w:gz") as tar:
                prefix = (
                    "Installed.app/Contents/MacOS/"
                    if target.startswith("darwin")
                    else ""
                )
                entries = [
                    ("updated.txt", b"accepted-build-2"),
                    (probe.name, probe.read_bytes()),
                ]
                if not missing:
                    entries.append(
                        ("release-identity.json", json.dumps(identity_value).encode())
                    )
                if duplicate:
                    entries.append(
                        (
                            "other/release-identity.json",
                            json.dumps(identity_value).encode(),
                        )
                    )
                for name, content in entries:
                    entry = tarfile.TarInfo(prefix + name)
                    entry.size = len(content)
                    entry.mode = 0o755 if name == probe.name else 0o644
                    tar.addfile(entry, io.BytesIO(content))
            return data.getvalue()

        def select(blob):
            state["archive"] = blob
            path = temp / "archive.tar.gz"
            path.write_bytes(blob)
            signature = json.loads(run([signer, "-file", path], env=env))
            state["feed"] = {
                "product": "desktop",
                "version": "2.0.0",
                "buildNumber": "2",
                "notes": "Fixture build",
                "artifacts": [
                    {
                        "platform": target,
                        "format": (
                            "app.tar.gz"
                            if target.startswith("darwin")
                            else "update.tar.gz"
                        ),
                        "downloadUrl": base + "/archive.tar.gz",
                        "updater": True,
                        **{k: signature[k] for k in ("sha256", "size", "signature")},
                    }
                ],
            }

        valid = archive()
        select(valid)
        original = copy.deepcopy(state["feed"])
        downloads = temp / "downloads"
        downloads.mkdir()
        probe_env = dict(
            os.environ, TMPDIR=str(downloads), TMP=str(downloads), TEMP=str(downloads)
        )
        command = [probe, "-base", base, "-key", key]
        run(command, env=probe_env)
        report["checks"].append("valid signed archive accepted")
        report["release_notes"] = []
        for language, notes, expected in [
            ("zh-CN", "Latest\n\n<!-- zh -->\n\n最新", "最新"),
            ("en", "Latest\n\n<!-- zh -->\n\n最新", "Latest"),
            ("ja", "Latest\n\n<!-- zh -->\n\n最新", "Latest"),
            ("zh-CN", "Latest\n\n<!-- zh -->\n\n", "Latest"),
            ("zh-CN", "Latest", "Latest"),
        ]:
            state["feed"]["notes"] = notes
            actual = json.loads(run(command + ["-notes", "-language", language], env=probe_env))
            report["release_notes"].append(
                {"language": language, "expected": expected, "actual": actual}
            )
            assert actual == expected, f"wrong update notes for {language}: {actual!r}"
        report["checks"].append("mygo receives notes in one language with English fallback")
        english = "## v2.0.0-build.3\n\nLatest\n\n## v2.0.0-build.2\n\nIntermediate"
        chinese = "## v2.0.0-build.3\n\n最新\n\n## v2.0.0-build.2\n\n中间版本"
        state["cumulative_notes"] = english + "\n\n<!-- zh -->\n\n" + chinese
        state["feed"] = {**original, "buildNumber": "3"}
        report["cumulative_release_notes"] = []
        for language, expected in [("en", english), ("zh-CN", chinese)]:
            actual = json.loads(run(command + ["-notes", "-language", language], env=probe_env))
            report["cumulative_release_notes"].append(
                {"language": language, "expected": expected, "actual": actual}
            )
            assert actual == expected, f"wrong cumulative update notes for {language}: {actual!r}"
        del state["cumulative_notes"]
        report["checks"].append("mygo receives cumulative notes for the installed build and selected language")
        for label, mutate in [
            ("wrong product", lambda f: f.update(product="cli")),
            ("wrong build", lambda f: f.update(buildNumber="0")),
            (
                "duplicate updater",
                lambda f: f["artifacts"].append(copy.deepcopy(f["artifacts"][0])),
            ),
            ("wrong format", lambda f: f["artifacts"][0].update(format="portable.zip")),
            (
                "HTTP public host",
                lambda f: f["artifacts"][0].update(downloadUrl="http://example.com/a"),
            ),
            ("wrong SHA256", lambda f: f["artifacts"][0].update(sha256="0" * 64)),
            ("wrong signature", lambda f: f["artifacts"][0].update(signature="AAAA")),
            ("wrong size", lambda f: f["artifacts"][0].update(size=1)),
        ]:
            state["feed"] = copy.deepcopy(original)
            mutate(state["feed"])
            run(command, env=probe_env, passed=False)
            report["checks"].append(label + " rejected")
        state["feed"] = copy.deepcopy(original)
        run(command + ["-channel", "invalid"], env=probe_env, passed=False)
        report["checks"].append("invalid channel rejected")
        for label, blob in [
            ("signed identity mismatch", archive({**identity, "build_number": 1})),
            ("duplicate identity", archive(duplicate=True)),
            ("missing identity", archive(missing=True)),
        ]:
            select(blob)
            run(command, env=probe_env, passed=False)
            report["checks"].append(label + " rejected")
        select(valid)
        run(command + ["-install"], env=probe_env)
        deadline = time.monotonic() + 20
        while not (payload / "updated.txt").exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        assert (payload / "updated.txt").read_text() == "accepted-build-2"
        assert not (payload / "obsolete.txt").exists()
        assert (user / "preserve.txt").read_text() == "synthetic-user-data"
        report["checks"].extend(
            [
                "mygo installed numbered build",
                "obsolete owned file removed",
                "user data preserved",
            ]
        )
        assert not list(downloads.iterdir()), "private download files remain"
        report["checks"].append("private downloads removed after success and failure")
        report["archive_sha256"] = hashlib.sha256(valid).hexdigest()
        server.shutdown()
        server.server_close()
        report["status"] = "passed"
finally:
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    (out / "SHA256SUMS").write_text(
        hashlib.sha256((out / "report.json").read_bytes()).hexdigest()
        + "  report.json\n"
    )
print(out / "report.json")
