#!/usr/bin/env python3
"""Build a frozen private bundle and accept native macOS process entry/close."""

import argparse
import hashlib
import http.client
import json
import os
from pathlib import Path
import platform
import plistlib
import shutil
import signal
import socket
import subprocess
import tempfile
import time
import uuid


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def wait(check, seconds=35):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        try:
            value = check()
            if value:
                return value
        except (OSError, ValueError, AssertionError, subprocess.SubprocessError, http.client.HTTPException) as err:
            last = str(err)
        time.sleep(0.08)
    raise AssertionError(f"native process state did not converge: {last}")


def alive(pid):
    result = subprocess.run(["ps", "-p", str(pid), "-o", "stat="], text=True, capture_output=True)
    return result.returncode == 0 and not result.stdout.strip().startswith("Z")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--dock", action="store_true", help="verify Dock items using existing Accessibility permission")
    args = parser.parse_args()
    assert platform.system() == "Darwin", "this acceptance requires native macOS"
    root = args.root.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    task = Path(tempfile.mkdtemp(prefix="uc-bundle-", dir="/private/tmp"))
    receipt = {"status": "FAIL", "platform": platform.platform(), "arch": platform.machine(), "checks": [], "checkpoints": [], "native_event_loops": True, "window_visibility_evidence": "mygo automation windows[].visible; app-scoped state only", "os_native_visibility": "NOT_RUN", "foreground_focus": "NOT_RUN", "native_tray_click": "NOT_RUN", "native_hotkey": "NOT_RUN", "native_paste": "NOT_RUN", "login_registration": "NOT_RUN", "clipboard": "desktop_e2e directory board; no host clipboard", "input": "role IPC close/quit; no physical input"}
    receipt["source_head"] = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()
    receipt["dock"] = "NOT_RUN"
    all_pids = set()
    profiles = []
    executable = None
    dock_probe = task / "dock-probe"

    def check_dock(name, expected):
        if not args.dock:
            return

        def matches():
            count = int(subprocess.check_output([dock_probe, bundle], text=True, timeout=5))
            assert count == expected, f"{name}: expected {expected} Dock items, got {count}"
            return True
        wait(matches, seconds=10)
        receipt["checks"].append(f"{name}: {expected} Dock items for the private bundle")

    def private_processes():
        rows = []
        if executable is None:
            return rows
        text = subprocess.check_output(["ps", "-axo", "pid=,command="], text=True)
        for line in text.splitlines():
            fields = line.strip().split(None, 1)
            if len(fields) == 2 and fields[1].startswith(str(executable.parent) + "/"):
                pid = int(fields[0])
                all_pids.add(pid)
                rows.append({"pid": pid, "command": fields[1]})
        return rows

    def env_for(profile):
        discovery = task / "discovery"
        return dict(os.environ, UNIC_HOME=str(profile), UNIC_SOCK=str(profile / "unic.sock"), UNIC_DESKTOP_E2E_PROFILE=str(profile), UNIC_DESKTOP_E2E_PROCESS_FIXTURE="", UNIC_DESKTOP_E2E_GUEST="", MYGO_AUTOMATION="", UNIC_RENDEZVOUS_URL="http://127.0.0.1:1", HOME=str(discovery), XDG_CONFIG_HOME=str(discovery / "config"), XDG_DATA_HOME=str(discovery / "data"))

    def control(profile, role, command):
        result = subprocess.run([str(executable), "--role", role, "--command", command], env=env_for(profile), text=True, capture_output=True, timeout=20)
        if result.returncode:
            raise AssertionError(result.stderr.strip())
        return json.loads(result.stdout)

    def probe(profile, role):
        result = control(profile, role, "probe")
        all_pids.add(result["pid"])
        return result

    class Connection(http.client.HTTPConnection):
        def __init__(self, path):
            super().__init__("localhost", timeout=2)
            self.path = path

        def connect(self):
            self.sock = socket.socket(socket.AF_UNIX)
            self.sock.settimeout(2)
            self.sock.connect(self.path)

    def service(profile, stop=False):
        conn = Connection(str(profile / "unic.sock"))
        try:
            if stop:
                current = service(profile)
                conn.request("POST", "/v1/service/stop", json.dumps({"id": current["id"]}), {"Content-Type": "application/json"})
            else:
                conn.request("GET", "/v1/service")
            response = conn.getresponse()
            assert response.status == 200
            result = json.loads(response.read())
            if not stop:
                all_pids.add(result["pid"])
            return result
        finally:
            conn.close()

    def native_window(profile, panel=False):
        with socket.socket(socket.AF_UNIX) as connection:
            connection.settimeout(4)
            connection.connect(str(profile / ("automation-panel.sock" if panel else "automation.sock")))
            connection.sendall(b'{"id":1,"method":"windows"}\n')
            stream = connection.makefile("rb")
            response = json.loads(stream.readline())
            assert not response.get("error"), response
            windows = [w for w in response["result"]["windows"] if w["kind"] == "content" and w["visible"]]
            return windows

    def checkpoint(profile, name):
        roles = {}
        for role in ["tray", "gui", "quick-panel"]:
            try:
                identity = probe(profile, role)
                identity["kernel_start"] = subprocess.check_output(["ps", "-p", str(identity["pid"]), "-o", "lstart="], text=True).strip()
                roles[role] = identity
            except (AssertionError, OSError, subprocess.SubprocessError):
                pass
        core = service(profile)
        core["kernel_start"] = subprocess.check_output(["ps", "-p", str(core["pid"]), "-o", "lstart="], text=True).strip()
        receipt["checkpoints"].append({"name": name, "at": time.time(), "roles": roles, "service": core, "all_private_bundle_processes": private_processes()})
        return roles, core

    def new_profile(name, mode):
        profile = task / name
        profile.mkdir(mode=0o700)
        (profile / "e2e-owner").write_text("desktop-e2e")
        (profile / "desktop-setup.json").write_text('{"complete":true}')
        (profile / "config.toml").write_text(f"[desktop]\nmode='{mode}'\nstart_in_background=true\nlaunch_at_login=false\nquick_panel_shortcut=''\n[telemetry]\nenabled=false\n[sync]\nenabled=false\n")
        profiles.append(profile)
        return profile

    def launch(profile, background=False):
        env = env_for(profile)
        command = ["/usr/bin/open", "-n"]
        # LaunchServices does not inherit arbitrary subprocess environment.
        for key, value in env.items():
            if key.startswith(("UNIC_", "MYGO_", "XDG_")) or key == "HOME":
                command += ["--env", f"{key}={value}"]
        command += [str(bundle)]
        if background:
            command += ["--args", "--background"]
        subprocess.run(command, env=env, check=True, capture_output=True, text=True, timeout=10)

    try:
        snapshot = task / "source"
        snapshot.mkdir()
        names = subprocess.check_output(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=root).split(b"\0")
        source = {}
        for raw in sorted(set(names)):
            if not raw:
                continue
            name = os.fsdecode(raw)
            path = root / name
            if not path.is_file() or path.is_symlink():
                continue
            target = snapshot / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, target)
            target.chmod(path.stat().st_mode)
            source[name] = sha(target)
        receipt["source_tree_sha256"] = hashlib.sha256(json.dumps(source, sort_keys=True).encode()).hexdigest()
        receipt["source_files"] = source
        if args.dock:
            receipt["dock"] = "FAIL"
            subprocess.run(["swiftc", snapshot / "app/desktop/e2e/testdata/macos-bundle/dock.swift", "-o", dock_probe], check=True, timeout=60)
            subprocess.run([dock_probe], check=True, timeout=5)
        bundle = task / "UniClipboard Native Acceptance.app"
        macos = bundle / "Contents" / "MacOS"
        macos.mkdir(parents=True)
        identifier = "app.uniclipboard.test.bundle." + uuid.uuid4().hex
        (bundle / "Contents" / "Info.plist").write_bytes(plistlib.dumps({"CFBundleIdentifier": identifier, "CFBundleName": "UniClipboard Native Acceptance", "CFBundleExecutable": "unic-desktop", "CFBundlePackageType": "APPL", "CFBundleVersion": "1", "CFBundleShortVersionString": "0.0.0", "LSUIElement": True, "NSPrincipalClass": "NSApplication", "NSHighResolutionCapable": True}))
        executable = macos / "unic-desktop"
        for name, source_path in [("unic-desktop", "./app/desktop/cmd/unic-desktop"), ("unicd", "./app/service/cmd/unicd")]:
            command = ["go", "build", "-race", "-buildvcs=false", "-o", str(macos / name)]
            if name == "unic-desktop":
                command += ["-tags", "desktop_e2e"]
            command += [source_path]
            subprocess.run(command, cwd=snapshot, check=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=180)
        receipt["binary_sha256"] = {name: sha(macos / name) for name in ["unic-desktop", "unicd"]}

        background_normal = new_profile("background-normal", "normal")
        launch(background_normal, background=True)
        wait(lambda: service(background_normal))
        wait(lambda: probe(background_normal, "tray"))
        control(background_normal, "tray", "background")
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            try:
                unexpected_gui = probe(background_normal, "gui")
            except (AssertionError, OSError, subprocess.SubprocessError):
                unexpected_gui = None
            if unexpected_gui is not None:
                checkpoint(background_normal, "background-normal-unexpected-gui")
                raise AssertionError("explicit normal --background unexpectedly created a GUI role")
            time.sleep(0.08)
        roles, _ = checkpoint(background_normal, "background-normal-tray-service-only")
        assert set(roles) == {"tray"}
        check_dock("background entry", 0)
        receipt["checks"].append("explicit normal background bundle entry retains tray/service without a GUI role")

        normal = new_profile("normal", "normal")
        launch(normal)
        wait(lambda: native_window(normal))
        roles, before = checkpoint(normal, "normal-manual-visible")
        assert "gui" in roles and "tray" in roles and len({roles["gui"]["pid"], roles["tray"]["pid"], before["pid"]}) == 3
        check_dock("normal GUI open", 1)
        control(normal, "gui", "close")
        wait(lambda: not alive(roles["gui"]["pid"]))
        after_roles, after = checkpoint(normal, "normal-gui-closed")
        assert "gui" not in after_roles and after_roles["tray"]["pid"] == roles["tray"]["pid"] and after["id"] == before["id"] and after["pid"] == before["pid"]
        check_dock("normal GUI closed", 0)
        if args.dock:
            control(normal, "tray", "open-gui")
            wait(lambda: native_window(normal))
            reopened = probe(normal, "gui")
            assert reopened["pid"] != roles["gui"]["pid"]
            check_dock("normal GUI reopened", 1)
            control(normal, "gui", "close")
            wait(lambda: not alive(reopened["pid"]))
            check_dock("reopened GUI closed", 0)
        receipt["checks"].append("manual normal bundle entry reports an app-scoped visible GUI window despite saved background=true; normal close retains same tray/core")

        light = new_profile("light", "lightweight")
        launch(light)
        wait(lambda: native_window(light))
        roles, before = checkpoint(light, "light-manual-visible")
        assert "gui" in roles and "tray" in roles
        control(light, "tray", "open-quick-panel")
        wait(lambda: native_window(light, panel=True))
        panel_roles, _ = checkpoint(light, "light-panel-visible")
        assert "quick-panel" in panel_roles
        assert len({panel_roles["gui"]["pid"], panel_roles["tray"]["pid"], panel_roles["quick-panel"]["pid"], before["pid"]}) == 4
        check_dock("lightweight GUI and quick panel open", 1)
        control(light, "gui", "close")
        wait(lambda: not alive(roles["gui"]["pid"]) and not alive(roles["tray"]["pid"]) and not alive(panel_roles["quick-panel"]["pid"]))
        after_roles, after = checkpoint(light, "light-gui-closed")
        assert not after_roles and after["id"] == before["id"] and after["pid"] == before["pid"] and after["mode"] == "standalone"
        check_dock("lightweight GUI closed", 0)
        receipt["checks"].append("manual lightweight bundle entry reports app-scoped visible GUI/panel windows; close retires all Desktop roles and preserves same standalone service generation/PID")

        background = new_profile("background-light", "lightweight")
        launch(background, background=True)
        wait(lambda: service(background))
        roles, core = checkpoint(background, "background-light-service-only")
        assert not roles and core["mode"] == "standalone"
        receipt["checks"].append("explicit lightweight background bundle entry leaves only standalone unicd")

        pending = new_profile("pending-setup", "lightweight")
        (pending / "desktop-setup.json").write_text('{"complete":false}')
        launch(pending, background=True)
        windows = wait(lambda: native_window(pending))
        pending_gui = probe(pending, "gui")
        pending_tray = probe(pending, "tray")
        assert control(pending, "gui", "status")["setup_required"]
        assert not control(pending, "tray", "status")["ready"]
        assert not (pending / "unic.sock").exists()
        receipt["checkpoints"].append({"name": "pending-setup-background-gated", "at": time.time(), "roles": {"gui": pending_gui, "tray": pending_tray}, "app_window_states": windows, "service": None, "all_private_bundle_processes": private_processes()})
        receipt["checks"].append("pending setup overrides lightweight background entry and reports an app-scoped visible setup window without a service or adapter")
        if args.dock:
            receipt["dock"] = "PASS"
        receipt["status"] = "PASS"
    except BaseException as error:
        receipt["error"] = str(error)
        raise
    finally:
        receipt["private_bundle_processes_before_cleanup"] = private_processes()
        if executable is not None and executable.exists():
            for profile in profiles:
                for role in ["tray", "gui", "quick-panel"]:
                    try:
                        probe(profile, role)
                        control(profile, role, "quit")
                    except (AssertionError, OSError, subprocess.SubprocessError):
                        pass
                try:
                    service(profile, stop=True)
                except (AssertionError, OSError, subprocess.SubprocessError, http.client.HTTPException):
                    pass
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline and any(alive(pid) for pid in all_pids):
            time.sleep(0.08)
        remaining = [pid for pid in all_pids if alive(pid)]
        receipt["cleanup"] = {"observed_pids": sorted(all_pids), "remaining_before_forced_cleanup": remaining, "private_profile_removed": False}
        if remaining:
            receipt["status"] = "FAIL"
            for pid in remaining:
                os.kill(pid, signal.SIGTERM)
            time.sleep(0.2)
            for pid in remaining:
                if alive(pid):
                    os.kill(pid, signal.SIGKILL)
        receipt["cleanup"]["remaining_after_cleanup"] = [pid for pid in all_pids if alive(pid)]
        if executable is not None and executable.exists():
            unregister = subprocess.run(["/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister", "-u", str(executable.parents[2])], text=True, capture_output=True, timeout=10)
            receipt["cleanup"]["private_bundle_launchservices_unregister_exit"] = unregister.returncode
        for profile in profiles:
            if profile.exists():
                for path in profile.rglob("*.log"):
                    text = path.read_text(errors="replace")
                    if "DATA RACE" in text:
                        receipt["status"] = "FAIL"
                        receipt.setdefault("race_failures", []).append(str(path.relative_to(task)))
        shutil.rmtree(task)
        receipt["cleanup"]["private_profile_removed"] = not task.exists()
        path = output / "receipt.json"
        path.write_text(json.dumps(receipt, indent=2) + "\n")
        (output / "SHA256SUMS").write_text(sha(path) + "  receipt.json\n")
        print(path)
        if receipt["status"] != "PASS":
            raise SystemExit(1)


if __name__ == "__main__":
    main()
