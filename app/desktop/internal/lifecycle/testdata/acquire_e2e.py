#!/usr/bin/env python3
"""Real-process service acquisition acceptance on Unix; no native UI or clipboard."""

import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    children = []
    checks = []
    receipt = {"native_ui": "NOT_RUN", "checks": checks, "processes": [], "status": "failed"}
    receipt["binary_sha256"] = {name: hashlib.sha256((args.bin / name).read_bytes()).hexdigest() for name in ["unicd", "owner"]}
    root = Path(__file__).resolve().parents[5]
    receipt["source_commit"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    receipt["source_sha256"] = {
        str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
        for directory in ["app/desktop/internal/lifecycle", "internal/servicelaunch", "internal/servicelifecycle"]
        for path in (root / directory).rglob("*") if path.is_file() and path.suffix in [".go", ".py"]
    }

    class Connection(http.client.HTTPConnection):
        def __init__(self, path):
            super().__init__("localhost", timeout=1)
            self.path = path

        def connect(self):
            self.sock = socket.socket(socket.AF_UNIX)
            self.sock.settimeout(1)
            self.sock.connect(self.path)

    def service(home):
        conn = Connection(str(home / "unic.sock"))
        try:
            conn.request("GET", "/v1/service")
            response = conn.getresponse()
            assert response.status == 200
            return json.loads(response.read())
        finally:
            conn.close()

    def wait(fn):
        deadline = time.monotonic() + 35
        last = None
        while time.monotonic() < deadline:
            try:
                value = fn()
                if value:
                    return value
            except (OSError, ValueError, AssertionError, http.client.HTTPException) as err:
                last = err
            time.sleep(0.05)
        raise AssertionError(f"state did not converge: {last}")

    with tempfile.TemporaryDirectory(prefix="uc-acq-") as temporary:
        base = Path(temporary)
        def launch(home, name, *command, sock=None):
            home.mkdir(mode=0o700, exist_ok=True)
            (home / "config.toml").write_text("[telemetry]\nenabled=false\n")
            env = dict(os.environ, UNIC_HOME=str(home), UNIC_SOCK=str(sock or home / "unic.sock"), UNIC_RENDEZVOUS_URL="http://127.0.0.1:1")
            log = open(base / f"{len(children)}-{name}.log", "w+")
            proc = subprocess.Popen([str(args.bin / name), *command], env=env, stdout=log, stderr=log)
            children.append((proc, log))
            receipt["processes"].append({"pid": proc.pid, "created_at": time.time(), "command": [name, *command]})
            return proc, log

        def ready(log):
            log.flush()
            return "ready" in Path(log.name).read_text()

        def stop(proc):
            proc.send_signal(signal.SIGINT)
            assert proc.wait(timeout=20) == 0

        try:
            home = base / "standalone"
            daemon, _ = launch(home, "unicd")
            old = wait(lambda: service(home))
            receipt["standalone_service"] = old
            adopter, log = launch(home, "owner", "--acquire")
            wait(lambda: ready(log) or adopter.poll() is not None)
            current = service(home)
            assert current["id"] == old["id"], "acquisition replaced independent standalone generation"
            assert "owned=false" in Path(log.name).read_text(), "adopted service acquired an owner pipe"
            stop(adopter)
            assert service(home)["id"] == old["id"] and daemon.poll() is None
            checks.append("standalone generation reused and survives adopter exit")

            mismatch, log = launch(base / "foreign", "owner", "--acquire", sock=home / "unic.sock")
            assert mismatch.wait(timeout=10) != 0
            assert "different state directory" in Path(log.name).read_text()
            assert service(home)["id"] == old["id"]
            checks.append("foreign profile rejected without stopping standalone")

            owned_home = base / "owned"
            owner, log = launch(owned_home, "owner", "--acquire")
            wait(lambda: ready(log))
            owned = service(owned_home)
            receipt["owned_service"] = owned
            assert "owned=true" in Path(log.name).read_text()
            contender, log = launch(owned_home, "owner", "--acquire")
            assert contender.wait(timeout=10) != 0
            assert service(owned_home)["id"] == owned["id"]
            checks.append("live desktop owner rejected without replacement")
            stop(owner)
            wait(lambda: not (owned_home / "unic.sock").exists())
            checks.append("owned service retires after graceful owner exit")

            owner, log = launch(owned_home, "owner", "--acquire")
            wait(lambda: ready(log))
            owner.kill()
            owner.wait(timeout=10)
            wait(lambda: not (owned_home / "unic.sock").exists())
            checks.append("owner crash closes pipe and retires owned service")

            unknown_home = base / "unknown"
            unknown_home.mkdir(mode=0o700)
            unknown_socket = unknown_home / "unic.sock"
            with socket.socket(socket.AF_UNIX) as listener:
                listener.bind(str(unknown_socket))
                listener.listen()
                rejected, _ = launch(unknown_home, "owner", "--acquire")
                assert rejected.wait(timeout=10) != 0
                assert unknown_socket.exists()
            checks.append("unknown listener rejected without unlink")
            receipt["status"] = "passed"
        finally:
            for proc, log in reversed(children):
                if proc.poll() is None:
                    proc.terminate()
                    try:
                        proc.wait(timeout=20)
                    except subprocess.TimeoutExpired:
                        proc.kill()
                        proc.wait(timeout=5)
                log.close()
            receipt["all_direct_children_exited"] = all(proc.poll() is not None for proc, _ in children)
            args.receipt.parent.mkdir(parents=True, exist_ok=True)
            args.receipt.write_text(json.dumps(receipt, indent=2) + "\n")


if __name__ == "__main__":
    main()
