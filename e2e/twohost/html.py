#!/usr/bin/env python3
"""Supplement production CLI acceptance with HTML+text local-API injection."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid

from run import ROOT, Mac, Windows, history, identity, js, output, pending, psquote, ready, run, until


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--ssh", default="win")
    p.add_argument("--production-manifest", type=Path, required=True)
    p.add_argument("--binaries", type=Path)
    p.add_argument("--output", type=Path, required=True)
    opt = p.parse_args()
    prior = json.loads(opt.production_manifest.read_text())
    opt.output.mkdir(parents=True, exist_ok=False)
    os.chmod(opt.output, 0o700)
    root = Path(tempfile.mkdtemp(prefix="uh4-", dir="/tmp"))
    build_production = opt.binaries is None
    opt.binaries = opt.binaries or root / "production"
    win = Windows(opt.ssh, r"C:\Users\mark\AppData\Local\Temp" + "\\" + root.name)
    mac = Mac(root, opt.binaries / "mac")
    m = {"status": "running", "scope": "two physical hosts; HTML+text injected via production local API; official unic exports verify both representations",
         "source_head": prior["source_head"], "source_sha256": prior["source_sha256"],
         "go": run(["go", "version"]).decode().strip(), "hosts": prior["hosts"],
         "binary_sha256": prior["binary_sha256"], "checks": [], "samples": [], "clock_calibrations": [],
         "runner_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
         "started_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
         "latency_definition": prior["latency_definition"],
         "limits": ["HTML submission uses a test injector because unic send has no HTML input", "native clipboard capture and application are not exercised"]}
    def save():
        (opt.output / "manifest.json").write_text(json.dumps(m, indent=2) + "\n")
    try:
        assert prior["status"] == "passed", "production CLI acceptance must pass first"
        for name, digest in m["source_sha256"].items():
            assert hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == digest
        if build_production:
            for platform in ("mac", "windows"):
                dest = opt.binaries / platform
                dest.mkdir(parents=True)
                env = dict(os.environ)
                if platform == "windows":
                    env.update(GOOS="windows", GOARCH="amd64", CGO_ENABLED="0")
                run(["go", "build", "-trimpath", "-o", str(dest) + "/", "./app/service/cmd/unic", "./app/service/cmd/unicd"], cwd=ROOT, env=env)
        for platform, names in (("mac", ("unic", "unicd")), ("windows", ("unic.exe", "unicd.exe"))):
            for name in names:
                source = opt.binaries / platform / name
                assert hashlib.sha256(source.read_bytes()).hexdigest() == m["binary_sha256"][platform + "/" + name]
                if platform == "windows":
                    win.put(source, name)
        injector = root / "injector"
        (injector / "mac").mkdir(parents=True)
        (injector / "windows").mkdir()
        for platform, dest in (("mac", injector / "mac/unic"), ("windows", injector / "windows/unic.exe")):
            env = dict(os.environ)
            if platform == "windows":
                env.update(GOOS="windows", GOARCH="amd64", CGO_ENABLED="0")
            run(["go", "build", "-trimpath", "-o", str(dest), "./e2e/twohost/snapshot"], cwd=ROOT, env=env)
        clock_binary = root / "clock.exe"
        run(["go", "build", "-trimpath", "-o", str(clock_binary), "./e2e/twohost/clock"], cwd=ROOT,
            env={**os.environ, "GOOS": "windows", "GOARCH": "amd64", "CGO_ENABLED": "0"})
        win.put(clock_binary, "clock.exe")
        m["clock_probe"] = {"binary_sha256": hashlib.sha256(clock_binary.read_bytes()).hexdigest(),
                            "source_sha256": hashlib.sha256((ROOT / "e2e/twohost/clock/main.go").read_bytes()).hexdigest()}
        m["injector_source_sha256"] = hashlib.sha256((ROOT / "e2e/twohost/snapshot/main.go").read_bytes()).hexdigest()
        m["injector_binary_sha256"] = {platform: hashlib.sha256((injector / platform / name).read_bytes()).hexdigest()
                                        for platform, name in (("mac", "unic"), ("windows", "unic.exe"))}
        win.put(injector / "windows/unic.exe", r"injector\unic.exe")
        for name, digest in (("unic.exe", m["binary_sha256"]["windows/unic.exe"]),
                             ("unicd.exe", m["binary_sha256"]["windows/unicd.exe"]),
                             ("injector\\unic.exe", m["injector_binary_sha256"]["windows"]),
                             ("clock.exe", m["clock_probe"]["binary_sha256"])):
            remote_hash = win.ps("(Get-FileHash -LiteralPath " + psquote(win.root + "\\" + name) + " -Algorithm SHA256).Hash.ToLower()").decode().strip()
            assert remote_hash == digest
        # The injector is deliberately isolated from the production program path.
        send_mac = Mac(root, injector / "mac")
        send_win = Windows(opt.ssh, win.root + r"\injector")
        send_win.home = win.home
        Path(mac.home).mkdir()
        Path(mac.home, "config.toml").write_text('device_name = "html-mac"\n')
        win.ps("New-Item -ItemType Directory -Force -Path " + psquote(win.home) + " | Out-Null")
        cfg = root / "config.toml"
        cfg.write_text('device_name = "html-win"\n')
        win.put(cfg, r"state\config.toml")
        mac.start()
        win.start()
        until("mac ready", lambda: ready(mac))
        until("Windows ready", lambda: ready(win))
        ticket = output(mac, "addr")
        pair = output(win, "pair", ticket)
        a, b = identity(mac), identity(win)
        fp_a = until("mac approval", lambda: pending(mac, b))
        fp_b = until("Windows approval", lambda: pending(win, a))
        assert fp_a == fp_b and fp_a in pair
        mac.cli("approve", b, fp_b)
        win.cli("approve", a, fp_a)
        until("online", lambda: "online" in output(mac, "peers") and "online" in output(win, "peers"), 90)
        m["checks"].append("fresh physical hosts mutually approved with matching fingerprints")
        for direction, sender, receiver in (("mac_to_windows", send_mac, win), ("windows_to_mac", send_win, mac)):
            for size in (1024, 65536, 1048576):
                clock = win.clock_probe()
                m["clock_calibrations"].append(clock)
                m.setdefault("windows_clock_estimate", clock)
                for index in range(3):
                    seed = direction + str(size) + str(index)
                    plain = hashlib.shake_256(seed.encode()).digest(size).hex().encode()[:size]
                    html = b'<!doctype html><meta charset="utf-8"><p>' + plain + b'</p>'
                    expected = {".txt": hashlib.sha256(plain).hexdigest(), ".html": hashlib.sha256(html).hexdigest()}
                    payload = {"request_id": uuid.uuid4().hex, "snapshot": {"Parts": [
                        {"Format": mime, "Data": base64.b64encode(data).decode()}
                        for mime, data in (("text/plain", plain), ("text/html", html))]}}
                    fixture = root / "snapshot.json"
                    fixture.write_text(json.dumps(payload))
                    path = win.put(fixture, "snapshot.json") if sender is send_win else str(fixture)
                    old = {v["id"] for v in history(receiver)}
                    start = time.monotonic()
                    result = json.loads(sender.cli(path)[1])
                    timing = sender.last_cli_timing.copy()
                    submitted = (time.monotonic() - start) * 1000
                    assert result["phase"] == "committed" and result["sync_disposition"] == "queued"
                    total = len(plain) + len(html)
                    item = until("HTML remote history", lambda: next((v for v in history(receiver)
                                 if v["id"] not in old and v["origin"] == "remote_device" and v["total_bytes"] == total), None), 90)
                    assert [v["mime"] for v in item["parts"]] == ["text/plain", "text/html"]
                    exported = receiver.exported_hashes(item["id"])
                    assert len(exported) == 2
                    assert {Path(v["name"]).suffix: v["sha256"] for v in exported} == expected
                    verified = (time.monotonic() - start) * 1000
                    receipt = next(v for v in js(receiver, "received", "list") if v.get("history_id") == item["id"])
                    sent_ms = timing["started_ms"] - (clock["offset_ms"] if sender is send_win else 0)
                    rx_ms = receipt["received_at_ms"] - (clock["offset_ms"] if receiver is win else 0)
                    sample = {"direction": direction, "type": "html+text", "bytes": total, "round": index + 1,
                              "submit_ms": round(submitted, 2), "verified_ms": round(verified, 2),
                              "receipt_estimate_ms": round(rx_ms - sent_ms, 2),
                              "receipt_estimate_uncertainty_ms": round(clock["uncertainty_ms"] + 1, 2),
                              "parts_sha256": expected}
                    m["samples"].append(sample)
                    save()
                    print(f"SAMPLE {direction} html+text {total}B receipt={sample['receipt_estimate_ms']}ms verified={verified:.2f}ms", flush=True)
        m["checks"].append("18 HTML+text transfers preserve both MIME types and every exported byte")
        final_clock = win.clock_probe()
        m["clock_calibrations"].append(final_clock)
        m["clock_offset_change_ms"] = final_clock["offset_ms"] - m["windows_clock_estimate"]["offset_ms"]
        m["source_unchanged"] = all(hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == digest for name, digest in m["source_sha256"].items())
        assert m["source_unchanged"]
        m["status"] = "passed"
    except Exception as e:
        m["status"] = "failed"
        m["failure"] = type(e).__name__ if isinstance(e, subprocess.SubprocessError) else str(e)[:500]
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
