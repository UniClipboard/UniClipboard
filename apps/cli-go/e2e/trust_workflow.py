#!/usr/bin/env python3
"""Go trust workflow acceptance: real daemon membership plus controlled HTTP
contract responses for outcomes/multiple issues that cannot be scheduled reliably.
All processes use compat.Runner's isolated HOME. The proxy forwards health,
authentication and the control WebSocket to the real daemon unchanged.
"""
import argparse
import copy
import http.server
import json
import os
from pathlib import Path
import pty
import select
import signal
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request

from scenarios import PASSPHRASE
from compat import Runner, Step, render

CHOICES = "/member/device-group-choices"


class LocalRendezvous:
    """Local invitation service fixture; peers still pair over real P2P."""
    def __enter__(self):
        tickets = {}
        lock = threading.Lock()

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", 0))))
                status, value = 200, {}
                with lock:
                    if self.path == "/v1/pairings":
                        length = body.get("codeLength", 8)
                        digits = str(len(tickets)+1).zfill(length)
                        code = digits[:length//2] + "-" + digits[length//2:]
                        tickets[code] = body["sponsorTicket"]
                        value = {"code": code, "expiresAtMs": 4102444800000}
                    elif self.path == "/v1/pairings/resolve":
                        ticket = tickets.get(body.get("code"))
                        if ticket is None:
                            status = 404
                        else:
                            value = {"sponsorTicket": ticket, "sponsorEndpointId": "ignored",
                                     "expiresAtMs": 4102444800000}
                    elif self.path == "/v1/pairings/consume":
                        tickets.pop(body.get("code"), None)
                        status = 204
                    else:
                        status = 404
                raw = json.dumps(value).encode() if status != 204 else b""
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

            def log_message(self, *args):
                pass

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        return f"http://127.0.0.1:{self.server.server_address[1]}"

    def __exit__(self, *args):
        self.server.shutdown()
        self.server.server_close()


class TrustRunner(Runner):
    def env(self, profile=None, extra=None):
        return super().env(profile, {"UC_E2E_RENDEZVOUS_BASE_URL": self.rendezvous, **(extra or {})})


class Terminal:
    def __init__(self, runner, args, profile=None):
        self.runner, self.args = runner, args
        self.master, slave = pty.openpty()
        self.proc = subprocess.Popen([str(Path(runner.bindir) / "uniclip"), *args],
                                     stdin=slave, stderr=slave, stdout=subprocess.PIPE,
                                     env=runner.env(profile), start_new_session=True)
        os.close(slave)
        self.transcript = b""
        self.cursor = 0

    def read(self):
        if select.select([self.master], [], [], 0.1)[0]:
            try:
                self.transcript += os.read(self.master, 65536)
            except OSError:
                pass

    def expect(self, text, timeout=30):
        deadline = time.monotonic() + timeout
        needle = text.encode()
        while time.monotonic() < deadline:
            pos = self.transcript.find(needle, self.cursor)
            if pos >= 0:
                self.cursor = pos + len(needle)
                # Prompts write their label immediately before entering raw mode.
                time.sleep(0.05)
                return
            self.read()
            if self.proc.poll() is not None:
                break
        raise AssertionError(f"missing prompt {text!r}: {self.transcript.decode(errors='replace')}")

    def type(self, text):
        os.write(self.master, text.encode())

    def finish(self, label, code=0):
        deadline = time.monotonic() + 30
        try:
            while self.proc.poll() is None and time.monotonic() < deadline:
                self.read()
            if self.proc.poll() is None:
                raise AssertionError("CLI did not exit: " + repr(self.transcript))
            for _ in range(3):
                self.read()
            step = Step(label, self.args, self.proc.returncode, self.proc.stdout.read(), self.transcript)
            self.runner.steps.append(step)
            assert step.code == code, render(step, self.runner.home)
            return step
        finally:
            self.close()

    def close(self):
        if self.proc.poll() is None:
            self.proc.kill()
            self.proc.wait()
        if self.master is not None:
            os.close(self.master)
            self.master = None


def success(step):
    assert step.code == 0, f"{step.label}: exit={step.code} {step.err!r}"
    return json.loads(step.out) if step.out else None


def state(runner, profile=None):
    return success(runner.run("read trust state", ["--json", "member", "trust", "status"],
                              profile=profile, compare=False))


def wait_issue(runner, profile):
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        current = state(runner, profile)
        if current["issues"]:
            return current
        time.sleep(0.25)
    raise AssertionError(f"no removal issue on {profile}")


class ContractProxy:
    """Inject only trust DTOs; the live daemon still owns discovery/auth/lease.
    These are transport contract cases, not assertions about Engine decisions.
    """
    def __init__(self, runner, snapshots, outcomes):
        self.path = Path(runner.data_root()) / "daemon.conn"
        self.original = self.path.read_bytes()
        self.conn = json.loads(self.original)
        self.snapshots, self.outcomes = snapshots, outcomes
        self.requests = []
        self.reads = 0
        proxy = self

        class Handler(http.server.BaseHTTPRequestHandler):
            def handle_request(self):
                if self.headers.get("Upgrade", "").lower() == "websocket":
                    with socket.create_connection((proxy.conn["host"], proxy.conn["port"]), 10) as upstream:
                        headers = "".join(f"{key}: {value}\r\n" for key, value in self.headers.items())
                        upstream.sendall(f"GET {self.path} HTTP/1.1\r\n{headers}\r\n".encode())
                        while True:
                            ready, _, _ = select.select([upstream, self.connection], [], [], 1)
                            for source in ready:
                                data = source.recv(65536)
                                if not data:
                                    return
                                (self.connection if source is upstream else upstream).sendall(data)
                body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
                if self.path == CHOICES and self.command == "GET":
                    index = min(len(proxy.requests), len(proxy.snapshots)-1)
                    data = {"data": proxy.snapshots[index], "ts": 1}
                    proxy.reads += 1
                elif self.path == CHOICES and self.command == "POST":
                    proxy.requests.append(json.loads(body))
                    index = min(len(proxy.requests)-1, len(proxy.outcomes)-1)
                    data = {"data": {"outcome": proxy.outcomes[index]}, "ts": 1}
                else:
                    url = f"http://{proxy.conn['host']}:{proxy.conn['port']}{self.path}"
                    req = urllib.request.Request(url, data=body if body else None, method=self.command,
                                                 headers={k: v for k, v in self.headers.items()
                                                          if k.lower() not in ('host', 'connection')})
                    with urllib.request.urlopen(req, timeout=15) as response:
                        self.reply(response.read(), response.status)
                    return
                self.reply(json.dumps(data).encode(), 200)

            do_GET = do_POST = handle_request

            def reply(self, body, status):
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *args):
                pass

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True

    def __enter__(self):
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        conn = dict(self.conn, port=self.server.server_address[1])
        self.path.write_text(json.dumps(conn))
        return self

    def __exit__(self, *args):
        self.path.write_bytes(self.original)
        self.server.shutdown()
        self.server.server_close()


def contract_cases(runner, baseline):
    local = baseline["deviceTrust"]["localDeviceId"]
    peer = "0123456789abcdef-peer"
    first = copy.deepcopy(baseline)
    first["revision"] = 40
    option = {"choiceId": "keep", "isCurrentGroup": True, "requiresRePairing": False,
              "memberDeviceIds": [local, peer, peer], "membersComplete": True,
              "members": [{"deviceId": local, "displayName": "This laptop"},
                          {"deviceId": peer, "displayName": "Phone"}],
              "impact": {"localDeviceOutcome": "active", "pausedDeviceIds": [peer, peer],
                         "requiresRejoinDeviceIds": [peer], "pendingConfirmationDeviceIds": [peer]}}
    first["issues"] = [{"issueId": "first-opaque-id", "choices": [dict(option, choiceId="apply"), option],
                        "reason": {"kind": "pending_removal", "detailsComplete": True,
                                   "changes": [{"kind": "removed_device",
                                                "actor": {"deviceId": local, "displayName": "This laptop"},
                                                "target": {"deviceId": peer, "displayName": "Phone"}}]}},
                       {"issueId": "second-opaque-id", "choices": [dict(option, choiceId="b:candidate"), option]}]
    second = copy.deepcopy(first)
    second["revision"] = 41
    second["issues"] = second["issues"][1:]
    done = copy.deepcopy(second)
    done["revision"] = 42
    done["issues"] = []

    with ContractProxy(runner, [first, second, done], ["completed", "already_completed"]) as proxy:
        tty = Terminal(runner, ["member", "trust", "choose"])
        try:
            tty.expect("Choice number (1-2)")
            for invalid in ("0", "3", "nope", "", "99999999999999999999999"):
                tty.type(invalid + "\n")
                tty.expect("Enter a choice number from 1 to 2")
                tty.expect("Choice number (1-2)")
                assert not proxy.requests, "invalid input submitted a choice"
            tty.type("2\n")
            tty.expect("Choice number (1-2)")
            tty.type("1\n")
            step = tty.finish("contract: invalid numbers and continuous fresh revisions")
            assert [r["issueId"] for r in proxy.requests] == ["first-opaque-id", "second-opaque-id"]
            assert [r["choiceId"] for r in proxy.requests] == ["keep", "b:candidate"]
            assert [r["expectedRevision"] for r in proxy.requests] == [40, 41]
            assert proxy.reads == 3
            assert b"proposed removing Phone" in step.err
            assert b"sync paused: Phone (0123456789ab" in step.err
            assert b"membership: active -> active" in step.err
            assert b"Phone (0123456789ab\xe2\x80\xa6), Phone" not in step.err
        finally:
            tty.close()

    incomplete = copy.deepcopy(first)
    incomplete["deviceTrust"]["devices"] = []
    for issue in incomplete["issues"]:
        issue.pop("reason", None)
        for choice in issue["choices"]:
            choice.pop("members", None)
            choice.pop("impact", None)
            choice["membersComplete"] = False
    with ContractProxy(runner, [incomplete], []) as proxy:
        human = runner.run("contract: incomplete details and ID fallback", ["member", "trust", "status"])
        assert human.code == 0 and not proxy.requests
        assert b"0123456789ab\xe2\x80\xa6" in human.err and b"Phone" not in human.err
        assert b"details are incomplete" in human.err and b"details are unavailable" in human.err
        assert b"0123456789ab\xe2\x80\xa6, 0123456789ab" not in human.err

    for outcome in ("completed", "pending", "re_pairing_required", "state_changed", "local_device_confirmation_required"):
        args = ["member", "trust", "choose"] + (["--once"] if outcome == "completed" else [])
        with ContractProxy(runner, [first, second], [outcome]) as proxy:
            tty = Terminal(runner, args)
            try:
                tty.expect("Choice number (1-2)")
                tty.type("2\n")
                tty.finish("contract: stop on " + outcome,
                           1 if outcome in ("state_changed", "local_device_confirmation_required") else 0)
                assert len(proxy.requests) == 1 and proxy.reads == 2
            finally:
                tty.close()

    removal = copy.deepcopy(second)
    removal["issues"][0]["choices"][0]["impact"]["localDeviceOutcome"] = "removed"
    with ContractProxy(runner, [first, removal], ["completed"]) as proxy:
        tty = Terminal(runner, ["member", "trust", "choose", "--confirm-local-removal"])
        try:
            tty.expect("Choice number (1-2)")
            tty.type("2\n")
            tty.expect("Choice number (1-2)")
            tty.type("1\n")
            tty.expect("This choice removes this device from the space. Continue?")
            assert len(proxy.requests) == 1
            tty.type("n")
            tty.finish("contract: initial confirmation flag does not authorize later removal")
            assert len(proxy.requests) == 1
        finally:
            tty.close()

    for as_json, terminal in ((False, False), (True, False), (False, True), (True, True)):
        args = (["--json"] if as_json else []) + ["member", "trust", "choose", "--issue", "first-opaque-id", "--choice", "keep"]
        with ContractProxy(runner, [first, second], ["completed"]) as proxy:
            step = (Terminal(runner, args).finish("contract: explicit IDs in TTY") if terminal else
                    runner.run("contract: explicit IDs without TTY", args))
            assert step.code == 0 and len(proxy.requests) == 1
            assert b"Choice number" not in step.err
            if as_json:
                value = json.loads(step.out)
                assert list(value) == ["ok", "result", "state"]
                assert value["ok"] and value["result"] == {"outcome": "completed"}
                assert value["state"] == second
    for args, expected in ((["member", "trust", "choose"], "issue_id_required"),
                           (["--json", "member", "trust", "choose"], "issue_id_required")):
        with ContractProxy(runner, [first], []) as proxy:
            step = runner.run("contract: missing IDs without interaction", args)
            assert step.code == 1 and not proxy.requests
            if args[0] == "--json":
                assert json.loads(step.out)["code"] == expected


def real_removal(runner):
    """Three real peers; removal, cancellation, revision race and local safety."""
    profiles = [runner.profile, runner.profile + "-bob", runner.profile + "-carol"]
    runner.extra_profiles.extend(profiles[1:])
    sponsor, bob, carol = profiles
    success(runner.run("init sponsor", ["space", "init", "--passphrase", PASSPHRASE,
                                       "--device-name", "Mac mini"]))
    success(runner.run("start sponsor", ["--json", "start"]))
    for profile, name in ((bob, "Xiaomi 14"), (carol, "Linux laptop")):
        invite = runner.spawn("invite", ["space", "invite"], profile=sponsor)
        buf, code, deadline = b"", None, time.monotonic() + 60
        os.set_blocking(invite["proc"].stdout.fileno(), False)
        while time.monotonic() < deadline and code is None:
            buf += invite["proc"].stdout.read() or b""
            for line in buf.decode(errors="replace").splitlines():
                if line.startswith("INVITATION_CODE="):
                    code = line.split("=", 1)[1].strip()
            time.sleep(0.1)
        assert code, "no invitation code"
        success(runner.run("join " + name, ["space", "join", "--code", code,
                                            "--passphrase", PASSPHRASE, "--device-name", name], profile=profile))
        os.set_blocking(invite["proc"].stdout.fileno(), True)
        runner.finish(invite, sig=signal.SIGINT, compare=False)
        success(runner.run("start " + name, ["--json", "start"], profile=profile))
    roster = success(runner.run("roster", ["--json", "member", "list"]))
    bob_id = next(d["device_id"] for d in roster if d["device_name"] == "Xiaomi 14")
    success(runner.run("remove Bob", ["--json", "member", "remove", bob_id]))
    bob_state = wait_issue(runner, bob)
    carol_state = wait_issue(runner, carol)
    human = runner.run("real: human removal explanation", ["member", "trust", "status"], profile=carol)
    assert human.code == 0 and b"proposed removing Xiaomi 14" in human.err and b"Mac mini" in human.err
    issue = bob_state["issues"][0]
    apply_number = next(i+1 for i, c in enumerate(issue["choices"]) if c["choiceId"] == "apply")
    tty = Terminal(runner, ["member", "trust", "choose"], bob)
    try:
        tty.expect("Choice number")
        tty.type("0\n")
        tty.expect("Enter a choice number from 1 to")
        tty.expect("Choice number")
        assert state(runner, bob)["issues"][0]["issueId"] == issue["issueId"]
        tty.type(str(apply_number) + "\n")
        tty.expect("This choice removes this device from the space. Continue?")
        tty.type("n")
        tty.finish("real: local removal declined")
        assert state(runner, bob)["issues"][0]["issueId"] == issue["issueId"]
    finally:
        tty.close()
    step = runner.run("real: JSON local removal guard", ["--json", "member", "trust", "choose", "--issue",
                      issue["issueId"], "--choice", "apply"], profile=bob)
    assert step.code == 1 and json.loads(step.out)["code"] == "local_removal_confirmation_required"
    issue = carol_state["issues"][0]
    keep_number = next(i+1 for i, c in enumerate(issue["choices"]) if c["choiceId"] == "keep")
    tty = Terminal(runner, ["member", "trust", "choose"], carol)
    try:
        tty.expect("Choice number")
        changed = runner.run("real: concurrent keep", ["--json", "member", "trust", "choose", "--issue",
                            issue["issueId"], "--choice", "keep"], profile=carol)
        success(changed)
        tty.type(str(keep_number) + "\n")
        # Engine may return already_completed for idempotent replay or state_changed.
        deadline = time.monotonic() + 30
        while tty.proc.poll() is None and time.monotonic() < deadline:
            tty.read()
        assert tty.proc.poll() in (0, 1)
        step = tty.finish("real: revision changes while entering choice", tty.proc.returncode)
        assert b"Device group state changed" in step.err or b"already completed" in step.err
    finally:
        tty.close()
    tty = Terminal(runner, ["member", "trust", "choose", "--once"], bob)
    try:
        tty.expect("Choice number")
        tty.type(str(apply_number) + "\n")
        tty.expect("This choice removes this device from the space. Continue?")
        tty.type("y")
        tty.finish("real: numbered removal explicitly confirmed")
        assert state(runner, bob)["deviceTrust"]["localMembership"] == "removed"
    finally:
        tty.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--contract-only", action="store_true", help="Skip the three-peer network scenario")
    args = parser.parse_args()
    out = Path(args.out).resolve()
    out.mkdir(parents=True, exist_ok=True)
    bindir = str(Path(args.bin).resolve())
    with tempfile.TemporaryDirectory(prefix="uniclip-trust-") as work, LocalRendezvous() as rendezvous:
        runner = TrustRunner("go", bindir, bindir, work, "trust-workflow")
        runner.rendezvous = rendezvous
        try:
            success(runner.run("init contract profile", ["space", "init", "--passphrase", PASSPHRASE,
                                                         "--device-name", "contract-host"]))
            success(runner.run("start contract daemon", ["--json", "start"]))
            contract_cases(runner, state(runner))
            print("HTTP contract cases passed.", flush=True)
            if not args.contract_only:
                # Use a distinct profile so the real peer topology starts fresh.
                runner.extra_profiles.append(runner.profile)
                runner.profile += "-real"
                real_removal(runner)
            print("Trust workflow acceptance passed.")
        finally:
            (out / "transcript.txt").write_text("".join(render(s, runner.home) for s in runner.steps))
            runner.cleanup()


if __name__ == "__main__":
    main()
