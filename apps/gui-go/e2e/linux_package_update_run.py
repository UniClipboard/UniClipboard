#!/usr/bin/env python3
"""Installed package -> native WebView -> local feed -> package update hint."""

import argparse, json, os, pathlib, subprocess, sys, tempfile, time, shutil

sys.path.insert(0, "/work/apps/gui-go/e2e")
from linux_appimage_run import Run, start_xvfb, wait_daemon, copy_logs

p = argparse.ArgumentParser()
p.add_argument("--out", type=pathlib.Path, required=True)
p.add_argument("--kind", choices=["deb", "rpm"], required=True)
p.add_argument("--pubkey", type=pathlib.Path, required=True)
a = p.parse_args()
out = a.out
out.mkdir(parents=True, exist_ok=True)
home = pathlib.Path(tempfile.mkdtemp(prefix="uc-installed-package-"))
(home / ".cache").mkdir()
(home / "run").mkdir(mode=0o700)
env = dict(
    os.environ,
    HOME=str(home),
    XDG_CONFIG_HOME=str(home / ".config"),
    XDG_DATA_HOME=str(home / ".local/share"),
    XDG_CACHE_HOME=str(home / ".cache"),
    XDG_RUNTIME_DIR=str(home / "run"),
    DISPLAY=":99",
    GDK_BACKEND="x11",
    XDG_SESSION_TYPE="x11",
    UC_DISABLE_SYSTEM_CLIPBOARD="1",
    UC_GUI_GO_EXIT_MODE="full",
    UC_GUI_GO_E2E_PHASE="linux-package-update",
    UC_GUI_GO_E2E_VISIBLE="1",
    UC_GUI_GO_E2E_SECRET="appimage-e2e-passphrase",
    UC_UPDATE_ENDPOINT="http://127.0.0.1:18080/feed.json",
    UC_UPDATE_PUBKEY=a.pubkey.read_text().strip(),
)
for key in [
    "APPIMAGE",
    "APPDIR",
    "UC_PORTABLE",
    "UC_PROFILE",
    "UC_GUI_GO_ISOLATED",
    "UNICLIPBOARD_ENV",
]:
    env.pop(key, None)
feed = out / "feed"
feed.mkdir()
(feed / "feed.json").write_text(
    json.dumps(
        {
            "version": "99.0.0-e2e",
            "notes": "Package hint fixture",
            "pub_date": "2026-10-08T00:00:00Z",
            "platforms": {
                "linux-aarch64": {
                    "url": "http://127.0.0.1:18080/must-not-download",
                    "signature": "not-downloaded",
                }
            },
        }
    )
)
server = subprocess.Popen(
    [
        sys.executable,
        "-m",
        "http.server",
        "18080",
        "--bind",
        "127.0.0.1",
        "--directory",
        str(feed),
    ],
    stdout=(out / "feed.log").open("w"),
    stderr=subprocess.STDOUT,
)
env["DBUS_SESSION_BUS_ADDRESS"] = os.environ["UC_E2E_BUS"]
xvfb = start_xvfb(out)
receiver = subprocess.Popen(
    [
        sys.executable,
        "/work/apps/gui-go/e2e/linux_notification_receiver.py",
        str(out / "notification"),
    ],
    env=env,
    stdout=(out / "receiver.log").open("w"),
    stderr=subprocess.STDOUT,
)
run = Run(out, pathlib.Path("/usr/bin/uniclipboard"), env)
gui = None
result = {"passed": False, "kind": a.kind}
try:
    for _ in range(100):
        if (out / "notification/ready").exists():
            break
        if receiver.poll() is not None:
            raise RuntimeError("notification receiver exited")
        time.sleep(0.1)
    probe = subprocess.run(
        ["secret-tool", "store", "--label=uc-probe", "uc", "probe"],
        input="probe",
        text=True,
        env=env,
        capture_output=True,
        timeout=15,
    )
    if probe.returncode:
        raise RuntimeError("keyring not ready: " + probe.stderr)
    subprocess.run(["secret-tool", "clear", "uc", "probe"], env=env, check=True)
    gui = run.launch("installed")
    conn, daemon = wait_daemon(home, 90)
    if conn is None:
        raise RuntimeError("installed daemon did not become ready")
    kind = gui.step("package-install-kind", 120)
    hint = gui.step("package-update-hint", 120)
    # Let the real modal entrance animation finish before capturing the disposable display.
    time.sleep(1)
    # Screenshot only the disposable Xvfb display.
    os.environ["DISPLAY"] = env["DISPLAY"]
    os.environ["GDK_BACKEND"] = "x11"
    import gi

    gi.require_version("Gdk", "3.0")
    from gi.repository import Gdk

    Gdk.init([])
    root = Gdk.get_default_root_window()
    Gdk.pixbuf_get_from_window(root, 0, 0, root.get_width(), root.get_height()).savev(
        "/tmp/uc-package-screen.png", "png", [], []
    )
    shutil.copy2("/tmp/uc-package-screen.png", out / "screen.png")
    gui.proc.wait(timeout=45)
    rows = (
        [
            json.loads(l)
            for l in (out / "notification/received.jsonl").read_text().splitlines()
        ]
        if (out / "notification/received.jsonl").exists()
        else []
    )
    result.update(
        {
            "detected": kind,
            "hint": hint,
            "notificationReceived": rows,
            "passed": kind["ok"]
            and kind["detail"].get("data") == a.kind
            and hint["ok"]
            and rows
            == [
                {
                    "summary": "Linux acceptance",
                    "body": "Isolated package notification fixture",
                }
            ],
        }
    )
except Exception as e:
    result["error"] = repr(e)
finally:
    if gui and gui.proc.poll() is None:
        gui.proc.terminate()
        gui.proc.wait(timeout=15)
    cleanup_errors = []
    for conn in home.rglob("daemon.conn"):
        try:
            pid = json.loads(conn.read_text())["pid"]
            if os.readlink(f"/proc/{pid}/exe") == "/usr/bin/uniclipd":
                os.kill(pid, 15)
        except (FileNotFoundError, ProcessLookupError):
            # Full GUI exit may remove the connection or stop the recorded daemon first.
            continue
        except (OSError, ValueError, KeyError) as exc:
            cleanup_errors.append(f"{conn.name}: {type(exc).__name__}")
    for proc in [receiver, xvfb, server]:
        if proc.poll() is None:
            proc.terminate()
        proc.wait(timeout=10)
    result["cleanupErrors"] = cleanup_errors
    if cleanup_errors:
        result["passed"] = False
    errors = []
    for d in (home / ".local/state").glob("app.uniclipboard.desktop*"):
        errors += copy_logs(d, out / "logs" / d.name)
    result["evidenceCopyErrors"] = errors
    if errors:
        result["passed"] = False
    (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
sys.exit(0 if result["passed"] else 1)
