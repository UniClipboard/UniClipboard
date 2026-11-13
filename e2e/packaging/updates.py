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
import subprocess
import tarfile
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
p = argparse.ArgumentParser()
p.add_argument("--output", default=str(ROOT / ".cache/update-e2e"))
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


report["source_sha"] = run(["git", "rev-parse", "HEAD"]).strip()
report["source_files"] = {
    str(f.relative_to(ROOT)): hashlib.sha256(f.read_bytes()).hexdigest()
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
try:
    with tempfile.TemporaryDirectory(prefix="unic-update-e2e-") as temporary:
        temp = Path(temporary)
        signer = temp / ("sign.exe" if os.name == "nt" else "sign")
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
                data = (
                    state["archive"]
                    if self.path == "/archive.tar.gz"
                    else json.dumps(state["feed"]).encode()
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
        command = [probe, "-base", base, "-key", key]
        run(command)
        report["checks"].append("valid signed archive accepted")
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
            run(command, passed=False)
            report["checks"].append(label + " rejected")
        state["feed"] = copy.deepcopy(original)
        run(command + ["-channel", "invalid"], passed=False)
        report["checks"].append("invalid channel rejected")
        for label, blob in [
            ("signed identity mismatch", archive({**identity, "build_number": 1})),
            ("duplicate identity", archive(duplicate=True)),
            ("missing identity", archive(missing=True)),
        ]:
            select(blob)
            run(command, passed=False)
            report["checks"].append(label + " rejected")
        select(valid)
        run(command + ["-install"])
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
