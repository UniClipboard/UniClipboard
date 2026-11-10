#!/usr/bin/env python3
"""Complete closure checks without repeating an already verified size matrix."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import time
import uuid

from run import ROOT, Mac, Windows, history, identity, js, output, psquote, ready, until


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--matrix-manifest", type=Path, required=True)
    p.add_argument("--workspace", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--ssh", default="win")
    opt = p.parse_args()
    m = json.loads(opt.matrix_manifest.read_text())
    assert len(m["samples"]) == 72
    assert "bidirectional size matrix: every sample exported and SHA256 verified" in m["checks"]
    assert not m.get("cleanup_errors"), "resolve earlier process cleanup before resuming"
    opt.output.mkdir(parents=True, exist_ok=False)
    root = opt.workspace
    mac = Mac(root, root / "bin/mac")
    win = Windows(opt.ssh, r"C:\Users\mark\AppData\Local\Temp" + "\\" + root.name)
    m["recovery"] = {"matrix_manifest_sha256": hashlib.sha256(opt.matrix_manifest.read_bytes()).hexdigest(),
                     "prior_status": m["status"], "prior_failure": m.pop("failure", None),
                     "reason": "Windows active redirect files prevented log reading; matrix had completed. Closure resumes only after verifying the same source, binaries and isolated state.",
                     "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    m["status"], m["matrix_status"], m["closure_status"] = "running", "passed", "running"
    def save():
        (opt.output / "manifest.json").write_text(json.dumps(m, indent=2) + "\n")
    def check(name):
        m["checks"].append(name)
        save()
        print("PASS " + name, flush=True)
    try:
        for name, digest in m["source_sha256"].items():
            assert hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == digest
        for name in ("unic", "unicd"):
            assert hashlib.sha256((root / "bin/mac" / name).read_bytes()).hexdigest() == m["binary_sha256"]["mac/" + name]
        for name in ("unic.exe", "unicd.exe"):
            actual = win.ps("(Get-FileHash -LiteralPath " + psquote(win.root + "\\" + name) + " -Algorithm SHA256).Hash.ToLower()").decode().strip()
            assert actual == m["binary_sha256"]["windows/" + name]
        logs = [(root / "daemon.log").read_bytes()]
        for name in ("daemon.out", "daemon.err"):
            logs.append(base64.b64decode(win.ps("[Convert]::ToBase64String([IO.File]::ReadAllBytes(" + psquote(win.root + "\\" + name) + "))").strip()))
        assert all(b"P1-sentinel-20261110" not in v for v in logs)
        m["matrix_log_evidence"] = [{"bytes": len(v), "sha256": hashlib.sha256(v).hexdigest()} for v in logs]
        if all(not v for v in logs):
            check("completed matrix daemon logs are empty: no content, address or ticket output")
        else:
            check("completed matrix logs exclude the secret-content sentinel")
            m["limits"].append("Original pairing-ticket sentinel was not available for closure recheck; log byte counts and hashes are recorded")
        mac.start()
        win.start()
        until("mac ready", lambda: ready(mac))
        until("Windows ready", lambda: ready(win))
        online = lambda: "online" in output(mac, "peers") and "online" in output(win, "peers")
        until("existing trust reconnects", online, 90)
        a, b = identity(mac), identity(win)
        before = [{v["id"] for v in history(n)} for n in (mac, win)]
        oldest = next(v for v in reversed(history(mac)) if v["kind"] == "text" and v["origin"] == "explicit")
        durable = js(mac, "history", "show", oldest["id"], "--json")
        mac.stop()
        win.stop()
        mac.start()
        win.start()
        until("mac restarted", lambda: ready(mac))
        until("Windows restarted", lambda: ready(win))
        assert identity(mac) == a and identity(win) == b
        assert [{v["id"] for v in history(n)} for n in (mac, win)] == before
        assert js(mac, "history", "show", oldest["id"], "--json") == durable
        until("restart reconnect", online, 90)
        check("same matrix profiles preserve identities, trust and every history ID across restart")
        for sender, receiver in ((mac, win), (win, mac)):
            key = uuid.uuid4().hex
            text = "post-restart-" + key
            old = {v["id"] for v in history(receiver)}
            result = js(sender, "submit", "--json", "--request-id", key, text)
            assert result["phase"] == "committed" and result["sync_disposition"] == "queued"
            item = until("post-restart content", lambda: next((v for v in history(receiver)
                         if v["id"] not in old and v["origin"] == "remote_device" and v["total_bytes"] == len(text)), None), 90)
            parts = receiver.exported_hashes(item["id"])
            assert len(parts) == 1 and parts[0]["sha256"] == hashlib.sha256(text.encode()).hexdigest()
        check("new bidirectional content transfers and exports correctly after restart")
        win.cli("unpair", a)
        old = {v["id"] for v in history(win)}
        result = js(mac, "submit", "--json", "--request-id", uuid.uuid4().hex, "revoked closure probe")
        assert result["phase"] == "committed"
        time.sleep(4)
        assert {v["id"] for v in history(win)} == old
        check("revocation blocks later remote content while local commit succeeds")
        m["source_unchanged"] = all(hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == digest for name, digest in m["source_sha256"].items())
        assert m["source_unchanged"]
        m["status"], m["closure_status"] = "passed", "passed"
    except Exception as e:
        m["status"], m["closure_status"] = "failed", "failed"
        m["failure"] = type(e).__name__
        raise
    finally:
        for node in (mac, win):
            try:
                node.stop()
            except Exception:
                m["status"] = "failed"
                m.setdefault("cleanup_errors", []).append(type(node).__name__)
        m["finished_at"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")
        save()


if __name__ == "__main__":
    main()
