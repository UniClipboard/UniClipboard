#!/usr/bin/env python3
"""Production CLI acceptance on a macOS controller and a Windows SSH host."""
import argparse
import base64
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shutil
import struct
import subprocess
import tempfile
import time
import uuid
import zlib

ROOT = Path(__file__).resolve().parents[2]


def run(args, **kw):
    return subprocess.run(args, check=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, timeout=240, **kw).stdout


def psquote(s):
    return "'" + str(s).replace("'", "''") + "'"


class Windows:
    def __init__(self, target, directory):
        self.target, self.root = target, directory
        self.home = directory + r"\state"
        self.pid = None
        self.connection = None
        self.last_cli_timing = None

    def ps(self, script):
        encoded = base64.b64encode(script.encode("utf-16le")).decode()
        return run(["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
                    self.target, "powershell -NoProfile -NonInteractive -EncodedCommand " + encoded])

    def put(self, source, name):
        dest = self.root + "\\" + name
        self.ps("New-Item -ItemType Directory -Force -Path " + psquote(str(Path(dest.replace('\\', '/')).parent).replace('/', '\\')) + " | Out-Null")
        run(["scp", "-q", str(source), self.target + ":" + dest.replace("\\", "/")])
        return dest

    def cli(self, *args, stdin=None, ok=True):
        key = uuid.uuid4().hex
        out, err = (self.root + "\\" + key + s for s in (".out", ".err"))
        input_opt = ""
        if stdin is not None:
            input_opt = " -RedirectStandardInput " + psquote(stdin)
        line = subprocess.list2cmdline(list(args))
        script = ("$env:UNIC_HOME=" + psquote(self.home) + "; "
                  "$started=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds(); $p=Start-Process -FilePath " + psquote(self.root + r"\unic.exe") +
                  " -ArgumentList " + psquote(line) + " -Wait -PassThru -NoNewWindow" +
                  " -RedirectStandardOutput " + psquote(out) +
                  " -RedirectStandardError " + psquote(err) + input_opt + "; "
                  "@{started_ms=$started;finished_ms=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds();code=$p.ExitCode;out=[Convert]::ToBase64String([IO.File]::ReadAllBytes(" + psquote(out) +
                  "));err=[Convert]::ToBase64String([IO.File]::ReadAllBytes(" + psquote(err) +
                  "))} | ConvertTo-Json -Compress; Remove-Item " + psquote(out) + "," + psquote(err))
        v = json.loads(self.ps(script))
        self.last_cli_timing = {k: v[k] for k in ("started_ms", "finished_ms")}
        if ok and v["code"] != 0:
            raise RuntimeError("Windows CLI " + args[0] + " failed: " + base64.b64decode(v["err"]).decode(errors="replace"))
        return v["code"], base64.b64decode(v["out"]), base64.b64decode(v["err"])

    def start(self):
        script = ("$env:UNIC_HOME=" + psquote(self.home) + "; "
                  "$p=Start-Process -FilePath " + psquote(self.root + r"\unicd.exe") +
                  " -PassThru -RedirectStandardOutput " + psquote(self.root + r"\daemon.out") +
                  " -RedirectStandardError " + psquote(self.root + r"\daemon.err") + "; $p.Id; $p.WaitForExit()")
        encoded = base64.b64encode(script.encode("utf-16le")).decode()
        self.connection = subprocess.Popen(
            ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10", self.target,
             "powershell -NoProfile -NonInteractive -EncodedCommand " + encoded],
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        self.pid = int(self.connection.stdout.readline().strip())

    def stop(self):
        if self.pid is not None:
            self.ps("$p=Get-Process -Id " + str(self.pid) + " -ErrorAction SilentlyContinue; "
                    "if ($p -and $p.Path -eq " + psquote(self.root + r"\unicd.exe") +
                    ") { Stop-Process -Id $p.Id -Force }")
            self.pid = None
            self.connection.wait(timeout=15)
            self.connection.stdout.close()

    def exported_hashes(self, item):
        dest = self.root + "\\export-" + uuid.uuid4().hex
        self.cli("history", "export", item, "--output", dest)
        script = ("@(Get-ChildItem -LiteralPath " + psquote(dest) +
                  " -File | Sort-Object Name | ForEach-Object { @{name=$_.Name;size=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower()} }) | ConvertTo-Json -Compress")
        result = json.loads(self.ps(script))
        self.ps("Remove-Item -LiteralPath " + psquote(dest) + " -Recurse -Force")
        return result if isinstance(result, list) else [result]

    def clock_probe(self):
        p = subprocess.Popen(["ssh", "-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
                              self.target, self.root + r"\clock.exe"],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        def line():
            if not select.select([p.stdout], [], [], 8)[0]:
                raise TimeoutError("native clock probe timeout")
            return p.stdout.readline().strip()
        probes = []
        try:
            assert line() == b"ready"
            for _ in range(12):
                wall = time.time_ns() / 1e6
                start = time.monotonic_ns()
                p.stdin.write(b"probe\n")
                p.stdin.flush()
                remote_ms = int(line()) / 1e6
                elapsed = (time.monotonic_ns() - start) / 1e6
                probes.append({"offset_ms": remote_ms - (wall + elapsed / 2),
                               "uncertainty_ms": elapsed / 2, "controller_ms": wall + elapsed / 2})
        finally:
            p.stdin.close()
            p.terminate()
            p.wait(timeout=10)
            p.stdout.close()
        return min(probes, key=lambda v: v["uncertainty_ms"])


class Mac:
    def __init__(self, directory, binaries):
        self.root = Path(directory)
        self.home = str(self.root / "state")
        self.binaries = binaries
        self.process = None
        self.log = None
        self.last_cli_timing = None

    def cli(self, *args, stdin=None, ok=True):
        started = time.time_ns() / 1e6
        with open(stdin, "rb") if stdin else open(os.devnull, "rb") as f:
            p = subprocess.run([str(self.binaries / "unic"), *args], stdin=f,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               env={**os.environ, "UNIC_HOME": self.home}, timeout=120)
        self.last_cli_timing = {"started_ms": started, "finished_ms": time.time_ns() / 1e6}
        if ok and p.returncode:
            raise RuntimeError("macOS CLI " + args[0] + " failed: " + p.stderr.decode(errors="replace"))
        return p.returncode, p.stdout, p.stderr

    def start(self):
        self.log = open(self.root / "daemon.log", "ab")
        self.process = subprocess.Popen([str(self.binaries / "unicd")],
                                        env={**os.environ, "UNIC_HOME": self.home},
                                        stdout=self.log, stderr=self.log)

    def stop(self):
        if self.process is not None:
            self.process.terminate()
            try:
                self.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
            self.process = None
            self.log.close()

    def exported_hashes(self, item):
        dest = self.root / ("export-" + uuid.uuid4().hex)
        self.cli("history", "export", item, "--output", str(dest))
        result = [{"name": p.name, "size": p.stat().st_size,
                   "sha256": hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(dest.iterdir())]
        shutil.rmtree(dest)
        return result


def output(node, *args):
    return node.cli(*args)[1].decode("utf-8").strip()


def js(node, *args):
    return json.loads(output(node, *args))


def history(node):
    return js(node, "history", "list", "-n", "200", "--json") or []


def until(description, fn, seconds=60):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        v = fn()
        if v:
            return v
        time.sleep(.2)
    raise AssertionError("timeout: " + description)


def ready(node):
    return node.cli("status", ok=False)[0] == 0


def pending(node, device):
    for line in output(node, "requests").splitlines():
        if line.startswith(device + " "):
            return re.split(r"\s{2,}", line.strip())[-1]
    return None


def identity(node):
    return re.search(r"device\s+.*\(([^)]+)\)", output(node, "status"))[1]


def submit(node, key, kind, path):
    args = ["submit", "--json", "--request-id", key]
    if kind == "text":
        return js_stdin(node, args, path)
    args += ["--png", path] if kind == "png" else ["-f", path]
    return js(node, *args)


def js_stdin(node, args, path):
    return json.loads(node.cli(*args, stdin=path)[1])


def png_bytes(width, height, seed):
    raw = hashlib.shake_256(seed.encode()).digest(width * height * 3)
    rows = b"".join(b"\0" + raw[i:i + width * 3] for i in range(0, len(raw), width * 3))
    def chunk(kind, value):
        return struct.pack(">I", len(value)) + kind + value + struct.pack(">I", zlib.crc32(kind + value))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)) +
            chunk(b"IDAT", zlib.compress(rows, 1)) + chunk(b"IEND", b""))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ssh", default="win")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--binaries", type=Path, help="existing bin directory with mac/ and windows/")
    parser.add_argument("--rounds", type=int, default=3)
    opt = parser.parse_args()
    opt.output.mkdir(parents=True, exist_ok=False)
    os.chmod(opt.output, 0o700)
    work = Path(tempfile.mkdtemp(prefix="up1-", dir="/tmp"))
    remote = r"C:\Users\mark\AppData\Local\Temp" + "\\" + work.name
    win, mac = Windows(opt.ssh, remote), None
    manifest = {"status": "running", "scope": "two physical hosts, production CLI, real tailcat; no system clipboard",
                "started_at": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "checks": [], "samples": [],
                "latency_definition": "controller monotonic time before submit until receiver history export SHA256 verification; includes CLI, SSH, polling, export and hashing; not pure network latency",
                "limits": ["HTML/rich-text submission is not exposed by the production CLI", "native capture/application and echo are not exercised", "in-flight queue replacement, slow observers and cancellation races require separate acceptance"],
                "source_head": run(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip()}
    source = {}
    for name in run(["git", "ls-files", "-z"], cwd=ROOT).decode().split("\0"):
        if (name.endswith(".go") and name.startswith(("engine/", "internal/", "plugin/", "app/service/"))) or name in ("go.mod", "go.sum"):
            source[name] = hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
    manifest["source_sha256"] = source
    manifest["runner_sha256"] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    manifest["prebuilt_binaries"] = bool(opt.binaries)
    def save():
        (opt.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    def check(name):
        manifest["checks"].append(name)
        save()
        print("PASS " + name, flush=True)
    try:
        bins = opt.binaries or work / "bin"
        if not opt.binaries:
            def build(platform):
                target = bins / platform
                target.mkdir(parents=True)
                env = {**os.environ, "CGO_ENABLED": "0"} if platform == "windows" else dict(os.environ)
                if platform == "windows":
                    env.update(GOOS="windows", GOARCH="amd64")
                run(["go", "build", "-trimpath", "-o", str(target) + "/", "./app/service/cmd/unic", "./app/service/cmd/unicd"], cwd=ROOT, env=env)
            with concurrent.futures.ThreadPoolExecutor(2) as pool:
                list(pool.map(build, ("mac", "windows")))
        manifest["go"] = run(["go", "version"]).decode().strip()
        clock_binary = work / "clock.exe"
        run(["go", "build", "-trimpath", "-o", str(clock_binary), "./e2e/twohost/clock"],
            cwd=ROOT, env={**os.environ, "GOOS": "windows", "GOARCH": "amd64", "CGO_ENABLED": "0"})
        manifest["clock_probe"] = {"source_sha256": hashlib.sha256((ROOT / "e2e/twohost/clock/main.go").read_bytes()).hexdigest(),
                                   "binary_sha256": hashlib.sha256(clock_binary.read_bytes()).hexdigest(),
                                   "method": "12 request/response timestamps over one persistent SSH stream; smallest half RTT bounds offset uncertainty"}
        win.put(clock_binary, "clock.exe")
        manifest["hosts"] = {"mac": run(["uname", "-sm"]).decode().strip(),
                             "windows": win.ps("[System.Runtime.InteropServices.RuntimeInformation]::OSDescription; [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture").decode().strip()}
        deps = run(["go", "list", "-deps", "./app/service/cmd/unic", "./app/service/cmd/unicd"], cwd=ROOT).decode()
        assert not any(s in deps for s in ("github.com/egoist/mygo", "/app/desktop/", "/uikit"))
        manifest["binary_sha256"] = {}
        for platform, names in (("mac", ("unic", "unicd")), ("windows", ("unic.exe", "unicd.exe"))):
            for name in names:
                p = bins / platform / name
                manifest["binary_sha256"][platform + "/" + name] = hashlib.sha256(p.read_bytes()).hexdigest()
                if platform == "windows":
                    win.put(p, name)
        remote_hashes = json.loads(win.ps("@(Get-ChildItem " + psquote(remote + r"\*.exe") + " | ForEach-Object { @{name=$_.Name;sha256=(Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()} }) | ConvertTo-Json -Compress"))
        for v in remote_hashes:
            expected = manifest["clock_probe"]["binary_sha256"] if v["name"] == "clock.exe" else manifest["binary_sha256"]["windows/" + v["name"]]
            assert v["sha256"] == expected
        check("matching production binaries transferred and verified; no UI dependency")
        mac = Mac(work, bins / "mac")
        Path(mac.home).mkdir()
        win.ps("New-Item -ItemType Directory -Force -Path " + psquote(win.home) + " | Out-Null")
        Path(mac.home, "config.toml").write_text('device_name = "p1-mac"\n[plugins]\nlua = ["example.lua-sensitive"]\n')
        cfg = work / "windows.toml"
        cfg.write_text('device_name = "p1-win"\n[plugins]\nlua = ["example.lua-emailtag"]\n[plugins.instances."example.lua-emailtag".config]\ntag = "p1-received"\n')
        win.put(cfg, r"state\config.toml")
        for node, example in ((mac, "sensitive"), (win, "emailtag")):
            for p in (ROOT / "plugins/examples/lua" / example).iterdir():
                name = "state/plugins/lua/" + example + "/" + p.name
                if node is win:
                    win.put(p, name.replace("/", "\\"))
                else:
                    dest = work / name
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(p, dest)
        mac.start()
        win.start()
        until("mac ready", lambda: ready(mac))
        until("Windows ready", lambda: ready(win))
        check("isolated headless daemons run on macOS and Windows")
        # Measure control overhead separately; do not subtract it from latency.
        baseline, clock_probes = [], []
        for _ in range(5):
            t = time.monotonic()
            wall = time.time_ns() / 1e6
            remote_ms = int(win.ps("[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()").strip())
            elapsed = (time.monotonic() - t) * 1000
            baseline.append(round(elapsed, 2))
            clock_probes.append({"offset_ms": remote_ms - (wall + elapsed / 2), "uncertainty_ms": elapsed / 2})
        manifest["ssh_probe_ms"] = baseline
        manifest["powershell_clock_estimate"] = min(clock_probes, key=lambda v: v["uncertainty_ms"])
        clock = win.clock_probe()
        manifest["windows_clock_estimate"] = clock
        manifest["clock_calibrations"] = [clock]
        first = js(mac, "submit", "--json", "--request-id", "unpaired", "p1 local commit")
        assert first["phase"] == "committed" and first["history_id"] and first["sync_disposition"] == "no_targets"
        check("unpaired submission commits locally with no_targets")
        ticket = output(mac, "addr")
        pair = output(win, "pair", ticket)
        a, b = identity(mac), identity(win)
        fp_a = until("mac approval", lambda: pending(mac, b))
        fp_b = until("Windows approval", lambda: pending(win, a))
        assert fp_a == fp_b and fp_a in pair
        before = len(history(win))
        js(mac, "submit", "--json", "--request-id", "pending", "not approved")
        time.sleep(3)
        assert len(history(win)) == before
        check("fingerprints match; no transmission before mutual approval")
        mac.cli("approve", b, fp_b)
        win.cli("approve", a, fp_a)
        def online():
            return "online" in output(mac, "peers") and "online" in output(win, "peers")
        until("both online", online, 90)
        check("mutual CLI approval establishes real two-host tailcat connection")
        def receive(node, old, size, digest, timeout=90):
            item = until("new remote history", lambda: next((v for v in history(node) if v["id"] not in old and v["origin"] == "remote_device" and v["total_bytes"] == size), None), timeout)
            got = node.exported_hashes(item["id"])
            assert len(got) == 1 and got[0]["size"] == size and got[0]["sha256"] == digest, "received content hash mismatch"
            return item
        # Bidirectional text plus retry/conflict and receive-side processor.
        path = work / "mail.txt"
        path.write_bytes("P1 双机 alpha@example.test\n".encode())
        old = {v["id"] for v in history(win)}
        first = submit(mac, "retry", "text", str(path))
        item = receive(win, old, path.stat().st_size, hashlib.sha256(path.read_bytes()).hexdigest())
        receipt = next(v for v in js(win, "received", "list") if v.get("history_id") == item["id"])
        assert win.cli("received", "show", receipt["receipt_id"])[1] == path.read_bytes()
        assert "p1-received" in item["tags"] and "p1-received" not in js(mac, "history", "show", first["history_id"], "--json")["tags"]
        with concurrent.futures.ThreadPoolExecutor(4) as pool:
            retries = list(pool.map(lambda _: submit(mac, "retry", "text", str(path)), range(4)))
        assert all(v == first for v in retries)
        assert js(mac, "submission", "retry") == first
        failure = mac.cli("submit", "--json", "--request-id", "retry", "different", ok=False)
        assert failure[0] != 0 and b"conflict" in failure[2]
        time.sleep(3)
        assert len([v for v in history(win) if v["id"] not in old]) == 1
        check("UTF-8 content verified; concurrent retries deduplicate; changed input conflicts; receive tags remain local")
        before = {v["id"] for v in history(win)}
        secret = js(mac, "submit", "--json", "--request-id", "secret", "password: P1-sentinel-20261110")
        detail = js(mac, "history", "show", secret["history_id"], "--json")
        assert detail["expires_at_ms"] > 0 and "sensitive" in detail["tags"] and secret["sync_disposition"] != "queued"
        time.sleep(4)
        assert {v["id"] for v in history(win)} == before
        check("capture processor preserves local content, tags and expiry while blocking sync")
        for key, value, expected in (("sync.enabled", "false", "disabled"),
                                     ('plugins.instances."builtin.tailcat".sync.text', "false", "policy_filtered")):
            def set_policy(value):
                if key == "sync.enabled":
                    mac.cli("config", "set", key, value)
                else:
                    # config set splits dotted paths; apply is the supported CLI
                    # route for plugin IDs containing dots.
                    policy = work / "text-policy.toml"
                    policy.write_text('device_name = "p1-mac"\n[plugins]\nlua = ["example.lua-sensitive"]\n[plugins.instances."builtin.tailcat".sync]\ntext = ' + value + '\n')
                    mac.cli("config", "apply", str(policy))
            set_policy(value)
            time.sleep(.4)
            old = {v["id"] for v in history(win)}
            result = js(mac, "submit", "--json", "--request-id", uuid.uuid4().hex, "policy test " + key)
            assert result["history_id"] and result["phase"] == "committed" and result["sync_disposition"] == expected
            time.sleep(3)
            assert {v["id"] for v in history(win)} == old
            set_policy("true")
            time.sleep(3)
            assert {v["id"] for v in history(win)} == old
            check(key + ": local commit succeeds, remote blocked, no replay on enable")
        # Every sample has distinct content, so deduplication cannot masquerade as transfer.
        specs = [("text", n) for n in (1, 1024, 65536, 1048576)]
        specs += [("png", n) for n in (16, 256, 1024)]
        specs += [("file", n) for n in (0, 1024, 1048576, 16777216, 104857600)]
        for direction, sender, receiver in (("mac_to_windows", mac, win), ("windows_to_mac", win, mac)):
            for kind, size in specs:
                clock = win.clock_probe()
                manifest["clock_calibrations"].append(clock)
                for index in range(opt.rounds):
                    seed = direction + kind + str(size) + str(index)
                    data = (png_bytes(size, size, seed) if kind == "png" else
                            hashlib.shake_256(seed.encode()).digest(size).hex().encode()[:size] if kind == "text" else
                            hashlib.shake_256(seed.encode()).digest(size))
                    payload = work / (uuid.uuid4().hex + (".png" if kind == "png" else ".bin"))
                    payload.write_bytes(data)
                    source = win.put(payload, "payload.bin") if sender is win else str(payload)
                    old = {v["id"] for v in history(receiver)}
                    start = time.monotonic()
                    result = submit(sender, uuid.uuid4().hex, kind, source)
                    sender_timing = sender.last_cli_timing.copy()
                    commit_ms = (time.monotonic() - start) * 1000
                    assert result["phase"] == "committed" and result["sync_disposition"] == "queued", result
                    received_item = receive(receiver, old, len(data), hashlib.sha256(data).hexdigest(), 180)
                    elapsed = (time.monotonic() - start) * 1000
                    receipt = next(v for v in js(receiver, "received", "list") if v.get("history_id") == received_item["id"])
                    offset = clock["offset_ms"]
                    sent_ms = sender_timing["started_ms"] - (offset if sender is win else 0)
                    received_ms = receipt["received_at_ms"] - (offset if receiver is win else 0)
                    manifest["samples"].append({"direction": direction, "type": kind, "bytes": len(data),
                                                "round": index + 1, "submit_ms": round(commit_ms, 2),
                                                "verified_ms": round(elapsed, 2),
                                                "receipt_estimate_ms": round(received_ms - sent_ms, 2),
                                                "receipt_estimate_uncertainty_ms": round(clock["uncertainty_ms"] + 1, 2),
                                                "sender_started_ms": sender_timing["started_ms"],
                                                "receiver_received_ms": receipt["received_at_ms"],
                                                "clock_offset_ms": offset,
                                                "sha256": hashlib.sha256(data).hexdigest()})
                    payload.unlink()
                    save()
                    print(f"SAMPLE {direction} {kind} {len(data)}B {elapsed:.2f}ms", flush=True)
        clock_after = win.clock_probe()
        manifest["clock_calibrations"].append(clock_after)
        manifest["clock_offset_change_ms"] = clock_after["offset_ms"] - manifest["windows_clock_estimate"]["offset_ms"]
        check("bidirectional size matrix: every sample exported and SHA256 verified")
        durable = js(mac, "history", "show", first["history_id"], "--json")
        mac.stop()
        win.stop()
        logs = (work / "daemon.log").read_bytes() + win.ps("[IO.File]::ReadAllText(" + psquote(remote + r"\daemon.out") + "); [IO.File]::ReadAllText(" + psquote(remote + r"\daemon.err") + ")")
        assert ticket.encode() not in logs and b"P1-sentinel-20261110" not in logs
        check("daemon logs exclude the pairing ticket and secret content sentinel")
        mac.start()
        win.start()
        until("mac restarted", lambda: ready(mac))
        until("Windows restarted", lambda: ready(win))
        assert identity(mac) == a and identity(win) == b
        assert js(mac, "history", "show", first["history_id"], "--json") == durable
        until("restart reconnect", online, 90)
        old = {v["id"] for v in history(win)}
        path.write_bytes(b"post-restart P1")
        submit(mac, "post-restart", "text", str(path))
        receive(win, old, path.stat().st_size, hashlib.sha256(path.read_bytes()).hexdigest())
        check("restart preserves identity, trust and history; reconnect transfers new content")
        win.cli("unpair", a)
        old = {v["id"] for v in history(win)}
        js(mac, "submit", "--json", "--request-id", "revoked", "revoked test")
        time.sleep(4)
        assert {v["id"] for v in history(win)} == old
        check("revocation blocks subsequent remote history")
        manifest["source_unchanged"] = all(hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == digest for name, digest in source.items())
        assert manifest["source_unchanged"]
        manifest["status"] = "passed"
    except Exception as e:
        manifest["status"] = "failed"
        # Keep errors bounded. Never serialize raw pairing commands or addresses.
        manifest["failure"] = ("external command failed (details kept out of manifest)" if isinstance(e, subprocess.SubprocessError) else str(e)[:1000])
        raise
    finally:
        for node in (mac, win):
            if node:
                try:
                    node.stop()
                except Exception:
                    manifest.setdefault("cleanup_errors", []).append("failed to stop owned " + type(node).__name__)
        if manifest.get("cleanup_errors"):
            manifest["status"] = "failed"
        manifest["finished_at"] = time.strftime("%Y-%m-%dT%H:%M:%S%z")
        save()
        print("MANIFEST " + str(opt.output / "manifest.json"), flush=True)


if __name__ == "__main__":
    main()
