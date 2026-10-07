#!/usr/bin/env python3
"""Clean-machine acceptance of the shipped macOS DMG (issue #1895).

Installs UniClipboard.app from the DMG into /Applications the way a user does (mount, copy), runs the
shipping app with no profile and no test control plane, and checks what only a real install can show:

  - Gatekeeper accepts the DMG and the installed app (`spctl`), when the build is Developer ID signed
  - the app starts from a PATH without `uniclipd`, and the daemon that runs is the bundled one
  - the quick panel helper runs from the bundle
  - the data root is the real `~/Library/Application Support/app.uniclipboard.desktop` (no profile suffix)
  - the daemon answers /health
  - clipboard capture works: with `--cli`, a marker put on the real pasteboard shows up in `uniclip search`
  - a normal quit stops the daemon and the helper
  - the host really is the requested architecture (native Intel is not Rosetta)

The shipping app has no profile, so it uses the real data root and the real login keychain of the machine.
This script therefore refuses to run anywhere but a disposable GitHub-hosted runner.

  smoke_release.py --dmg UniClipboard_1.1.1_x64.dmg --arch x86_64 --signed true --out evidence [--cli uniclip]
"""
import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

APP = Path("/Applications/UniClipboard.app")
MACOS = APP / "Contents/MacOS"
DATA = Path.home() / "Library/Application Support/app.uniclipboard.desktop"
MINIMAL_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"


def sh(*cmd, check=True, env=None):
    p = subprocess.run([str(c) for c in cmd], capture_output=True, text=True, env=env)
    if check and p.returncode != 0:
        raise AssertionError(f"{' '.join(map(str, cmd))} failed ({p.returncode}): {(p.stdout + p.stderr).strip()[-600:]}")
    return p


def image_path(pid):
    out = sh("ps", "-p", pid, "-o", "comm=", check=False).stdout.strip()
    return os.path.realpath(out) if out else ""


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dmg", required=True, type=Path)
    ap.add_argument("--arch", required=True, choices=["arm64", "x86_64"])
    ap.add_argument("--signed", required=True, choices=["true", "false"])
    ap.add_argument("--cli", type=Path)
    ap.add_argument("--out", required=True, type=Path)
    args = ap.parse_args()
    if not args.dmg.is_file():
        sys.exit(f"--dmg {args.dmg!r} is not a file")
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        sys.exit("refusing to run: the shipping app uses the real profile and keychain; use a disposable GitHub-hosted runner")
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    r = {"host": {"machine": platform.machine(), "macOS": platform.mac_ver()[0],
                  "translated": sh("sysctl", "-n", "sysctl.proc_translated", check=False).stdout.strip() or "n/a",
                  "model": sh("sysctl", "-n", "hw.model", check=False).stdout.strip()},
         "dmg": args.dmg.name, "passed": False, "checks": {}}
    checks = r["checks"]
    gui_pid = daemon_pid = helper_pid = None
    opener = None
    try:
        assert platform.machine() == args.arch, f"host is {platform.machine()}, the bundle under test is {args.arch}"
        assert not APP.exists(), "/Applications/UniClipboard.app already exists on this machine"
        assert not DATA.exists(), f"{DATA} already exists: not a clean machine"

        mount = Path("/tmp/uc-smoke-dmg")
        mount.mkdir(exist_ok=True)
        # A DMG downloaded in a browser carries the quarantine attribute; the assessment must still pass.
        sh("xattr", "-w", "com.apple.quarantine", "0081;00000000;Safari;", args.dmg)
        if args.signed == "true":
            p = sh("spctl", "-a", "-vv", "-t", "open", "--context", "context:primary-signature", args.dmg, check=False)
            checks["gatekeeperAcceptsDmg"] = p.returncode == 0 and "accepted" in (p.stdout + p.stderr)
            checks["gatekeeperDmgDetail"] = (p.stdout + p.stderr).strip()
            assert checks["gatekeeperAcceptsDmg"], checks["gatekeeperDmgDetail"]
        sh("hdiutil", "attach", "-nobrowse", "-readonly", "-mountpoint", mount, args.dmg)
        try:
            sh("ditto", mount / "UniClipboard.app", APP)  # drag-and-drop install
            checks["dmgHasApplicationsLink"] = (mount / "Applications").is_symlink()
        finally:
            sh("hdiutil", "detach", mount, check=False)
        assert checks["dmgHasApplicationsLink"], "the DMG has no /Applications link"

        if args.signed == "true":
            p = sh("spctl", "-a", "-vv", "-t", "exec", APP, check=False)
            detail = (p.stdout + p.stderr).strip()
            checks["gatekeeperAcceptsApp"] = p.returncode == 0 and "Notarized Developer ID" in detail
            checks["gatekeeperAppDetail"] = detail
            assert checks["gatekeeperAcceptsApp"], detail
            p = sh("xcrun", "stapler", "validate", APP, check=False)
            checks["staplerValidate"] = p.returncode == 0
            assert checks["staplerValidate"], p.stdout + p.stderr

        env = {"HOME": str(Path.home()), "PATH": MINIMAL_PATH, "USER": os.environ.get("USER", "")}
        assert shutil.which("uniclipd", path=MINIMAL_PATH) is None
        # Direct execution with the output captured: on a runner `open` can stay blocked and leaves no log, which
        # made a failed start undiagnosable. Gatekeeper has already assessed the installed bundle above.
        applog = (out / "app.log").open("w")
        opener = subprocess.Popen([str(MACOS / "gui-go")], env=env, stdin=subprocess.DEVNULL, stdout=applog, stderr=subprocess.STDOUT,
                                  start_new_session=True)
        deadline = time.monotonic() + 120
        conn = None
        while time.monotonic() < deadline:
            if (DATA / "daemon.conn").is_file():
                try:
                    conn = json.loads((DATA / "daemon.conn").read_text())
                    break
                except ValueError:
                    pass
            time.sleep(1)
        assert conn, "no daemon.conn in the real data root within 120 s"
        checks["dataRootIsRealNoProfile"] = DATA.is_dir()
        daemon_pid = conn["pid"]
        checks["daemonImage"] = image_path(daemon_pid)
        assert checks["daemonImage"] == str(MACOS / "uniclipd"), f"daemon is {checks['daemonImage']}"
        with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(f"http://{conn['host']}:{conn['port']}/health", timeout=10) as resp:
            checks["health"] = json.load(resp)["data"]["status"]
        assert checks["health"] == "ok"

        checks["guiExitedEarly"] = opener.poll()
        gui = sh("pgrep", "-f", str(MACOS / "gui-go"), check=False).stdout.split()
        assert gui, "the GUI process is not running"
        gui_pid = int(gui[0])
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline and helper_pid is None:
            kids = [int(p) for p in sh("pgrep", "-P", gui_pid, check=False).stdout.split()]
            helper = [p for p in kids if image_path(p) == str(MACOS / "uniclip-quick-panel")]
            helper_pid = helper[0] if helper else None
            time.sleep(1)
        checks["helperRunning"] = helper_pid is not None
        assert helper_pid, "the quick panel helper did not start from the bundle"

        if args.cli:
            cli = [str(args.cli.resolve())]
            trace = r.setdefault("cliTrace", [])

            def run_cli(*a):
                p = sh(*cli, *a, check=False, env=env)
                trace.append({"args": [x for x in a if x != "smoke-passphrase-1895"], "rc": p.returncode, "out": (p.stdout + p.stderr)[-500:]})
                return p
            init = run_cli("space", "init", "--passphrase", "smoke-passphrase-1895", "--device-name", "smoke")
            assert init.returncode == 0, "uniclip space init failed (see cliTrace)"
            # The pasteboard watcher can miss the first change right after start-up, so the marker is put on the
            # pasteboard again (a new token each time, because search is exact-token) until one shows up.
            found = False
            marker = ""
            for attempt in range(8):
                marker = f"smokecapture{int(time.time())}x{attempt}"
                subprocess.run(["pbcopy"], input=marker, text=True, check=True)
                changes = sh("osascript", "-e", "the clipboard as text", check=False).stdout.strip()
                for _ in range(4):
                    time.sleep(1)
                    p = run_cli("search", marker)
                    if marker in (p.stdout + p.stderr):
                        found = True
                        break
                trace.append({"args": ["attempt", attempt], "rc": 0, "out": f"pasteboard now reads: {changes!r}; found={found}"})
                if found:
                    break
            checks["clipboardMarker"] = marker
            del trace[6:-8]
            checks["clipboardCaptureFindsMarker"] = found
            assert found, "the marker put on the pasteboard never appeared in history"
        else:
            checks["clipboardCaptureFindsMarker"] = "not run (no --cli)"

        sh("osascript", "-e", 'tell application id "app.uniclipboard.desktop" to quit', check=False)
        deadline = time.monotonic() + 40
        while (alive(gui_pid) or alive(daemon_pid) or alive(helper_pid)) and time.monotonic() < deadline:
            time.sleep(0.5)
        checks["quitStoppedAll"] = not (alive(gui_pid) or alive(daemon_pid) or alive(helper_pid))
        assert checks["quitStoppedAll"], "a normal quit left the GUI, daemon or helper running"
        r["passed"] = True
    except AssertionError as e:
        r["error"] = str(e)
        diag = out / "diagnostics.txt"
        with diag.open("w") as f:
            for cmd in (["ps", "-axo", "pid,ppid,comm"], ["launchctl", "managername"], ["id"],
                        ["log", "show", "--last", "5m", "--style", "compact", "--predicate", 'process == "gui-go" OR process == "uniclipd" OR process == "syspolicyd" OR process == "amfid"']):
                f.write("$ " + " ".join(cmd) + "\n")
                f.write(subprocess.run(cmd, capture_output=True, text=True).stdout[-20000:] + "\n")
        reports = Path.home() / "Library/Logs/DiagnosticReports"
        if reports.is_dir():
            shutil.copytree(reports, out / "DiagnosticReports", dirs_exist_ok=True)
    finally:
        if opener is not None:
            opener.kill()
        for pid in (helper_pid, daemon_pid, gui_pid):
            if pid and alive(pid) and image_path(pid).startswith(str(MACOS)):
                os.kill(pid, 15)
        logs = Path.home() / "Library/Logs/app.uniclipboard.desktop"
        if logs.is_dir():
            shutil.copytree(logs, out / "logs", dirs_exist_ok=True)
        (out / "smoke.json").write_text(json.dumps(r, indent=2))
    print(json.dumps(r, indent=2))
    sys.exit(0 if r["passed"] else 1)


if __name__ == "__main__":
    main()
