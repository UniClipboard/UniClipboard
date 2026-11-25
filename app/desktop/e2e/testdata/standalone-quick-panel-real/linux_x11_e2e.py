#!/usr/bin/env python3
"""Real-VM Linux X11 standalone Quick Panel E2E (N02).

Runs against a pure native X11 server (e.g. Xvfb +extension XTEST + openbox EWMH WM)
with WAYLAND_DISPLAY unset, using the untouched production unic/unicd/unic-desktop
binaries, real physical XTest keyboard events, a real external PyGTK3 Gtk.TextView
editor window, and independent PyGTK3/xsel X11 CLIPBOARD verification.
"""
import argparse
import base64
import ctypes
import hashlib
import http.client
import json
import os
import shutil
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


def make_png(width=64, height=64):
    raw_rows = []
    for y in range(height):
        row = bytearray([0])
        for x in range(width):
            row.extend([(x * 3 + 17) % 256, (y * 5 + 31) % 256, ((x ^ y) * 4) % 256, 255])
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


def x11_env(base_env=None):
    env = dict(base_env if base_env is not None else os.environ)
    env.pop("WAYLAND_DISPLAY", None)
    env["XDG_SESSION_TYPE"] = "x11"
    env["GDK_BACKEND"] = "x11"
    env["DISPLAY"] = os.environ.get("DISPLAY", ":98")
    return env


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


def x11_active_window():
    out = subprocess.check_output(["xprop", "-root", "_NET_ACTIVE_WINDOW"], env=x11_env(), text=True, timeout=5)
    parts = out.strip().split()
    wid = parts[-1].rstrip(",") if parts else "0x0"
    if wid in ("0x0", ""):
        return "0x0", None
    try:
        pout = subprocess.check_output(["xprop", "-id", wid, "_NET_WM_PID"], env=x11_env(), text=True, timeout=5)
        if "=" in pout:
            return wid, int(pout.split("=")[-1].strip())
    except Exception:
        pass
    return wid, None


class XTestSender:
    def __init__(self):
        self.x11 = ctypes.CDLL("libX11.so.6")
        self.xtst = ctypes.CDLL("libXtst.so.6")
        self.x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
        self.x11.XOpenDisplay.restype = ctypes.c_void_p
        self.x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
        self.x11.XStringToKeysym.restype = ctypes.c_ulong
        self.x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        self.x11.XKeysymToKeycode.restype = ctypes.c_ubyte
        self.x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
        self.xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
        self.dpy = self.x11.XOpenDisplay(os.environ.get("DISPLAY", ":98").encode("ascii"))
        if not self.dpy:
            raise RuntimeError("XOpenDisplay failed")

    def keycode(self, sym_name: str) -> int:
        sym = self.x11.XStringToKeysym(sym_name.encode("ascii"))
        kc = self.x11.XKeysymToKeycode(self.dpy, sym) if sym else 0
        if not kc:
            raise RuntimeError(f"no keycode for {sym_name}")
        return int(kc)

    def tap(self, sym_name: str, ctrl: bool = False):
        kc = self.keycode(sym_name)
        ctrl_kc = self.keycode("Control_L") if ctrl else 0
        if ctrl:
            self.xtst.XTestFakeKeyEvent(self.dpy, ctrl_kc, 1, 0)
            self.x11.XSync(self.dpy, 0)
            time.sleep(0.03)
        self.xtst.XTestFakeKeyEvent(self.dpy, kc, 1, 0)
        self.x11.XSync(self.dpy, 0)
        time.sleep(0.03)
        self.xtst.XTestFakeKeyEvent(self.dpy, kc, 0, 0)
        self.x11.XSync(self.dpy, 0)
        if ctrl:
            time.sleep(0.03)
            self.xtst.XTestFakeKeyEvent(self.dpy, ctrl_kc, 0, 0)
            self.x11.XSync(self.dpy, 0)

    def close(self):
        if self.dpy:
            self.x11.XCloseDisplay(self.dpy)
            self.dpy = None


X11_CLIP_READER = r"""
import sys, gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, Gdk

mime = sys.argv[1]
cb = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
if mime == "text":
    val = cb.wait_for_text()
    if val is None:
        sys.exit(2)
    sys.stdout.buffer.write(val.encode("utf-8"))
else:
    atom = Gdk.Atom.intern(mime, False)
    sel = cb.wait_for_contents(atom)
    if sel is None:
        sys.exit(2)
    data = sel.get_data()
    if data is None:
        sys.exit(2)
    sys.stdout.buffer.write(data)
"""


def x11_clip_read(mime: str) -> bytes:
    res = subprocess.run(["python3", "-c", X11_CLIP_READER, mime], env=x11_env(), capture_output=True, timeout=8)
    if res.returncode != 0:
        raise RuntimeError(f"x11_clip_read({mime}) failed ({res.returncode}): {res.stderr.decode('utf-8', 'replace')}")
    return res.stdout


X11_EDITOR_SCRIPT = r"""
import json, os, sys, gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk, GLib

state_file = sys.argv[1]
win = Gtk.Window(title="N02 External X11 Editor")
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
    parser.add_argument("--bin-dir", required=True, type=Path)
    parser.add_argument("--out-dir", required=True, type=Path)
    parser.add_argument("--work-dir", required=True, type=Path)
    parser.add_argument("--guest-uuid", default="")
    args = parser.parse_args()

    bin_dir = args.bin_dir.resolve()
    out_dir = args.out_dir.resolve()
    work_dir = args.work_dir.resolve()
    if out_dir.exists():
        shutil.rmtree(out_dir)
    if work_dir.exists():
        shutil.rmtree(work_dir)
    out_dir.mkdir(parents=True, mode=0o700)
    work_dir.mkdir(parents=True, mode=0o700)
    (work_dir / "discovery").mkdir(parents=True, mode=0o700)

    def env_for(profile: Path):
        env = {k: v for k, v in x11_env().items() if not k.startswith(("UNIC_", "MYGO_"))}
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
        return env

    def role_probe(profile: Path, role: str):
        res = subprocess.run(
            [str(bin_dir / "unic-desktop"), "--role", role, "--command", "probe"],
            env=env_for(profile),
            capture_output=True,
            text=True,
            timeout=10,
        )
        return json.loads(res.stdout) if res.returncode == 0 else None

    wm_name = ""
    try:
        check = subprocess.check_output(["xprop", "-root", "_NET_SUPPORTING_WM_CHECK"], env=x11_env(), text=True, timeout=5)
        wm_id = check.strip().split()[-1]
        wm_name = subprocess.check_output(["xprop", "-id", wm_id, "_NET_WM_NAME"], env=x11_env(), text=True, timeout=5).strip()
    except Exception:
        pass

    timeline = []
    procs = []
    manifest = {
        "case": "N02-linux-x11-standalone-quick-panel",
        "status": "FAIL",
        "guest_uuid": args.guest_uuid,
        "started_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "environment": {
            "os": subprocess.check_output(["uname", "-a"], text=True).strip(),
            "display": os.environ.get("DISPLAY", ""),
            "wayland_display": os.environ.get("WAYLAND_DISPLAY", ""),
            "xdg_session_type": os.environ.get("XDG_SESSION_TYPE", ""),
            "window_manager": wm_name,
            "virtualization": subprocess.check_output(["systemd-detect-virt", "--vm"], text=True).strip(),
        },
        "binary_sha256": {p.name: sha256_file(p) for p in sorted(bin_dir.iterdir()) if p.is_file()},
        "checks": [],
        "process_snapshots": {},
        "clipboard_readbacks": {},
        "paste_receipt": {},
        "timeline": timeline,
    }

    def log_event(step, detail=None):
        entry = {"at": round(time.time(), 3), "step": step}
        if detail is not None:
            entry["detail"] = detail
        timeline.append(entry)
        print(f"[N02] {step}: {json.dumps(detail, ensure_ascii=False) if detail is not None else ''}", flush=True)

    def spawn(cmd, env, log_name):
        f = open(out_dir / log_name, "wb")
        p = subprocess.Popen(cmd, env=env, stdout=f, stderr=subprocess.STDOUT)
        procs.append((p, f, log_name))
        return p

    def init_profile(name: str, sync_enabled: bool):
        p = work_dir / name
        p.mkdir(mode=0o700)
        (p / "desktop-setup.json").write_text('{"complete":true}\n')
        (p / "config.toml").write_text(
            f"[telemetry]\nenabled = false\n"
            f"[sync]\nenabled = {'true' if sync_enabled else 'false'}\n"
            f"[desktop]\nmode = 'normal'\nquick_panel_shortcut = ''\n"
        )
        res = subprocess.run(
            [str(bin_dir / "unic"), "migrate", "--initialize-empty", "--json"],
            env=env_for(p),
            capture_output=True,
            text=True,
            check=True,
            timeout=20,
        )
        log_event(f"init_profile:{name}", json.loads(res.stdout))
        return p

    xtest = XTestSender()
    try:
        assert manifest["environment"]["display"], "DISPLAY required"
        assert not manifest["environment"]["wayland_display"], "WAYLAND_DISPLAY must be unset for native X11"
        spice = subprocess.run(["pgrep", "-u", str(os.getuid()), "-x", "spice-vdagent"], capture_output=True)
        assert spice.returncode == 1, "spice-vdagent must not run under test user"
        manifest["checks"].append("native-x11-environment-verified")

        main_prof = init_profile("main", sync_enabled=True)
        peer_prof = init_profile("peer", sync_enabled=True)
        main_daemon = spawn([str(bin_dir / "unicd")], env_for(main_prof), "main-unicd.log")
        peer_daemon = spawn([str(bin_dir / "unicd")], env_for(peer_prof), "peer-unicd.log")
        main_svc = wait_until("main unicd ready", lambda: api_req(main_prof / "unic.sock", "GET", "/v1/service"))
        wait_until("peer unicd ready", lambda: api_req(peer_prof / "unic.sock", "GET", "/v1/service"))

        ticket = api_req(peer_prof / "unic.sock", "POST", "/v1/pairing-ticket")["ticket"]
        api_req(main_prof / "unic.sock", "POST", "/v1/peers", {"addr": ticket})

        def approve_all(sock):
            for req in api_req(sock, "GET", "/v1/pending") or []:
                api_req(
                    sock,
                    "POST",
                    f"/v1/pending/{req['id']}/approve",
                    {"fingerprint": req["fingerprint"], "revision": req["revision"], "bindings": req.get("bindings", [])},
                )
            return True

        wait_until(
            "peers connected",
            lambda: approve_all(peer_prof / "unic.sock")
            and approve_all(main_prof / "unic.sock")
            and any(p.get("connected") for p in (api_req(main_prof / "unic.sock", "GET", "/v1/status").get("peers") or [])),
            timeout=30,
        )

        managed_body = b"N02 native X11 managed cached file payload 2026\nutf8-\xe4\xb8\xad\xe6\x96\x87\n"
        managed_sha = sha256_bytes(managed_body)
        src_file = work_dir / "n02-x11-report.txt"
        src_file.write_bytes(managed_body)
        api_req(
            peer_prof / "unic.sock",
            "POST",
            "/v1/submissions",
            {"request_id": "n02-peer-file", "snapshot": {"files": [{"name": "n02-x11-report.txt", "path": str(src_file), "size": len(managed_body)}]}},
        )

        def find_managed():
            for it in (api_req(main_prof / "unic.sock", "GET", "/v1/history").get("items") or []):
                if it.get("origin") == "remote_device" and it.get("kind") in ("file", "files"):
                    return it
            return None

        managed_item = wait_until("managed file received", find_managed, timeout=25)
        peer_daemon.terminate()
        peer_daemon.wait(timeout=10)
        main_daemon.terminate()
        main_daemon.wait(timeout=10)
        main_daemon = spawn([str(bin_dir / "unicd")], env_for(main_prof), "main-unicd-2.log")
        main_svc = wait_until("main unicd restarted", lambda: api_req(main_prof / "unic.sock", "GET", "/v1/service"))

        png_bytes = make_png(64, 64)
        png_sha = sha256_bytes(png_bytes)
        api_req(
            main_prof / "unic.sock",
            "POST",
            "/v1/submissions",
            {"request_id": "n02-png", "snapshot": {"parts": [{"format": "image/png", "data": base64.b64encode(png_bytes).decode("ascii")}]}},
        )
        time.sleep(0.15)
        text_payload = "N02 X11 独立快捷面板自动粘贴验证 🚀"
        text_sha = sha256_bytes(text_payload.encode("utf-8"))
        api_req(
            main_prof / "unic.sock",
            "POST",
            "/v1/submissions",
            {"request_id": "n02-text", "snapshot": {"parts": [{"format": "text/plain", "data": base64.b64encode(text_payload.encode("utf-8")).decode("ascii")}]}},
        )
        log_event("history_seeded", {"text_sha256": text_sha, "png_sha256": png_sha, "file_sha256": managed_sha})

        # Launch real external X11 PyGTK3 editor and verify _NET_ACTIVE_WINDOW
        editor_state = work_dir / "editor-state.json"
        editor_proc = spawn(["python3", "-c", X11_EDITOR_SCRIPT, str(editor_state)], x11_env(), "x11-editor.log")
        editor_wid = wait_until(
            "external X11 editor active",
            lambda: (lambda w, p: w if p == editor_proc.pid else None)(*x11_active_window()),
            timeout=15,
        )
        log_event("external_editor_focused", {"editor_pid": editor_proc.pid, "window_id": editor_wid})

        # Invoke untouched production unic-desktop --quick-panel
        r = subprocess.run([str(bin_dir / "unic-desktop"), "--quick-panel"], env=env_for(main_prof), capture_output=True, text=True, timeout=20)
        assert r.returncode == 0, f"prod --quick-panel failed: {r.stdout} {r.stderr}"
        panel_role = wait_until("quick-panel role probe", lambda: role_probe(main_prof, "quick-panel"), timeout=15)
        assert role_probe(main_prof, "tray") is None and role_probe(main_prof, "gui") is None
        ps_standalone = unic_processes(main_prof)
        manifest["process_snapshots"]["x11_standalone_panel"] = ps_standalone
        assert len(ps_standalone) == 2, f"expected 2 processes (unicd + quick-panel), got {ps_standalone}"

        panel_wid = wait_until(
            "Quick Panel active on X11",
            lambda: (lambda w, p: w if p == panel_role["pid"] else None)(*x11_active_window()),
            timeout=15,
        )
        manifest["checks"].append("x11-production-standalone-only-unicd-and-panel")
        log_event("panel_focused_on_x11", {"panel_pid": panel_role["pid"], "panel_wid": panel_wid})

        # Repeat --quick-panel while visible -> focus without hiding or replacing PID
        r2 = subprocess.run([str(bin_dir / "unic-desktop"), "--quick-panel"], env=env_for(main_prof), capture_output=True, text=True, timeout=15)
        assert r2.returncode == 0
        time.sleep(0.3)
        assert x11_active_window()[1] == panel_role["pid"]
        assert role_probe(main_prof, "quick-panel")["pid"] == panel_role["pid"]
        manifest["checks"].append("x11-repeat-quick-panel-focuses-without-hiding")
        log_event("repeat_quick_panel_refocused", {"pid": panel_role["pid"]})

        # Physical XTest Return -> real cross-app single-paste insertion into external X11 editor!
        time.sleep(0.45)
        xtest.tap("Return")
        wait_until(
            "external X11 editor received pasted text",
            lambda: editor_state.exists() and json.loads(editor_state.read_text()).get("text") == text_payload,
            timeout=12,
        )
        time.sleep(0.9)
        final_editor = json.loads(editor_state.read_text())
        assert final_editor["text"] == text_payload, f"editor text mismatch/duplicate: {final_editor!r}"
        read_text = x11_clip_read("text")
        assert sha256_bytes(read_text) == text_sha
        manifest["paste_receipt"] = {
            "editor_pid": editor_proc.pid,
            "editor_window_id": editor_wid,
            "inserted_text": final_editor["text"],
            "expected_sha256": text_sha,
            "actual_sha256": sha256_bytes(final_editor["text"].encode("utf-8")),
            "single_insertion_verified": True,
        }
        manifest["clipboard_readbacks"]["text_after_paste_dismiss"] = {
            "expected_sha256": text_sha,
            "actual_sha256": sha256_bytes(read_text),
            "match": True,
        }
        manifest["checks"].append("x11-enter-cross-app-auto-paste-single-insertion")
        log_event("x11_cross_app_paste_verified", manifest["paste_receipt"])

        # Reopen -> Down -> Ctrl+C (PNG) -> verify X11 image/png after dismiss
        subprocess.run([str(bin_dir / "unic-desktop"), "--quick-panel"], env=env_for(main_prof), check=True, timeout=15)
        wait_until("panel active for png", lambda: x11_active_window()[1] == panel_role["pid"], timeout=10)
        time.sleep(0.35)
        xtest.tap("Down")
        time.sleep(0.2)
        xtest.tap("c", ctrl=True)
        wait_until("panel dismissed after png copy", lambda: x11_active_window()[1] != panel_role["pid"], timeout=10)
        read_png = x11_clip_read("image/png")
        assert sha256_bytes(read_png) == png_sha
        manifest["clipboard_readbacks"]["png_after_dismiss"] = {
            "expected_sha256": png_sha,
            "actual_sha256": sha256_bytes(read_png),
            "bytes": len(read_png),
            "match": True,
        }
        manifest["checks"].append("x11-windowless-png-copy-and-readback")
        log_event("x11_png_copied_and_verified", manifest["clipboard_readbacks"]["png_after_dismiss"])

        # Reopen -> Down x2 -> Ctrl+C (managed file) -> delete history -> external xsel replacement reclaims!
        subprocess.run([str(bin_dir / "unic-desktop"), "--quick-panel"], env=env_for(main_prof), check=True, timeout=15)
        wait_until("panel active for file", lambda: x11_active_window()[1] == panel_role["pid"], timeout=10)
        time.sleep(0.35)
        xtest.tap("Down")
        time.sleep(0.15)
        xtest.tap("Down")
        time.sleep(0.2)
        xtest.tap("c", ctrl=True)
        wait_until("panel dismissed after file copy", lambda: x11_active_window()[1] != panel_role["pid"], timeout=10)
        uri_raw = x11_clip_read("text/uri-list").decode("utf-8", "replace").strip()
        uris = [line.strip() for line in uri_raw.splitlines() if line.strip() and not line.startswith("#")]
        assert len(uris) == 1 and uris[0].startswith("file://"), f"unexpected X11 uri-list: {uri_raw!r}"
        managed_path = Path(urllib.parse.unquote(urllib.parse.urlparse(uris[0]).path))
        assert managed_path.exists() and str(managed_path).startswith(str(main_prof / "inbox"))
        assert sha256_file(managed_path) == managed_sha
        manifest["clipboard_readbacks"]["managed_file_after_dismiss"] = {
            "uri": uris[0],
            "path": str(managed_path),
            "expected_sha256": managed_sha,
            "actual_sha256": sha256_file(managed_path),
            "match": True,
        }
        manifest["checks"].append("x11-windowless-managed-file-copy-and-readback")
        log_event("x11_managed_file_copied_and_verified", manifest["clipboard_readbacks"]["managed_file_after_dismiss"])

        api_req(main_prof / "unic.sock", "DELETE", f"/v1/history/{managed_item['id']}?revision={managed_item['revision']}")
        time.sleep(0.5)
        assert managed_path.exists(), "managed file prematurely deleted while held on X11 clipboard"
        subprocess.run(["xsel", "-ib"], input=b"N02-X11-EXTERNAL-REPLACEMENT", env=x11_env(), check=True, timeout=5)
        wait_until("managed file reclaimed on X11", lambda: not managed_path.exists(), timeout=12)
        cache_after = api_req(main_prof / "unic.sock", "GET", "/v1/file-cache")
        manifest["clipboard_readbacks"]["external_replacement_and_reclaim"] = {
            "external_clip": x11_clip_read("text").decode("utf-8", "replace"),
            "managed_file_removed": not managed_path.exists(),
            "cache_state_after_release": cache_after,
        }
        manifest["checks"].append("x11-external-replacement-releases-file-lease-and-reclaims-cache")
        log_event("x11_file_lease_reclaimed", manifest["clipboard_readbacks"]["external_replacement_and_reclaim"])

        # Tray coexistence (zero echo) + Tray SIGTERM exit preserves standalone unicd & quick-panel
        tray_proc = spawn([str(bin_dir / "unic-desktop"), "--role", "tray"], env_for(main_prof), "main-tray.log")
        wait_until("tray ready", lambda: role_probe(main_prof, "tray"), timeout=15)
        manifest["process_snapshots"]["x11_with_tray_coexistence"] = unic_processes(main_prof)
        hist_before = len(api_req(main_prof / "unic.sock", "GET", "/v1/history").get("items") or [])

        subprocess.run([str(bin_dir / "unic-desktop"), "--quick-panel"], env=env_for(main_prof), check=True, timeout=15)
        wait_until("panel active with tray", lambda: x11_active_window()[1] == panel_role["pid"], timeout=10)
        time.sleep(0.35)
        xtest.tap("c", ctrl=True)
        wait_until("panel dismissed with tray", lambda: x11_active_window()[1] != panel_role["pid"], timeout=10)
        assert sha256_bytes(x11_clip_read("text")) == text_sha
        time.sleep(1.5)
        hist_after = len(api_req(main_prof / "unic.sock", "GET", "/v1/history").get("items") or [])
        assert hist_after == hist_before, f"X11 tray coexistence caused echo: {hist_before} -> {hist_after}"
        manifest["checks"].append("x11-tray-coexistence-zero-echo")
        log_event("x11_tray_coexistence_zero_echo", {"history_count": hist_after})

        tray_proc.terminate()
        tray_proc.wait(timeout=10)
        time.sleep(0.5)
        assert role_probe(main_prof, "tray") is None
        svc_after = api_req(main_prof / "unic.sock", "GET", "/v1/service")
        panel_after = role_probe(main_prof, "quick-panel")
        assert svc_after["id"] == main_svc["id"] and svc_after["pid"] == main_svc["pid"]
        assert panel_after is not None and panel_after["pid"] == panel_role["pid"]
        manifest["process_snapshots"]["x11_after_tray_exit"] = unic_processes(main_prof)

        subprocess.run([str(bin_dir / "unic-desktop"), "--quick-panel"], env=env_for(main_prof), check=True, timeout=15)
        wait_until("panel active after tray exit", lambda: x11_active_window()[1] == panel_role["pid"], timeout=10)
        time.sleep(0.35)
        xtest.tap("Down")
        time.sleep(0.2)
        xtest.tap("c", ctrl=True)
        wait_until("panel dismissed after post-tray png copy", lambda: x11_active_window()[1] != panel_role["pid"], timeout=10)
        assert sha256_bytes(x11_clip_read("image/png")) == png_sha
        manifest["checks"].append("x11-tray-exit-preserves-standalone-unicd-and-panel-copy")
        log_event("x11_post_tray_exit_copy_verified", {"png_sha256": png_sha})

        manifest["status"] = "PASS"
        manifest["finished_at_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    except Exception as exc:
        manifest["error"] = f"{type(exc).__name__}: {exc}"
        log_event("FAILED", manifest["error"])
        raise
    finally:
        xtest.close()
        for r in ps_snapshot():
            try:
                raw_env = Path(f"/proc/{r['pid']}/environ").read_bytes().decode("utf-8", "replace")
                if str(work_dir) in raw_env and r["pid"] != os.getpid():
                    os.kill(r["pid"], signal.SIGTERM)
            except OSError:
                pass
        time.sleep(0.4)
        for p, f, _ in procs:
            try:
                if p.poll() is None:
                    p.terminate()
                    p.wait(timeout=5)
            except Exception:
                try:
                    p.kill()
                except Exception:
                    pass
            try:
                f.close()
            except Exception:
                pass
        (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
        sums = [f"{sha256_file(p)}  {p.name}" for p in sorted(out_dir.iterdir()) if p.is_file() and p.name != "SHA256SUMS"]
        (out_dir / "SHA256SUMS").write_text("\n".join(sums) + "\n")


if __name__ == "__main__":
    main()
