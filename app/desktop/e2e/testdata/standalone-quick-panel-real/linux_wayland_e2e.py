#!/usr/bin/env python3
"""Real-VM Linux Wayland standalone Quick Panel E2E (N01a KWin Copy/lease + N01b Niri auto-paste).

Usage:
  WAYLAND_DISPLAY=wayland-n01 env -u DISPLAY python3 linux_wayland_e2e.py \
      --mode kwin --bin-dir /path/to/bin --out-dir /path/to/out --work-dir /path/to/work --guest-uuid <UUID>

  WAYLAND_DISPLAY=wayland-1 env -u DISPLAY python3 linux_wayland_e2e.py \
      --mode niri --bin-dir /path/to/bin --out-dir /path/to/out --work-dir /path/to/work --guest-uuid <UUID>
"""
import argparse
import base64
import hashlib
import http.client
import json
import os
import signal
import socket
import struct
import subprocess
import time
import urllib.parse
import zlib
from pathlib import Path


class UnixHTTPConnection(http.client.HTTPConnection):
    def __init__(self, sock_path, timeout=15):
        super().__init__("localhost", timeout=timeout)
        self.sock_path = str(sock_path)

    def connect(self):
        s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        s.settimeout(self.timeout)
        s.connect(self.sock_path)
        self.sock = s


def api_req(sock_path, method, path, body=None, timeout=20):
    conn = UnixHTTPConnection(sock_path, timeout=timeout)
    try:
        headers = {}
        payload = None
        if body is not None:
            payload = json.dumps(body).encode("utf-8")
            headers["Content-Type"] = "application/json"
        conn.request(method, path, body=payload, headers=headers)
        resp = conn.getresponse()
        raw = resp.read()
        if resp.status >= 400:
            raise RuntimeError(f"{method} {path} -> HTTP {resp.status}: {raw.decode('utf-8', 'replace')}")
        return json.loads(raw.decode("utf-8")) if raw else None
    finally:
        conn.close()


def auto_rpc(sock_path, method, params=None, timeout=8):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.settimeout(timeout)
        s.connect(str(sock_path))
        req = {"id": 1, "method": method}
        if params is not None:
            req["params"] = params
        s.sendall((json.dumps(req) + "\n").encode("utf-8"))
        line = s.makefile("rb").readline()
        if not line:
            raise RuntimeError(f"empty automation reply for {method}")
        resp = json.loads(line.decode("utf-8"))
        if resp.get("error"):
            raise RuntimeError(f"automation {method} error: {resp['error']}")
        return resp.get("result")


def make_png(width=64, height=64):
    raw_rows = []
    for y in range(height):
        row = bytearray([0])
        for x in range(width):
            row.extend([(x * 4) % 256, (y * 4) % 256, ((x + y) * 2) % 256, 255])
        raw_rows.append(bytes(row))
    idat = zlib.compress(b"".join(raw_rows), 6)

    def chunk(ctype, data):
        crc = zlib.crc32(ctype + data) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + ctype + data + struct.pack(">I", crc)

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b"")


def sha256_bytes(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def sha256_file(p: Path) -> str:
    return sha256_bytes(p.read_bytes())


def wait_until(desc, fn, timeout=20.0, interval=0.12):
    deadline = time.monotonic() + timeout
    last_err = None
    while time.monotonic() < deadline:
        try:
            res = fn()
            if res:
                return res
        except Exception as exc:
            last_err = exc
        time.sleep(interval)
    raise TimeoutError(f"timed out waiting for {desc} (last_err={last_err})")


def walk_nodes(tree):
    if isinstance(tree, dict) and isinstance(tree.get("nodes"), list):
        return tree["nodes"]
    return []


def find_node(tree, role=None, name_sub=None):
    for n in walk_nodes(tree):
        r = n.get("role") or ""
        name = n.get("label") or n.get("value") or ""
        if role and r != role:
            continue
        if name_sub and name_sub not in name:
            continue
        return n
    return None


def node_bounds(node):
    b = node.get("bounds") or {}
    return b.get("x", 0), b.get("y", 0), b.get("width", 0), b.get("height", 0)


def ps_snapshot():
    out = subprocess.check_output(["ps", "-u", str(os.getuid()), "-o", "pid=,ppid=,comm=,args="], text=True)
    rows = []
    for line in out.splitlines():
        parts = line.strip().split(None, 3)
        if len(parts) >= 4:
            rows.append({"pid": int(parts[0]), "ppid": int(parts[1]), "comm": parts[2], "args": parts[3]})
    return rows


def unic_processes(profile: Path):
    marker = str(profile)
    rows = []
    for r in ps_snapshot():
        if "unic" in r["comm"] or "unic-" in r["args"] or "unicd" in r["args"]:
            try:
                raw_env = Path(f"/proc/{r['pid']}/environ").read_bytes().decode("utf-8", "replace")
                if f"UNIC_HOME={marker}" in raw_env:
                    rows.append(r)
            except OSError:
                pass
    return rows


def proc_matches_profile_exe(pid: int, profile: Path, expected_exes: set) -> bool:
    if pid <= 0 or pid == os.getpid():
        return False
    try:
        exe = Path(os.readlink(f"/proc/{pid}/exe")).resolve()
        if exe not in expected_exes:
            return False
        entries = Path(f"/proc/{pid}/environ").read_bytes().split(b"\x00")
        target = f"UNIC_HOME={profile}".encode("utf-8")
        return target in entries
    except OSError:
        return False


def wl_env():
    env = {k: v for k, v in os.environ.items() if k != "DISPLAY"}
    env["XDG_SESSION_TYPE"] = "wayland"
    env["GDK_BACKEND"] = "wayland"
    return env


def wl_paste(mime=None, no_newline=False, timeout=5):
    cmd = ["wl-paste"]
    if no_newline:
        cmd.append("--no-newline")
    if mime:
        cmd += ["--type", mime]
    res = subprocess.run(cmd, env=wl_env(), capture_output=True, timeout=timeout)
    if res.returncode != 0:
        raise RuntimeError(f"{cmd} failed ({res.returncode}): {res.stderr.decode('utf-8', 'replace')}")
    return res.stdout


WAYLAND_EDITOR_SCRIPT = r"""
import json, os, sys, gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, GLib

state_file = sys.argv[1]
win = Gtk.Window(title="Wayland External Editor")
win.set_default_size(460, 260)
tv = Gtk.TextView()
buf = tv.get_buffer()
win.add(tv)

def write_state(*_):
    text = buf.get_text(buf.get_start_iter(), buf.get_end_iter(), True)
    tmp = state_file + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump({"pid": os.getpid(), "text": text, "focused": win.is_active()}, f, ensure_ascii=False)
    os.replace(tmp, state_file)

buf.connect("changed", write_state)
win.connect("notify::is-active", write_state)
win.connect("destroy", Gtk.main_quit)
win.show_all()
tv.grab_focus()
win.present()
GLib.timeout_add(120, lambda: (write_state(), True)[1])
Gtk.main()
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=["kwin", "niri"], required=True)
    parser.add_argument("--bin-dir", required=True, type=Path)
    parser.add_argument("--out-dir", required=True, type=Path)
    parser.add_argument("--work-dir", required=True, type=Path)
    parser.add_argument("--guest-uuid", required=True)
    args = parser.parse_args()

    bin_dir = args.bin_dir.resolve()
    out_dir = args.out_dir.resolve()
    work_dir = args.work_dir.resolve()
    if out_dir.exists():
        raise SystemExit(f"refusing to overwrite existing --out-dir (preserve prior evidence): {out_dir}")
    if work_dir.exists():
        raise SystemExit(f"refusing to overwrite existing --work-dir (preserve prior workspace): {work_dir}")
    out_dir.mkdir(parents=True, mode=0o700)
    work_dir.mkdir(parents=True, mode=0o700)
    (work_dir / "discovery").mkdir(parents=True, mode=0o700)

    def env_for(profile: Path, e2e_tag: bool):
        env = {k: v for k, v in wl_env().items() if not k.startswith(("UNIC_", "MYGO_"))}
        disc = work_dir / "discovery"
        env.update(
            {
                "UNIC_HOME": str(profile),
                "UNIC_SOCK": str(profile / "unic.sock"),
                "UNIC_RENDEZVOUS_URL": "http://127.0.0.1:1",
                "HOME": str(disc),
                "XDG_CONFIG_HOME": str(disc / "config"),
                "XDG_DATA_HOME": str(disc / "data"),
                "LANG": "zh_CN.UTF-8",
                "LC_ALL": "zh_CN.UTF-8",
            }
        )
        if e2e_tag:
            env["UNIC_DESKTOP_E2E_PROFILE"] = str(profile)
            env["UNIC_DESKTOP_E2E_GUEST"] = args.guest_uuid
            env["MYGO_AUTOMATION"] = ""
        return env

    def role_probe(bin_path: Path, profile: Path, role: str, e2e_tag: bool):
        res = subprocess.run(
            [str(bin_path), "--role", role, "--command", "probe"],
            env=env_for(profile, e2e_tag),
            capture_output=True,
            text=True,
            timeout=10,
        )
        return json.loads(res.stdout) if res.returncode == 0 else None

    timeline = []
    procs = []
    wl_owners = []
    owned_profiles = []
    manifest = {
        "case": f"N01-{args.mode}-linux-wayland-standalone-quick-panel",
        "status": "FAIL",
        "mode": args.mode,
        "guest_uuid": args.guest_uuid,
        "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "environment": {
            "os": subprocess.check_output(["uname", "-a"], text=True).strip(),
            "wayland_display": os.environ.get("WAYLAND_DISPLAY", ""),
            "display_env": os.environ.get("DISPLAY", ""),
            "virtualization": subprocess.check_output(["systemd-detect-virt", "--vm"], text=True).strip(),
        },
        "binary_sha256": {p.name: sha256_file(p) for p in sorted(bin_dir.iterdir()) if p.is_file()},
        "checks": [],
        "process_snapshots": {},
        "clipboard_readbacks": {},
        "paste_receipts": {},
        "screenshots": [],
        "timeline": timeline,
    }

    def log_event(step, detail=None):
        entry = {"at": round(time.time(), 3), "step": step}
        if detail is not None:
            entry["detail"] = detail
        timeline.append(entry)
        print(f"[N01-{args.mode}] {step}: {json.dumps(detail, ensure_ascii=False) if detail is not None else ''}", flush=True)

    def spawn(cmd, env, log_name):
        f = open(out_dir / log_name, "wb")
        p = subprocess.Popen(cmd, env=env, stdout=f, stderr=subprocess.STDOUT)
        procs.append((p, f, log_name))
        return p

    def capture_shot(sock, name, wid):
        res = auto_rpc(sock, "capture", {"window": wid})
        png_bytes = base64.b64decode(res["png"])
        (out_dir / name).write_bytes(png_bytes)
        manifest["screenshots"].append({"file": name, "bytes": len(png_bytes), "sha256": sha256_bytes(png_bytes)})

    def init_profile(name: str, e2e_tag: bool, sync_enabled: bool):
        p = work_dir / name
        p.mkdir(mode=0o700)
        if e2e_tag:
            (p / "e2e-owner").write_text("desktop-e2e")
        (p / "desktop-setup.json").write_text('{"complete":true}\n')
        (p / "config.toml").write_text(
            f"[telemetry]\nenabled = false\n[sync]\nenabled = {'true' if sync_enabled else 'false'}\n"
            f"[desktop]\nmode = 'normal'\nquick_panel_shortcut = ''\n"
        )
        subprocess.run([str(bin_dir / "unic"), "migrate", "--initialize-empty", "--json"], env=env_for(p, e2e_tag), check=True, capture_output=True, timeout=20)
        owned_profiles.append((p, e2e_tag))
        return p

    def start_wl_copy(text: str):
        p = subprocess.Popen(["wl-copy", "--foreground", "--type", "text/plain;charset=utf-8"], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=wl_env())
        p.stdin.write(text.encode("utf-8"))
        p.stdin.close()
        wl_owners.append(p)
        wait_until(f"wl-paste == {text}", lambda: wl_paste(no_newline=True).decode("utf-8", "replace") == text, timeout=5)
        return p

    try:
        assert manifest["environment"]["wayland_display"], "WAYLAND_DISPLAY required"
        assert not manifest["environment"]["display_env"], "DISPLAY must be unset for pure Wayland"
        assert manifest["environment"]["virtualization"] in ("qemu", "kvm")
        assert subprocess.run(["pgrep", "-u", str(os.getuid()), "-x", "spice-vdagent"], capture_output=True).returncode == 1
        forbidden = {"klipper", "cliphist", "copyq", "parcellite", "plasmashell", "waybar", "swaybar", "gnome-shell", "spice-vdagent"}
        assert not [r for r in ps_snapshot() if r["comm"] in forbidden]
        manifest["checks"].append("pure-wayland-environment-verified")

        if args.mode == "niri":
            manifest["environment"]["compositor"] = subprocess.check_output(["niri", "--version"], text=True).strip()
            prof = init_profile("main", e2e_tag=True, sync_enabled=False)
            spawn([str(bin_dir / "unicd")], env_for(prof, True), "unicd.log")
            wait_until("unicd ready", lambda: api_req(prof / "unic.sock", "GET", "/v1/service"))
            text2 = "Wayland 第二条保持打开粘贴"
            text1 = "Wayland Niri 自动粘贴验证 🚀"
            api_req(prof / "unic.sock", "POST", "/v1/submissions", {"request_id": "wl-2", "snapshot": {"parts": [{"format": "text/plain", "data": base64.b64encode(text2.encode()).decode()}]}})
            time.sleep(0.15)
            api_req(prof / "unic.sock", "POST", "/v1/submissions", {"request_id": "wl-1", "snapshot": {"parts": [{"format": "text/plain", "data": base64.b64encode(text1.encode()).decode()}]}})

            editor_state = work_dir / "editor-state.json"
            spawn(["python3", "-c", WAYLAND_EDITOR_SCRIPT, str(editor_state)], wl_env(), "wayland-editor.log")
            wait_until("external editor focused", lambda: editor_state.exists() and json.loads(editor_state.read_text()).get("focused") is True, timeout=15)

            panel_sock = prof / "automation-panel.sock"
            subprocess.run([str(bin_dir / "unic-desktop-e2e"), "--quick-panel"], env=env_for(prof, True), check=True, timeout=20)
            def visible_panel():
                for w in (auto_rpc(panel_sock, "windows").get("windows") or []):
                    if w.get("visible") and w.get("kind") == "content" and "快捷面板" in w.get("title", ""):
                        return w
                return None

            win = wait_until("panel visible and focused", lambda: (lambda w: w if w and w.get("focused") else None)(visible_panel()), timeout=15)
            wait_until("panel shows paste target", lambda: any("粘贴到" in (n.get("label") or "") for n in walk_nodes(auto_rpc(panel_sock, "tree", {"window": win["id"]}))), timeout=10)
            capture_shot(panel_sock, "01-niri-wayland-paste-ready.png", win["id"])
            manifest["checks"].append("niri-wayland-paste-target-enabled")

            auto_rpc(panel_sock, "input", {"window": win["id"], "events": [{"type": "key", "key": "Enter"}]})
            wait_until("editor pasted text1", lambda: editor_state.exists() and json.loads(editor_state.read_text()).get("text") == text1, timeout=12)
            time.sleep(0.8)
            ed1 = json.loads(editor_state.read_text())
            assert ed1["text"] == text1
            wait_until("panel hidden", lambda: len(auto_rpc(panel_sock, "windows").get("windows") or []) == 0, timeout=8)
            assert wl_paste(mime="text/plain;charset=utf-8", no_newline=True).decode() == text1
            manifest["paste_receipts"]["enter_single_paste"] = {"editor_pid": ed1["pid"], "inserted_text": ed1["text"], "sha256": sha256_bytes(text1.encode())}
            manifest["checks"].append("niri-wayland-enter-cross-app-auto-paste-single-insertion")

            subprocess.run([str(bin_dir / "unic-desktop-e2e"), "--quick-panel"], env=env_for(prof, True), check=True, timeout=15)
            win = wait_until("panel reopened", lambda: (lambda w: w if w and w.get("focused") else None)(visible_panel()), timeout=10)
            auto_rpc(panel_sock, "input", {"window": win["id"], "events": [{"type": "key", "key": "Down"}]})
            time.sleep(0.25)
            auto_rpc(panel_sock, "input", {"window": win["id"], "events": [{"type": "key", "key": "CmdOrCtrl+Enter"}]})
            want_both = text1 + text2
            wait_until("editor pasted text2", lambda: editor_state.exists() and json.loads(editor_state.read_text()).get("text") == want_both, timeout=12)
            win_reopened = wait_until(
                "panel auto reopened and focused",
                lambda: (lambda w: w if w and w.get("focused") else None)(visible_panel()),
                timeout=8,
            )
            capture_shot(panel_sock, "02-niri-wayland-reopened-after-ctrl-enter.png", win_reopened["id"])
            manifest["paste_receipts"]["ctrl_enter_keep_open_paste"] = {"editor_text": want_both, "sha256": sha256_bytes(want_both.encode()), "panel_reopened": True, "panel_focused": True}
            manifest["checks"].append("niri-wayland-ctrl-enter-paste-and-reopen")
        else:
            manifest["environment"]["compositor"] = subprocess.check_output(["kwin_wayland", "--version"], text=True).strip()
            prod_prof = init_profile("prod", e2e_tag=False, sync_enabled=False)
            prod_daemon = spawn([str(bin_dir / "unicd")], env_for(prod_prof, False), "prod-unicd.log")
            wait_until("prod unicd ready", lambda: api_req(prod_prof / "unic.sock", "GET", "/v1/service"))
            subprocess.run([str(bin_dir / "unic-desktop"), "--quick-panel"], env=env_for(prod_prof, False), check=True, timeout=20)
            prod_panel = wait_until("prod panel probe", lambda: role_probe(bin_dir / "unic-desktop", prod_prof, "quick-panel", False))
            assert len(unic_processes(prod_prof)) == 2
            subprocess.run([str(bin_dir / "unic-desktop"), "--quick-panel"], env=env_for(prod_prof, False), check=True, timeout=20)
            assert role_probe(bin_dir / "unic-desktop", prod_prof, "quick-panel", False)["pid"] == prod_panel["pid"]
            os.kill(prod_panel["pid"], signal.SIGTERM)
            prod_daemon.terminate()
            prod_daemon.wait(timeout=10)
            manifest["checks"].append("production-binary-standalone-quick-panel-launch-and-refocus")

            main_prof = init_profile("main", e2e_tag=True, sync_enabled=True)
            peer_prof = init_profile("peer", e2e_tag=False, sync_enabled=True)
            main_daemon = spawn([str(bin_dir / "unicd")], env_for(main_prof, True), "main-unicd.log")
            peer_daemon = spawn([str(bin_dir / "unicd")], env_for(peer_prof, False), "peer-unicd.log")
            wait_until("main unicd ready", lambda: api_req(main_prof / "unic.sock", "GET", "/v1/service"))
            wait_until("peer unicd ready", lambda: api_req(peer_prof / "unic.sock", "GET", "/v1/service"))
            ticket = api_req(peer_prof / "unic.sock", "POST", "/v1/pairing-ticket")["ticket"]
            api_req(main_prof / "unic.sock", "POST", "/v1/peers", {"addr": ticket})
            def approve_all(sock):
                for req in api_req(sock, "GET", "/v1/pending") or []:
                    api_req(sock, "POST", f"/v1/pending/{req['id']}/approve", {"fingerprint": req["fingerprint"], "revision": req["revision"], "bindings": req.get("bindings", [])})
                return True
            wait_until("peers connected", lambda: approve_all(peer_prof / "unic.sock") and approve_all(main_prof / "unic.sock") and any(p.get("connected") for p in (api_req(main_prof / "unic.sock", "GET", "/v1/status").get("peers") or [])), timeout=30)

            managed_body = b"N01 Wayland managed cached file payload 2026\nutf8-\xe4\xb8\xad\xe6\x96\x87\n"
            managed_sha = sha256_bytes(managed_body)
            src_file = work_dir / "n01-managed-report.txt"
            src_file.write_bytes(managed_body)
            api_req(peer_prof / "unic.sock", "POST", "/v1/submissions", {"request_id": "n01-peer-file", "snapshot": {"files": [{"name": "n01-managed-report.txt", "path": str(src_file), "size": len(managed_body)}]}})
            managed_item = wait_until("managed file received", lambda: next((it for it in (api_req(main_prof / "unic.sock", "GET", "/v1/history").get("items") or []) if it.get("origin") == "remote_device" and it.get("kind") in ("file", "files")), None), timeout=25)
            peer_daemon.terminate(); peer_daemon.wait(timeout=10)
            main_daemon.terminate(); main_daemon.wait(timeout=10)
            main_daemon = spawn([str(bin_dir / "unicd")], env_for(main_prof, True), "main-unicd-2.log")
            main_svc = wait_until("main unicd restarted", lambda: api_req(main_prof / "unic.sock", "GET", "/v1/service"))

            png_bytes = make_png(64, 64)
            png_sha = sha256_bytes(png_bytes)
            text_payload = "N01 Wayland 独立快捷面板文本验证 🚀"
            text_sha = sha256_bytes(text_payload.encode())
            api_req(main_prof / "unic.sock", "POST", "/v1/submissions", {"request_id": "n01-png", "snapshot": {"parts": [{"format": "image/png", "data": base64.b64encode(png_bytes).decode()}]}})
            time.sleep(0.15)
            api_req(main_prof / "unic.sock", "POST", "/v1/submissions", {"request_id": "n01-text", "snapshot": {"parts": [{"format": "text/plain", "data": base64.b64encode(text_payload.encode()).decode()}]}})

            baseline_owner = start_wl_copy("N01-WAYLAND-BASELINE")
            panel_sock = main_prof / "automation-panel.sock"
            subprocess.run([str(bin_dir / "unic-desktop-e2e"), "--quick-panel"], env=env_for(main_prof, True), check=True, timeout=20)
            def visible_win():
                for w in (auto_rpc(panel_sock, "windows").get("windows") or []):
                    if w.get("visible") and w.get("kind") == "content":
                        return w
                return None
            win = wait_until("panel visible", visible_win, timeout=15)
            panel_role = role_probe(bin_dir / "unic-desktop-e2e", main_prof, "quick-panel", True)
            assert len(unic_processes(main_prof)) == 2
            wait_until("text row visible", lambda: find_node(auto_rpc(panel_sock, "tree", {"window": win["id"]}), name_sub="N01 Wayland"), timeout=10)
            capture_shot(panel_sock, "01-wayland-panel-open.png", win["id"])
            auto_rpc(panel_sock, "input", {"window": win["id"], "events": [{"type": "key", "key": "Enter"}]})
            wait_until(
                "panel reports paste unavailable",
                lambda: find_node(auto_rpc(panel_sock, "tree", {"window": win["id"]}), name_sub="不支持自动粘贴"),
                timeout=8,
            )
            time.sleep(0.5)
            # On Wayland, wl-copy --foreground exits as soon as any client replaces clipboard ownership.
            assert baseline_owner.poll() is None, "clipboard ownership changed during refused Enter"
            assert wl_paste(no_newline=True).decode() == "N01-WAYLAND-BASELINE"
            manifest["checks"].append("wayland-copy-fallback-enter-refusal")

            auto_rpc(panel_sock, "input", {"window": win["id"], "events": [{"type": "key", "key": "CmdOrCtrl+C"}]})
            wait_until("hidden after text", lambda: len(auto_rpc(panel_sock, "windows").get("windows") or []) == 0, timeout=10)
            assert sha256_bytes(wl_paste(mime="text/plain;charset=utf-8", no_newline=True)) == text_sha
            manifest["checks"].append("wayland-windowless-text-selection-serving")

            subprocess.run([str(bin_dir / "unic-desktop-e2e"), "--quick-panel"], env=env_for(main_prof, True), check=True, timeout=15)
            win = wait_until("reopened for png", visible_win, timeout=10)
            auto_rpc(panel_sock, "input", {"window": win["id"], "events": [{"type": "key", "key": "Down"}, {"type": "key", "key": "CmdOrCtrl+C"}]})
            wait_until("hidden after png", lambda: len(auto_rpc(panel_sock, "windows").get("windows") or []) == 0, timeout=10)
            assert sha256_bytes(wl_paste(mime="image/png")) == png_sha
            manifest["checks"].append("wayland-windowless-png-selection-serving")

            subprocess.run([str(bin_dir / "unic-desktop-e2e"), "--quick-panel"], env=env_for(main_prof, True), check=True, timeout=15)
            win = wait_until("reopened for file", visible_win, timeout=10)
            fnode = find_node(auto_rpc(panel_sock, "tree", {"window": win["id"]}), name_sub="n01-managed-report.txt")
            x, y, w, h = node_bounds(fnode)
            auto_rpc(panel_sock, "input", {"window": win["id"], "events": [{"type": "click", "x": x + w / 2, "y": y + h / 2}, {"type": "key", "key": "CmdOrCtrl+C"}]})
            wait_until("hidden after file", lambda: len(auto_rpc(panel_sock, "windows").get("windows") or []) == 0, timeout=10)
            uris = [l.strip() for l in wl_paste(mime="text/uri-list").decode().splitlines() if l.strip() and not l.startswith("#")]
            assert len(uris) == 1, f"expected exactly 1 file URI, got {uris!r}"
            parsed_uri = urllib.parse.urlparse(uris[0])
            assert parsed_uri.scheme == "file", f"expected local file URI, got {uris[0]!r}"
            inbox_root = (main_prof / "inbox").resolve()
            managed_path = Path(urllib.parse.unquote(parsed_uri.path)).resolve()
            assert managed_path.is_relative_to(inbox_root), f"managed file outside inbox: {managed_path} not under {inbox_root}"
            assert managed_path.exists() and sha256_file(managed_path) == managed_sha
            api_req(main_prof / "unic.sock", "DELETE", f"/v1/history/{managed_item['id']}?revision={managed_item['revision']}")
            time.sleep(0.5)
            assert managed_path.exists()
            start_wl_copy("N01-EXTERNAL-REPLACEMENT")
            wait_until("managed file reclaimed", lambda: not managed_path.exists(), timeout=12)
            manifest["checks"].append("wayland-managed-file-lease-and-reclaim")

            tray_proc = spawn([str(bin_dir / "unic-desktop-e2e"), "--role", "tray"], env_for(main_prof, True), "main-tray.log")
            wait_until("tray ready", lambda: role_probe(bin_dir / "unic-desktop-e2e", main_prof, "tray", True), timeout=15)
            h0 = len(api_req(main_prof / "unic.sock", "GET", "/v1/history").get("items") or [])
            subprocess.run([str(bin_dir / "unic-desktop-e2e"), "--quick-panel"], env=env_for(main_prof, True), check=True, timeout=15)
            win = wait_until("reopened with tray", visible_win, timeout=10)
            auto_rpc(panel_sock, "input", {"window": win["id"], "events": [{"type": "key", "key": "CmdOrCtrl+C"}]})
            wait_until("hidden with tray", lambda: len(auto_rpc(panel_sock, "windows").get("windows") or []) == 0, timeout=10)
            assert sha256_bytes(wl_paste(mime="text/plain;charset=utf-8", no_newline=True)) == text_sha
            time.sleep(1.5)
            assert len(api_req(main_prof / "unic.sock", "GET", "/v1/history").get("items") or []) == h0
            tray_proc.terminate(); tray_proc.wait(timeout=10)
            svc_after = api_req(main_prof / "unic.sock", "GET", "/v1/service")
            panel_after = role_probe(bin_dir / "unic-desktop-e2e", main_prof, "quick-panel", True)
            assert svc_after["id"] == main_svc["id"] and svc_after["pid"] == main_svc["pid"]
            assert panel_after is not None and panel_after["pid"] == panel_role["pid"]
            subprocess.run([str(bin_dir / "unic-desktop-e2e"), "--quick-panel"], env=env_for(main_prof, True), check=True, timeout=15)
            win = wait_until("reopened after tray exit", visible_win, timeout=10)
            auto_rpc(panel_sock, "input", {"window": win["id"], "events": [{"type": "key", "key": "Down"}, {"type": "key", "key": "CmdOrCtrl+C"}]})
            wait_until("hidden after post-tray png", lambda: len(auto_rpc(panel_sock, "windows").get("windows") or []) == 0, timeout=10)
            assert sha256_bytes(wl_paste(mime="image/png")) == png_sha
            assert api_req(main_prof / "unic.sock", "GET", "/v1/service")["id"] == main_svc["id"]
            assert role_probe(bin_dir / "unic-desktop-e2e", main_prof, "quick-panel", True)["pid"] == panel_role["pid"]
            manifest["checks"].append("wayland-tray-coexistence-and-exit-preservation")

        manifest["status"] = "PASS"
        manifest["finished_at_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        log_event("PASSED", {"checks": manifest["checks"]})
    except Exception as exc:
        manifest["error"] = f"{type(exc).__name__}: {exc}"
        log_event("FAILED", manifest["error"])
        raise
    finally:
        for p in wl_owners:
            try:
                p.terminate(); p.wait(timeout=2)
            except Exception:
                pass
        for p, f, _ in procs:
            try:
                if p.poll() is None:
                    p.terminate(); p.wait(timeout=5)
            except Exception:
                try:
                    p.kill(); p.wait(timeout=2)
                except Exception:
                    pass
            try:
                f.close()
            except Exception:
                pass
        expected_exes = {
            p.resolve()
            for name in ("unic-desktop", "unic-desktop-e2e", "unicd")
            if (p := bin_dir / name).exists()
        }
        for prof, e2e_tag in owned_profiles:
            probe_bin = bin_dir / ("unic-desktop-e2e" if e2e_tag else "unic-desktop")
            if not probe_bin.exists():
                continue
            for role in ("gui", "quick-panel", "tray"):
                try:
                    info = role_probe(probe_bin, prof, role, e2e_tag)
                except Exception:
                    info = None
                if info and info.get("profile") == str(prof):
                    pid = int(info.get("pid") or 0)
                    if proc_matches_profile_exe(pid, prof, expected_exes):
                        try:
                            os.kill(pid, signal.SIGTERM)
                        except OSError:
                            pass
        time.sleep(0.4)
        (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
        sums = [f"{sha256_file(p)}  {p.name}" for p in sorted(out_dir.iterdir()) if p.is_file() and p.name != "SHA256SUMS"]
        (out_dir / "SHA256SUMS").write_text("\n".join(sums) + "\n")


if __name__ == "__main__":
    main()
