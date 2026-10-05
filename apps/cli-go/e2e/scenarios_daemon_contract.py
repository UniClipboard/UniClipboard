"""Daemon discovery and version-contract scenarios.

A tiny local HTTP server stands in for `/health` so both CLIs see exactly the
same incompatible, degraded or broken daemon; `daemon.conn` and `.daemon-pid`
are written by the scenario. No real daemon is involved unless stated.
"""
import http.server
import json
import os
import socket
import threading

from scenarios import scenario

API_REVISION = None


def _api_revision():
    global API_REVISION
    if API_REVISION is None:
        root = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
        text = open(os.path.join(root, "crates", "uc-daemon-contract", "src", "lib.rs")).read()
        API_REVISION = text.split('DAEMON_API_REVISION: &str =', 1)[1].split('"')[1]
    return API_REVISION


class _Health(http.server.BaseHTTPRequestHandler):
    status = 200
    body = b""

    def do_GET(self):  # noqa: N802
        self.send_response(self.status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(self.body)))
        self.end_headers()
        self.wfile.write(self.body)

    def log_message(self, *args):
        pass


def _serve(status, body):
    handler = type("H", (_Health,), {"status": status, "body": body})
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def _write_conn(r, port, pid):
    root = r.data_root()
    os.makedirs(root, exist_ok=True)
    with open(os.path.join(root, "daemon.conn"), "w") as fh:
        json.dump({"format": 1, "host": "127.0.0.1", "port": port, "token": "t", "pid": pid,
                   "startedAtMs": 1}, fh)


def _health(status="ok", version="1.1.1", revision=None, residency="standalone"):
    return json.dumps({"data": {"status": status, "packageVersion": version,
                                "apiRevision": revision if revision is not None else _api_revision(),
                                "residency": residency}, "ts": 1}).encode()


COMMANDS = (["space", "status"], ["--json", "start"], ["start", "--foreground"], ["space", "init", "--passphrase", "x"])


def _against(r, label, status, body):
    server = _serve(status, body)
    try:
        # pid 1 is alive but is not a daemon binary, like a stale record.
        _write_conn(r, server.server_address[1], 1)
        for args in COMMANDS:
            r.run(f"{label}: {' '.join(args)}", args, timeout=90)
    finally:
        server.shutdown()


@scenario
def incompatible_daemons(r):
    """Version, revision, status and body mismatches are reported, not reused."""
    _against(r, "older version", 200, _health(version="1.0.0"))
    _against(r, "newer version", 200, _health(version="9.0.0"))
    _against(r, "prerelease version", 200, _health(version="1.1.1-alpha.1"))
    _against(r, "unparsable version", 200, _health(version="weird"))
    _against(r, "missing version", 200, _health(version=" "))
    _against(r, "other revision", 200, _health(revision="other"))
    _against(r, "degraded same version", 200, _health(status="degraded"))
    _against(r, "failed status", 200, _health(status="failed"))
    _against(r, "http 500", 500, b"{}")


@scenario
def malformed_health_bodies(r):
    """Known difference: the decode-error detail is serde_json's text in Rust
    and encoding/json's in Go. Exit codes and the message prefix match."""
    _against(r, "garbage body", 200, b"not json")
    _against(r, "missing data", 200, b'{"ts":1}')


@scenario
def stale_connection_records(r):
    """A dead endpoint or corrupt records must not wedge the CLI."""
    sock = socket.socket()
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    _write_conn(r, port, 999999)
    r.run("dead endpoint: stop", ["--json", "stop"])
    r.run("dead endpoint: space status", ["space", "status"], timeout=120)
    root = r.data_root()
    with open(os.path.join(root, "daemon.conn"), "w") as fh:
        fh.write("not json")
    r.run("corrupt daemon.conn: status", ["space", "status"])
    with open(os.path.join(root, "daemon.conn"), "w") as fh:
        json.dump({"format": 2, "host": "127.0.0.1", "port": port, "token": "t", "pid": 1, "startedAtMs": 1}, fh)
    r.run("future daemon.conn format: status", ["space", "status"])
    os.remove(os.path.join(root, "daemon.conn"))
    with open(os.path.join(root, ".daemon-pid"), "w") as fh:
        fh.write("garbage")
    r.run("corrupt .daemon-pid: stop", ["stop"])
    with open(os.path.join(root, ".daemon-pid"), "w") as fh:
        fh.write(str(999999))
    r.run("legacy dead pid: stop", ["--json", "stop"])
    with open(os.path.join(root, ".daemon-pid"), "w") as fh:
        json.dump({"pid": os.getpid(), "mode": "in_process", "startedAtMs": 1}, fh)
    r.run("in-process pid of a non-daemon: stop", ["--json", "stop"])
