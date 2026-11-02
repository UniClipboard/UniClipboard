#!/usr/bin/env python3
"""Real paired daemons and native named pasteboards; never the user's clipboard."""
import argparse
import hashlib
import json
import os
import platform
import secrets
import shutil
import signal
import struct
import subprocess
import tempfile
import time
import tomllib
import urllib.error
import urllib.request
import uuid
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
SAFE_FIELDS = {
    "clipboard_cycle_id", "clipboard_cycle_time_us", "clipboard_identity_ref", "clipboard_ownership_version",
    "read_consumed", "identity_present", "entry_id", "accepted", "errored",
    "pending", "offline", "duplicate", "dropped", "error_kind",
}
MESSAGES = {
    "Captured system clipboard snapshot", "Wrote clipboard snapshot to system",
    "clipboard watcher observed", "clipboard change cycle queued",
    "clipboard change cycle completed", "Starting clipboard capture with provided snapshot",
    "Clipboard capture completed", "inbound clipboard applied",
    "outbound: dispatch_capture entering publish phase", "outbound: dispatch_capture completed",
    "Failed to notify clipboard change", "Failed to read clipboard snapshot",
    "host clipboard change processing failed", "host clipboard change stream failed",
    "host clipboard change stream closed; watcher stopped",
    "host clipboard watcher observed an invalid resend origin",
    "Skipping duplicated meaningful clipboard snapshot",
    "clipboard observation read",
}


class ApiFailure(RuntimeError):
    def __init__(self, label, method, route, status):
        self.status = status
        super().__init__(f"{label} HTTP {method} {route.split('?')[0]} status={status}")


class Blocked(Exception):
    """The real environment could not establish the required preconditions."""


def sha(data):
    return hashlib.sha256(data).hexdigest()


def wait_for(predicate, label, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(0.1)  # Poll an explicit condition; elapsed time never means success.
    raise AssertionError("deadline exceeded: " + label)


class Node:
    def __init__(self, label, binary, tools, private_root, commands):
        self.label, self.tools, self.commands = label, tools, commands
        self.root = private_root / label
        self.root.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.binary = self.bin / "uniclipd"
        shutil.copy2(binary, self.binary)
        self.profile = "e2e-loop-" + uuid.uuid4().hex
        self.board_name = "org.uniclipboard.e2e." + uuid.uuid4().hex
        self.data = self.bin / "data" / ("app.uniclipboard.desktop-" + self.profile)
        self.env = dict(os.environ)
        for key in ["UC_DISABLE_SYSTEM_CLIPBOARD", "UC_LOG_DIR", "UC_ENGINE_LOG_DIR", "UC_E2E_FILE_FAULT", "UC_E2E_SOURCE_FILE"]:
            self.env.pop(key, None)
        self.env.update({
            "UC_PROFILE": self.profile, "UC_PORTABLE": "1", "UNICLIPBOARD_ENV": "development",
            "UC_DAEMON_RUN_MODE": "standalone", "UC_E2E_PRIVATE_PASTEBOARD": self.board_name,
            "DYLD_INSERT_LIBRARIES": str(tools / "private-pasteboard.dylib"),
            "RUST_LOG": "info,uc_platform::clipboard=debug,uc_bootstrap::wiring::desktop_host=debug",
            "SENTRY_DSN": "",
        })
        self.process = None
        self.endpoint = self.session = self.device_id = None

    def board(self, command, *values):
        # Values are exclusively synthetic fixture input, never written into command receipts.
        result = subprocess.run([str(self.tools / "private-pasteboard"), command, *map(str, values)],
                                env=self.env, capture_output=True, timeout=15)
        self.commands.append({"node": self.label, "operation": "native-" + command, "exit_code": result.returncode})
        if result.returncode:
            raise AssertionError("native " + command + " failed on " + self.label)
        return json.loads(result.stdout) if result.stdout else None

    def api(self, method, route, body=None, auth=None, timeout=25):
        request = urllib.request.Request(self.endpoint + route, method=method,
            data=None if body is None else json.dumps(body).encode(),
            headers={"Content-Type": "application/json", "Authorization": auth or ("Session " + self.session)})
        try:
            with urllib.request.urlopen(request, timeout=timeout) as response:
                value = json.load(response)
                self.commands.append({"node": self.label, "operation": "http", "method": method,
                                      "route": route.split("?")[0], "status": response.status})
                return value.get("data", value)
        except urllib.error.HTTPError as error:
            # Never publish response bodies, invitation codes, passphrases, or connection tokens.
            raise ApiFailure(self.label, method, route, error.code) from None

    def start(self):
        self.board("clear")
        with (self.root / "console.log").open("wb") as output:
            self.process = subprocess.Popen([str(self.binary)], env=self.env, stdout=output, stderr=output)
        def ready():
            if self.process.poll() is not None:
                raise AssertionError(self.label + " daemon exited before readiness")
            try:
                connection = json.loads((self.data / "daemon.conn").read_text())
                if connection["pid"] != self.process.pid:
                    return False
                self.endpoint = "http://127.0.0.1:" + str(connection["port"])
                self.session = self.api("POST", "/auth/connect", {"pid": os.getpid(), "clientType": "cli"},
                                        "Bearer " + connection["token"])["sessionToken"]
                return True
            except (OSError, ValueError, KeyError, RuntimeError):
                return False
        wait_for(ready, self.label + " authenticated daemon readiness", 60)
        self.commands.append({"node": self.label, "operation": "daemon-start", "pid": self.process.pid,
                              "profile": self.profile, "binary_sha256": sha(self.binary.read_bytes())})

    def entries(self):
        return self.api("GET", "/clipboard/entries?limit=100")

    def events(self):
        records = []
        for path in (self.bin / "data").rglob("*json*"):
            if "logs" not in path.parts or not path.is_file():
                continue
            for line in path.read_text(errors="replace").splitlines():
                try:
                    record = json.loads(line)
                except ValueError:
                    continue  # An unfinished final JSONL record will be read on the next poll.
                message = record.get("message", "")
                if message not in MESSAGES:
                    continue
                fields = record.get("fields", record)
                records.append({"node": self.label, "timestamp": record.get("timestamp", ""),
                    "message": message, "fields": {key: fields[key] for key in SAFE_FIELDS if key in fields}})
        return sorted(records, key=event_time)

    def receipt(self, ownership):
        records = self.events()
        reads = [e for e in records if e["message"] == "Captured system clipboard snapshot"
                 and e["fields"].get("clipboard_ownership_version") == ownership]
        for read in reads:
            identity = read["fields"].get("clipboard_identity_ref")
            if not identity:
                continue
            observed = any(e["message"] == "clipboard watcher observed" and
                           e["fields"].get("clipboard_identity_ref") == identity for e in records)
            queued = [e for e in records if e["message"] == "clipboard change cycle queued"
                      and e["fields"].get("clipboard_identity_ref") == identity]
            for start in queued:
                cycle = start["fields"]["clipboard_cycle_id"]
                completed = next((e for e in records if e["message"] == "clipboard change cycle completed"
                                  and e["fields"].get("clipboard_cycle_id") == cycle), None)
                if observed and completed:
                    assert completed["fields"].get("read_consumed") is True, "Engine skipped queued snapshot"
                    return start, completed
        return None

    def stop(self):
        result = {"node": self.label, "exit_code": None, "forced": False}
        if self.process:
            if self.process.poll() is None:
                self.process.send_signal(signal.SIGTERM)
            try:
                result["exit_code"] = self.process.wait(timeout=25)
            except subprocess.TimeoutExpired:
                self.process.kill()
                result.update(exit_code=self.process.wait(), forced=True)
        self.board("release")
        return result


def png(rgb):
    width, height = 7, 5
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    rows = (b"\0" + bytes(rgb) * width) * height
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b"")


def fixtures(directory):
    directory.mkdir()
    image = directory / "fixture.png"
    image.write_bytes(png((17, 59, 103)))
    first, second = directory / "first.dat", directory / "second.dat"
    first.write_bytes(b"first synthetic file\n" * 233)
    second.write_bytes(b"second synthetic file\n" * 347)
    return [
        {"name": "text", "command": "text", "values": ["synthetic-loop-" + uuid.uuid4().hex]},
        {"name": "image", "command": "image", "values": [image]},
        {"name": "single-file", "command": "file", "values": [first]},
        {"name": "multiple-files", "command": "files", "values": [first, second]},
    ]


def semantic(summary, command):
    if command == "text":
        return summary.get("textSha256")
    if command == "image":
        return summary.get("image")
    return sorted(summary.get("files", []), key=lambda item: item["sha256"])


def transfer(sender, receiver, fixture, cases, expect_echo=False):
    before_a, before_b = {e["id"] for e in sender.entries()}, {e["id"] for e in receiver.entries()}
    since = time.time()
    sender.board(fixture["command"], *fixture["values"])
    expected = semantic(sender.board("inspect"), fixture["command"])
    assert expected, "native fixture is empty"
    def received():
        summary = receiver.board("inspect")
        return summary if semantic(summary, fixture["command"]) == expected else None
    native = wait_for(received, fixture["name"] + " received native content", 120)
    if expect_echo:
        def echo():
            return next((e for e in receiver.events() if e["message"] == "outbound: dispatch_capture entering publish phase"
                         and event_time(e) >= since), None)
        evidence = wait_for(echo, "old receiver must initiate erroneous return dispatch", 45)
        local_entry = evidence["fields"]["entry_id"]
        assert receiver.api("GET", "/clipboard/entries/" + local_entry + "/delivery")["source"]["tag"] == "local", "old receiver dispatch was not a local recapture"
        cases.append({"scenario": fixture["name"], "result": "EXPECTED_FAILURE_REPRODUCED", "evidence": evidence})
        return
    queued, completed = wait_for(lambda: receiver.receipt(native["changeCount"]),
                                "real receiver watcher consumed and completed target ownership", 45)
    begin_us = queued["fields"]["clipboard_cycle_time_us"]
    end_us = completed["fields"]["clipboard_cycle_time_us"]
    assert begin_us <= end_us, "host observation clock moved backwards"
    between = [e for e in receiver.events() if begin_us <= round(event_time(e) * 1_000_000) <= end_us]
    assert not any(e["message"] == "Starting clipboard capture with provided snapshot" for e in between), "remote observation recaptured locally"
    records = [e for e in receiver.events() if event_time(e) >= since]
    assert not any(e["message"] == "outbound: dispatch_capture entering publish phase" for e in records), "receiver initiated return dispatch"
    assert not any(e["message"] in {"Failed to notify clipboard change", "Failed to read clipboard snapshot", "host clipboard change processing failed", "host clipboard change stream failed", "host clipboard watcher observed an invalid resend origin"} for e in records), "observation evidence has failure or loss"
    new_a = wait_for(lambda: [e for e in sender.entries() if e["id"] not in before_a], "source history")
    new_b = wait_for(lambda: [e for e in receiver.entries() if e["id"] not in before_b], "received history")
    assert len(new_a) == len(new_b) == 1, "duplicate or unexpected history entries"
    local = sender.api("GET", "/clipboard/entries/" + new_a[0]["id"] + "/delivery")["source"]
    remote = receiver.api("GET", "/clipboard/entries/" + new_b[0]["id"] + "/delivery")["source"]
    assert local["tag"] == "local", "source capture was not local"
    assert remote["tag"] == "remote" and remote["deviceId"] == sender.device_id, "received history has wrong origin"
    if fixture["command"] == "text":
        detail = receiver.api("GET", "/clipboard/entries/" + new_b[0]["id"])
        assert sha(detail["content"].encode()) == expected, "decrypted history content differs"
    # Accepted is a positive sending assertion only; it never proves absence of an echo.
    wait_for(lambda: any(e["message"] == "outbound: dispatch_capture completed" and event_time(e) >= since
                         and e["fields"].get("accepted", 0) >= 1 for e in sender.events()), "source dispatch accepted", 60)
    cases.append({"scenario": fixture["name"], "sender": sender.label, "receiver": receiver.label,
                  "result": "PASSED", "ownership_version": native["changeCount"],
                  "cycle_id": completed["fields"]["clipboard_cycle_id"], "content": expected,
                  "source_entry": new_a[0]["id"], "remote_entry": new_b[0]["id"],
                  "source_tags": {"sender": local["tag"], "receiver": remote["tag"]},
                  "return_dispatch_started": 0, "local_capture_started": 0})


def recopy(node, peer, fixture, original, cases):
    """A new native ownership version for identical bytes must remain a local action."""
    before_node = {entry["id"] for entry in node.entries()}
    before_peer = {entry["id"] for entry in peer.entries()}
    current = node.board("inspect")
    expected = semantic(current, fixture["command"])
    since = time.time()
    node.board("recopy")
    native = node.board("inspect")
    assert native["changeCount"] != current["changeCount"], "recopy did not change native ownership"
    assert semantic(native, fixture["command"]) == expected, "native recopy changed content"
    queued, completed = wait_for(lambda: node.receipt(native["changeCount"]),
                                "identical-content local recopy must reach Engine", 20)
    dispatch = wait_for(lambda: next((event for event in node.events()
        if event["message"] == "outbound: dispatch_capture completed" and event_time(event) >= since), None),
        "identical-content local recopy dispatch completion", 60)
    fields = dispatch["fields"]
    assert fields.get("errored", 0) == fields.get("offline", 0) == fields.get("pending", 0) == 0, "recopy dispatch did not complete"
    assert fields.get("accepted", 0) + fields.get("duplicate", 0) > 0, "recopy did not reach its peer"
    assert semantic(peer.board("inspect"), fixture["command"]) == expected, "peer content differs after recopy"
    added_node = [entry["id"] for entry in node.entries() if entry["id"] not in before_node]
    added_peer = [entry["id"] for entry in peer.entries() if entry["id"] not in before_peer]
    assert len(added_node) <= 1 and len(added_peer) <= 1, "recopy created extra history"
    cases.append({"scenario": fixture["name"] + "-same-content-recopy", "sender": node.label,
                  "receiver": peer.label, "result": "PASSED", "ownership_version": native["changeCount"],
                  "cycle_id": completed["fields"]["clipboard_cycle_id"],
                  "source_entry": fields["entry_id"],
                  "remote_entry": added_peer[0] if added_peer else original["source_entry"],
                  "history_reuse_allowed": True})


def event_time(event):
    import datetime
    return datetime.datetime.fromisoformat(event["timestamp"].replace("Z", "+00:00")).timestamp()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--daemon", type=Path, required=True)
    parser.add_argument("--receiver-daemon", type=Path)
    parser.add_argument("--receiver-source-sha", help="Required source provenance when a different receiver binary is supplied")
    parser.add_argument("--tools", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--recopy", action="store_true", help="Also require deliberate identical-content recopy to synchronize")
    parser.add_argument("--expect-echo", action="store_true", help="Positive reproduction on an old unmodified receiver; text only")
    args = parser.parse_args()
    if args.receiver_daemon and not args.receiver_source_sha:
        parser.error("--receiver-daemon requires --receiver-source-sha")
    if platform.system() != "Darwin":
        parser.error("real AppKit requires macOS")
    output = args.artifacts.resolve() / ("run-" + uuid.uuid4().hex)
    output.mkdir(parents=True)
    commands, cases, cleanup, nodes = [], [], [], []
    report = {"result": "FAILED", "boundary": "isolated-native-process-integration",
              "general_clipboard_product_e2e": "NOT_RUN", "concurrency_matrix": "NOT_RUN",
              "same_content_recopy": "NOT_RUN", "full_encryption_audit": "NOT_RUN",
              "cases": cases, "cleanup": cleanup}
    manifest = {"desktop_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                "dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True).strip()),
                "engine_revision": tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["dependencies"]["uc-engine"]["rev"],
                "os": platform.platform(), "architecture": platform.machine(),
                "sender_binary_sha256": sha(args.daemon.read_bytes()),
                "receiver_binary_sha256": sha((args.receiver_daemon or args.daemon).read_bytes()),
                "receiver_source_sha": args.receiver_source_sha or subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                "tools_sha256": {name: sha((args.tools / name).read_bytes()) for name in ["private-pasteboard", "private-pasteboard.dylib"]},
                "runner_sha256": sha(Path(__file__).read_bytes()), "expected_old_receiver_echo": args.expect_echo,
                "invocation": {"runner": "scripts/e2e/inbound-clipboard-loop/run.py", "recopy": args.recopy},
                "helper_source_sha256": sha((ROOT / "scripts/e2e/clipboard-startup/private-pasteboard.m").read_bytes())}
    events = []
    try:
        with tempfile.TemporaryDirectory(prefix="uc-loop-native-") as temporary:
            private = Path(temporary)
            try:
                for label, binary in [("A", args.daemon), ("B", args.receiver_daemon or args.daemon)]:
                    node = Node(label, binary.resolve(), args.tools.resolve(), private, commands)
                    nodes.append(node)
                    node.start()
                a, b = nodes
                manifest["profiles"] = {node.label: node.profile for node in nodes}
                manifest["isolation"] = "independent native named pasteboards and portable development profiles"
                # Empty boards have distinct native objects; profiles alone are insufficient.
                a.board("text", "isolation-probe-" + uuid.uuid4().hex)
                assert not b.board("inspect").get("textSha256"), "pasteboards are shared"
                a.board("clear")
                password = secrets.token_urlsafe(24)
                initialized = a.api("POST", "/v2/setup/initialize", {"passphrase": password, "passphraseConfirm": password, "deviceName": "synthetic-peer-a"})
                a.device_id = initialized["selfDeviceId"]
                b.api("PUT", "/settings", {"general": {"deviceName": "synthetic-peer-b"}})
                try:
                    invitation = a.api("POST", "/v2/setup/issue-invitation")
                    admission = b.api("POST", "/v2/setup/redeem", {"code": invitation["code"], "passphrase": password}, timeout=90)
                    def active_admission():
                        try:
                            current = b.api("GET", "/member/device-group-choices")["deviceTrust"].get("currentJoin")
                        except ApiFailure as error:
                            if error.status == 503:
                                return None  # Admission is replacing the current application facade.
                            raise
                        if current and current["status"] in {"rejected", "terminated", "needs_attention"}:
                            raise Blocked("pairing terminal status=" + current["status"])
                        return current if current and current["status"] == "active" else None
                    if admission["status"] != "active":
                        admission = wait_for(active_admission, "real admission active", 120)
                    b.device_id = admission["joinedSpace"]["selfDeviceId"]
                except (RuntimeError, OSError, TimeoutError) as error:
                    raise Blocked("real pairing unavailable: " + str(error)) from None
                initial_entries = {node.label: {e["id"] for e in node.entries()} for node in nodes}
                canaries = []
                for fixture in fixtures(private / "fixtures"):
                    if fixture["command"] == "text":
                        canaries.extend(value.encode() for value in fixture["values"])
                    transfer(a, b, fixture, cases, args.expect_echo)
                    if args.expect_echo:
                        break
                    if args.recopy:
                        recopy(b, a, fixture, cases[-1], cases)
                    # A distinct local copy proves the watcher remains live after suppression.
                    control = {"name": fixture["name"] + "-local-control", "command": "text",
                               "values": ["synthetic-control-" + uuid.uuid4().hex]}
                    canaries.extend(value.encode() for value in control["values"])
                    transfer(b, a, control, cases)
                if not args.expect_echo:
                    for node in nodes:
                        expected_entries = set(initial_entries[node.label])
                        for case in cases:
                            expected_entries.add(case["source_entry"] if case["sender"] == node.label else case["remote_entry"])
                        assert {e["id"] for e in node.entries()} == expected_entries, "unexpected history entry after completed scenarios"
                report["result"] = "EXPECTED_FAILURE_REPRODUCED" if args.expect_echo else "PASSED"
                if args.recopy and not args.expect_echo:
                    report["same_content_recopy"] = "PASSED"
            finally:
                for node in reversed(nodes):
                    try:
                        cleanup.append(node.stop())
                    except Exception as error:
                        cleanup.append({"node": node.label, "error_type": type(error).__name__})
                post_errors = []
                for node in nodes:
                    node_events = node.events()
                    events.extend(node_events)
                    if not args.expect_echo and cases:
                        allowed = {case["source_entry"] for case in cases if case.get("sender") == node.label}
                        dispatches = [event for event in node_events if event["message"] == "outbound: dispatch_capture entering publish phase"]
                        if not all(event["fields"].get("entry_id") in allowed for event in dispatches):
                            post_errors.append(node.label + ": unexpected outbound initiation through process shutdown")
                    if "canaries" in locals():
                        database_files = [p for p in (node.bin / "data").rglob("*")
                                          if p.is_file() and (p.suffix in {".db", ".sqlite"} or p.name.endswith(("-wal", "-shm")))]
                        if not database_files:
                            post_errors.append(node.label + ": persistent history database missing")
                        if any(canary in path.read_bytes() for path in database_files for canary in canaries):
                            post_errors.append(node.label + ": synthetic text persisted in plaintext database/WAL")
                report["post_shutdown_failures"] = post_errors
                report["history_plaintext_canary_scan"] = "FAILED" if any("database" in error for error in post_errors) else "PASSED"
                if post_errors and report["result"] == "PASSED":
                    report.update(result="FAILED", reason=post_errors[0])

    except Blocked as error:
        report.update(result="BLOCKED", reason=str(error))
    except Exception as error:
        report.update(result="FAILED", reason=str(error), error_type=type(error).__name__)
    finally:
        report["private_profiles_removed"] = "private" in locals() and not private.exists()
        if any(item.get("forced") or item.get("exit_code") != 0 for item in cleanup):
            report.update(result="FAILED", cleanup_failed=True)
        (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        (output / "assertions.json").write_text(json.dumps(report, indent=2) + "\n")
        (output / "commands.json").write_text(json.dumps(commands, indent=2) + "\n")
        (output / "events.jsonl").write_text("".join(json.dumps(event) + "\n" for event in sorted(events, key=event_time)))
        (output / "SHA256SUMS").write_text("".join(sha(path.read_bytes()) + "  " + path.name + "\n" for path in sorted(output.iterdir()) if path.is_file() and path.name != "SHA256SUMS"))
    print(json.dumps({"result": report["result"], "artifacts": str(output), "reason": report.get("reason")}))
    return 0 if report["result"] in {"PASSED", "EXPECTED_FAILURE_REPRODUCED"} else (2 if report["result"] == "BLOCKED" else 1)


if __name__ == "__main__":
    raise SystemExit(main())
