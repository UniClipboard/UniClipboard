#!/usr/bin/env python3
"""Real CLI importer resilience on synthetic, frozen Rust encrypted data."""
import argparse
import errno
import hashlib
import http.client
import json
import os
from pathlib import Path
import platform
import resource
import shutil
import signal
import socket
import sqlite3
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlencode

sys.dont_write_bytecode = True
from migration_acceptance import frozen_source


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cli", required=True, type=Path)
    parser.add_argument("--daemon", required=True, type=Path)
    parser.add_argument("--artifacts", required=True, type=Path)
    parser.add_argument("--records", type=int, default=10000)
    parser.add_argument("--limited-dir", type=Path,
                        help="dedicated filesystem <=128 MiB, e.g. container tmpfs")
    args = parser.parse_args()
    assert args.records >= 1500, "use a source larger than a normal page"
    root = Path(__file__).resolve().parents[2]
    cli, daemon = args.cli.resolve(), args.daemon.resolve()
    artifacts = args.artifacts.resolve()
    artifacts.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="umr", dir="/private/tmp" if platform.system() == "Darwin" else None))
    receipt = {"status": "failed", "platform": platform.platform(), "records": args.records,
               "checks": [], "reports": {}, "native_credentials": "NOT_RUN",
               "binaries_sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in (cli, daemon)},
               "source_sha256": {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
                                 for p in sorted((root / "internal/v1migration").rglob("*")) if p.is_file()},
               "runner_sha256": {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
                                 for p in [Path(__file__).resolve(), root / "e2e/release/migration_acceptance.py", root / "e2e/credentials/main.go"]},
               "commands": ["unic migrate --source <SOURCE> --target <TARGET> --passphrase-stdin --json",
                            "unicd (UNIC_HOME=<TARGET>; isolated config; restart twice)"]}
    env = {k: v for k, v in os.environ.items() if not k.startswith("UNIC_")}
    env.update(HOME=str(work / "home"), USERPROFILE=str(work / "home"), XDG_CONFIG_HOME=str(work / "home/config"),
               XDG_DATA_HOME=str(work / "home/data"), LOCALAPPDATA=str(work / "home/local"),
               UNIC_RENDEZVOUS_URL="http://127.0.0.1:1")
    limited = None

    def check(name, condition):
        assert condition, name
        receipt["checks"].append(name)
        print("PASS", name, flush=True)

    def snapshot(source):
        return {str(p.relative_to(source)): hashlib.sha256(p.read_bytes()).hexdigest()
                for p in source.rglob("*") if p.is_file()}

    def run(source, target, passphrase, expected=0, interrupt=False):
        command = [str(cli), "migrate", "--source", str(source), "--target", str(target),
                   "--passphrase-stdin", "--json"]
        started = time.monotonic()
        p = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             env={**env, "UNIC_HOME": str(target)}, text=True)
        try:
            if interrupt:
                p.stdin.write(passphrase + "\n")
                p.stdin.close()
                reports = []
                for line in p.stdout:
                    value = json.loads(line)
                    reports.append(value)
                    if value.get("stage") == "import" and value.get("processed", 0) >= 250:
                        p.send_signal(signal.SIGINT)
                        break
                reports.extend(json.loads(line) for line in p.stdout)
                stderr = p.stderr.read()
                code = p.wait(timeout=60)
            else:
                out, stderr = p.communicate(passphrase + "\n", timeout=600)
                reports = [json.loads(line) for line in out.splitlines()]
                code = p.returncode
            assert code == expected, (code, expected, reports[-1:] if reports else [], stderr)
            report = reports[-1] if reports else json.loads(stderr)
            report["process_exit_code"] = code
            report["import_report_emitted"] = bool(reports)
            report["duration_seconds"] = round(time.monotonic() - started, 3)
            return report
        finally:
            if p.poll() is None:
                p.kill()
                p.wait()
            for stream in (p.stdin, p.stdout, p.stderr):
                if stream and not stream.closed:
                    stream.close()

    def target_state(target):
        with sqlite3.connect(f"file:{target / 'unic.db'}?mode=ro", uri=True) as db:
            assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
            return (db.execute("SELECT count(*) FROM clip_items").fetchone()[0],
                    db.execute("SELECT count(*) FROM history_imports").fetchone()[0],
                    db.execute("SELECT count(DISTINCT history_id) FROM history_imports").fetchone()[0])

    class UnixHTTP(http.client.HTTPConnection):
        def connect(self):
            self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.sock.settimeout(10)
            self.sock.connect(self.host)

    def service_generation(target):
        c = UnixHTTP(str(target / "unic.sock"))
        try:
            c.request("GET", "/v1/service", headers={"Host": "unic"})
            response = c.getresponse()
            assert response.status == 200, response.read()
            return json.loads(response.read())["id"]
        finally:
            c.close()

    def history(target):
        ids, cursor = set(), ""
        while True:
            c = UnixHTTP(str(target / "unic.sock"))
            try:
                c.request("GET", "/v1/history?" + urlencode({"limit": 200, "cursor": cursor}), headers={"Host": "unic"})
                response = c.getresponse()
                assert response.status == 200, response.read()
                page = json.loads(response.read())
            finally:
                c.close()
            batch = [item["id"] for item in page["items"]]
            assert not ids.intersection(batch)
            ids.update(batch)
            cursor = page.get("next_cursor", "")
            if not cursor:
                return ids

    def restart(target, expected):
        # Keep explicit age/count cleanup disabled. The small byte budget tests
        # the persisted import capacity floor, not user-requested deletion.
        (target / "config.toml").write_text('[telemetry]\nenabled=false\n[sync]\nenabled=false\n[history]\nauto_cleanup=false\nretention_days=0\nmax_bytes="1 KiB"\n')
        previous = None
        for number in range(2):
            p = subprocess.Popen([str(daemon)], env={**env, "UNIC_HOME": str(target)},
                                 stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
            try:
                deadline = time.monotonic() + 40
                while True:
                    try:
                        ids = history(target)
                        break
                    except (OSError, http.client.HTTPException):
                        assert p.poll() is None, "daemon exited before socket became available: " + p.stderr.read()
                        assert time.monotonic() < deadline, "daemon socket timed out"
                        time.sleep(.1)
                check(f"restart {number + 1}: exact public history count", len(ids) == expected)
                if previous is not None:
                    check("restarts retain identical history IDs", ids == previous)
                previous = ids
                generation = service_generation(target)
                source_before_block = snapshot(source)
                blocked = run(source, target, fixture["passphrase"], expected=1)
                receipt["reports"][f"target_owned_{number + 1}"] = blocked
                check(f"restart {number + 1}: active target owner blocks offline importer", blocked.get("status") == "failed" or blocked.get("reason") in {"in_use", "target_unavailable"})
                if blocked.get("reason") in {"in_use", "target_unavailable"}:
                    check(f"restart {number + 1}: ownership refusal precedes source import", not blocked["import_report_emitted"])
                check(f"restart {number + 1}: ownership refusal preserves service generation and source", service_generation(target) == generation and snapshot(source) == source_before_block)
            finally:
                if p.poll() is None:
                    p.send_signal(signal.SIGTERM)
                try:
                    p.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    p.kill()
                    p.wait()
                    raise AssertionError("daemon failed to stop")
                finally:
                    p.stderr.close()

    try:
        source, target = work / "source", work / "target"
        fixture, dbpath = frozen_source(root, source)
        before = snapshot(source)
        partial = run(source, target, fixture["passphrase"], expected=3)
        receipt["reports"]["partial"] = partial
        check("damaged row is partial with one verified commit", partial["status"] == "partial" and partial["imported"] == partial["verified"] == 1)
        check("partial import leaves encrypted source unchanged", snapshot(source) == before)
        with sqlite3.connect(dbpath) as db:
            db.execute("INSERT INTO clipboard_snapshot_representation VALUES('rep-bad','event-bad','text/plain','text/plain',?,NULL,9)", [bytes(fixture["repaired"])])
        repaired = run(source, target, fixture["passphrase"])
        receipt["reports"]["repair"] = repaired
        check("repair resumes only failed record", repaired["imported"] == 1 and repaired["skipped"] == 1 and repaired["verified"] == 2)
        with sqlite3.connect(dbpath) as db:
            db.executemany("INSERT INTO clipboard_entry VALUES(?,'event-text',?,?, 'text',NULL)",
                           [(f"bulk-{i:08}", 10000 + i, 10000 + i) for i in range(args.records - 2)])
        before = snapshot(source)
        cancelled = run(source, target, fixture["passphrase"], expected=1, interrupt=True)
        receipt["reports"]["interrupted"] = cancelled
        check("SIGINT reports cancelled after committed progress", cancelled["status"] == "cancelled" and cancelled["imported"] > 0)
        n, mappings, distinct = target_state(target)
        check("interruption preserves only complete mapped transactions", n == mappings == distinct and 2 < n < args.records)
        resumed = run(source, target, fixture["passphrase"])
        receipt["reports"]["resumed"] = resumed
        check("resume verifies entire large library", resumed["status"] == "complete" and resumed["verified"] == args.records and resumed["imported"] + resumed["skipped"] == args.records)
        again = run(source, target, fixture["passphrase"])
        receipt["reports"]["idempotent"] = again
        check("large retry creates no duplicate history", again["imported"] == 0 and again["skipped"] == args.records and target_state(target) == (args.records,) * 3)
        peak = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
        peak = peak if platform.system() == "Darwin" else peak * 1024
        receipt["migration_peak_child_rss_bytes"] = peak
        check("peak migration process RSS remains below 512 MiB", 0 < peak < 512 * 1024 * 1024)
        restart(target, args.records)
        check("large import and retries never modify encrypted source", snapshot(source) == before)
        if args.limited_dir:
            base = args.limited_dir.resolve()
            stat = os.statvfs(base)
            capacity = stat.f_blocks * stat.f_frsize
            check("ENOSPC filesystem is separate and bounded", os.stat(base).st_dev != os.stat(work).st_dev and 0 < capacity <= 128 * 1024 * 1024)
            limited = Path(tempfile.mkdtemp(prefix="unic-enospc-", dir=base))
            full_target = limited / "target"
            # Populate an independent seed first, then append this library.
            seed = work / "seed"
            seed_fixture, seed_db = frozen_source(root, seed)
            with sqlite3.connect(seed_db) as db:
                db.execute("DELETE FROM clipboard_entry WHERE entry_id='entry-bad'")
                db.execute("UPDATE clipboard_entry SET entry_id='seed-retained' WHERE entry_id='entry-text'")
                db.execute("DELETE FROM history_tag_assignment")
            run(seed, full_target, seed_fixture["passphrase"])
            seed_count = target_state(full_target)[0]
            reserve = limited / "reserve"
            reserve.write_bytes(b"r" * (256 * 1024))
            filler = limited / "filler"
            enospc = False
            with filler.open("wb", buffering=0) as f:
                try:
                    while True:
                        f.write(b"f" * (64 * 1024))
                except OSError as e:
                    enospc = e.errno == errno.ENOSPC
            check("bounded filesystem produces real ENOSPC", enospc)
            reserve.unlink()
            failed = run(source, full_target, fixture["passphrase"], expected=1)
            receipt["reports"]["enospc"] = failed
            check("full-disk import fails instead of completing or marking partial", failed["status"] == "failed")
            filler.unlink()
            recovered = run(source, full_target, fixture["passphrase"])
            receipt["reports"]["enospc_recovery"] = recovered
            check("freeing space permits complete verified recovery", recovered["status"] == "complete" and recovered["verified"] == args.records and target_state(full_target) == (args.records + seed_count,) * 3)
            with sqlite3.connect(f"file:{full_target / 'unic.db'}?mode=ro", uri=True) as db:
                retained = db.execute("SELECT count(*) FROM history_imports m JOIN clip_items i ON i.id=m.history_id WHERE m.record_id='seed-retained'").fetchone()[0]
            check("disk recovery retains unrelated seed and avoids eviction", seed_count == retained == 1)
            receipt["limited_filesystem_capacity_bytes"] = capacity
        else:
            receipt["enospc"] = "NOT_RUN: provide --limited-dir on a dedicated bounded filesystem"
        receipt["status"] = "passed"
    finally:
        shutil.rmtree(work)
        if limited is not None:
            shutil.rmtree(limited)
        receipt["private_profiles_removed"] = not work.exists() and (limited is None or not limited.exists())
        raw = json.dumps(receipt, indent=2, sort_keys=True).encode() + b"\n"
        (artifacts / "receipt.json").write_bytes(raw)
        (artifacts / "SHA256SUMS").write_text(hashlib.sha256(raw).hexdigest() + "  receipt.json\n")


if __name__ == "__main__":
    main()
