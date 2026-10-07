#!/usr/bin/env python3
"""Run a packaged macOS bundle (acceptance variant) the way an installed app runs, without a dev PATH.

The bundle comes from `packaging/macos/package.py bundle --variant acceptance`: the real release-profile
`uniclipd` and `uniclip-quick-panel`, the same layout, icon and signing flags as the shipping bundle, but
with the E2E control plane and a distinct identity (`<identifier>.e2e`). It runs in a throwaway HOME with a
`gui-go-*` profile, the file keystore and a no-op system clipboard, so it never touches the real
pasteboard, keychain, profile or history. The shipping (`release`-tagged) binary has no control plane and
no profile; it is exercised by the CI smoke job instead (see README, "macOS 发布构建").

What a pass proves: the bundle starts from a PATH that cannot contain `uniclipd`; the daemon and the quick
panel helper that run are the executables inside the bundle (checked by process image path); the daemon
answers /health; the shared frontend reaches its first screen over the real WebView -> HTTP/WS path; a full
quit stops the daemon and the helper. It does not prove anything about Developer ID, notarization or
Gatekeeper; `packaging/macos/verify_bundle.py` reports those separately.

  macos_bundle_run.py --app <UniClipboard E2E Test Build.app> --arch arm64 --out <dir> [--signature adhoc]
"""
import argparse
import json
import os
import platform
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from run import ROOT, isolated_env, pid_alive, wait_for_step  # noqa: E402

MINIMAL_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"


def image_path(pid):
    out = subprocess.run(["ps", "-p", str(pid), "-o", "comm="], capture_output=True, text=True).stdout.strip()
    return os.path.realpath(out) if out else ""


def children(pid):
    out = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split()
    return [int(p) for p in out]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", required=True, type=Path)
    ap.add_argument("--arch", required=True, choices=["arm64", "x86_64"])
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--signature", default="adhoc", choices=["none", "adhoc", "developer-id"])
    args = ap.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    app = args.app.resolve()
    macos = app / "Contents/MacOS"

    results = {"app": str(app), "arch": args.arch, "hostMachine": platform.machine(), "passed": False}
    # Static contract first: a bundle that fails it is not worth launching.
    verify = subprocess.run([sys.executable, str(ROOT / "apps/gui-go/packaging/macos/verify_bundle.py"), "--app", str(app),
                             "--arch", args.arch, "--signature", args.signature, "--hardened", "--identity-suffix", ".e2e",
                             "--out", str(out / "contract.json")], text=True)
    results["staticContract"] = verify.returncode == 0
    if verify.returncode != 0:
        (out / "assertions.json").write_text(json.dumps(results, indent=2))
        sys.exit("static bundle contract failed")

    home = tempfile.mkdtemp(prefix="uc-gui-go-")
    profile = "gui-go-" + os.path.basename(home)
    evidence = out / "native.jsonl"
    evidence.write_text("")
    control = out / "control.txt"
    control.write_text("")
    env = isolated_env(home, profile, {"PATH": MINIMAL_PATH, "UC_GUI_GO_ISOLATED": "1", "UC_GUI_GO_EVIDENCE": str(evidence),
                                       "UC_GUI_GO_EXIT_MODE": "full", "UC_GUI_GO_E2E_CONTROL_FILE": str(control)})
    assert not any((Path(d) / "uniclipd").exists() for d in MINIMAL_PATH.split(":")), "uniclipd is on the minimal PATH"
    conn_path = Path(home) / "Library/Application Support" / ("app.uniclipboard.desktop-" + profile) / "daemon.conn"
    proc, daemon_pid, helper_pids = None, None, []
    try:
        with (out / "gui.log").open("w") as log:
            proc = subprocess.Popen([str(macos / "gui-go")], env=env, stdout=log, stderr=log)
            # The default driver also walks the WebView quick panel scenario, which does not apply while the
            # native helper owns the panel; this run only needs the launch up to the first screen, then quits
            # through the control file the way the other scenarios do.
            rows = wait_for_step(proc, evidence, 0, "first-screen", 240)
            steps = {r["step"]: r for r in rows}
            assert all(r["ok"] for r in rows), f"failed steps: {[r for r in rows if not r['ok']]}"
            for needed in ("bootstrapped", "shared-app-mounted", "ws-status-snapshot", "first-screen"):
                assert needed in steps, f"missing step {needed}"
            conn = json.loads(conn_path.read_text())
            daemon_pid = conn["pid"]
            gui_image = image_path(proc.pid)
            daemon_image = image_path(daemon_pid)
            assert gui_image == str(macos / "gui-go"), gui_image
            assert daemon_image == str(macos / "uniclipd"), f"daemon is {daemon_image}, not the bundled uniclipd"
            with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(f"http://{conn['host']}:{conn['port']}/health", timeout=5) as r:
                assert json.load(r)["data"]["status"] == "ok"
            helper_pids = [p for p in children(proc.pid) if image_path(p) == str(macos / "uniclip-quick-panel")]
            assert len(helper_pids) == 1, f"expected the bundled quick panel helper as a child of the GUI, got {helper_pids}"
            results.update({"guiPid": proc.pid, "daemonPid": daemon_pid, "helperPid": helper_pids[0],
                            "daemonImage": daemon_image, "helperImage": image_path(helper_pids[0]),
                            "firstScreen": steps["first-screen"]["detail"]})
            with control.open("a") as f:
                f.write("exit\n")
            code = proc.wait(timeout=60)
            assert code == 0, f"GUI exit code {code}"
            deadline = time.monotonic() + 15
            while (pid_alive(daemon_pid) or pid_alive(helper_pids[0])) and time.monotonic() < deadline:
                time.sleep(0.2)
            assert not pid_alive(daemon_pid), "full quit left the daemon running"
            assert not pid_alive(helper_pids[0]), "full quit left the quick panel helper running"
            results["fullQuitStoppedDaemonAndHelper"] = True
            results["passed"] = True
    finally:
        if proc and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=20)
            except subprocess.TimeoutExpired:
                proc.kill()
        # Only processes this run started, identified by their image inside this bundle.
        for pid in [daemon_pid, *helper_pids]:
            if pid and pid_alive(pid) and image_path(pid).startswith(str(macos)):
                os.kill(pid, 15)
        (out / "assertions.json").write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps(results, indent=2))
    assert results["passed"], "bundle run failed"


if __name__ == "__main__":
    main()
