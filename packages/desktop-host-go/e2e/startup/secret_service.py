#!/usr/bin/env python3
"""Linux-only private D-Bus/production-provider E2E. No production wallet access."""
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
import time

repo = pathlib.Path(__file__).resolve().parents[4]

if "--session" in sys.argv:
    mode = sys.argv[-1]
    root = pathlib.Path(os.environ["UC_READINESS_E2E_ROOT"])
    daemon = None
    if mode == "delayed":
        # Start the private wallet after the first probe fails.
        daemon = subprocess.Popen(["sh", "-c", "sleep 1; exec gnome-keyring-daemon --foreground --components=secrets --unlock"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        daemon.stdin.write(b"e2e-wallet-password\n")
        daemon.stdin.close()
    started = time.monotonic()
    run = subprocess.run([os.environ["UC_READINESS_E2E_BINARY"]], capture_output=True, text=True, timeout=10)
    elapsed = time.monotonic() - started
    if daemon:
        daemon.terminate()
        daemon.wait(timeout=5)
    result = {"case": mode, "exit": run.returncode, "elapsed": elapsed, "stdout": run.stdout, "stderr": run.stderr, "fileStoreCreated": (root / "keyring").exists()}
    passed = not result["fileStoreCreated"] and ((mode == "delayed" and run.returncode == 0) or (mode == "unavailable" and run.returncode != 0 and 2.8 <= elapsed < 6))
    result["passed"] = passed
    print(json.dumps(result))
    sys.exit(0 if passed else 1)

if sys.platform != "linux" or any(not shutil.which(x) for x in ("dbus-run-session", "gnome-keyring-daemon")):
    print(json.dumps({"status": "not-run", "reason": "requires Linux, dbus-run-session and gnome-keyring-daemon"}))
    sys.exit(77)
subprocess.run(["cargo", "build", "-p", "uc-platform", "--example", "secure_storage_readiness"], cwd=repo, check=True)
output = pathlib.Path(tempfile.mkdtemp(prefix="uc-secret-service-e2e-"))
results = []
for mode in ("delayed", "unavailable"):
    sandbox = output / mode
    root = sandbox / "profile"
    root.mkdir(parents=True)
    (root / "secure-storage-source.json").write_text('{"version":1,"store":"system"}')
    env = dict(os.environ)
    for key in ("DBUS_SESSION_BUS_ADDRESS", "GNOME_KEYRING_CONTROL", "UC_PORTABLE", "APPIMAGE", "APPDIR", "UC_FORCE_FILE_KEYRING"):
        env.pop(key, None)
    env.update(HOME=str(sandbox), XDG_RUNTIME_DIR=str(sandbox / "runtime"), XDG_DATA_HOME=str(sandbox / "data"), XDG_CONFIG_HOME=str(sandbox / "config"), XDG_CURRENT_DESKTOP="GNOME", DISPLAY=":e2e", UC_PROFILE="gui-go-secret-service-e2e", UC_READINESS_E2E_ROOT=str(root), UC_READINESS_E2E_BINARY=str(repo / "target/debug/examples/secure_storage_readiness"))
    pathlib.Path(env["XDG_RUNTIME_DIR"]).mkdir(mode=0o700)
    # Prevent D-Bus activation from masking the absent-service phase.
    config = sandbox / "bus.conf"
    config.write_text('<busconfig><type>session</type><listen>unix:tmpdir=/tmp</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
    run = subprocess.run(["dbus-run-session", "--config-file=" + str(config), "--", sys.executable, __file__, "--session", mode], env=env, capture_output=True, text=True, timeout=20)
    (sandbox / "result.json").write_text(run.stdout)
    (sandbox / "stderr.txt").write_text(run.stderr)
    results.append({"case": mode, "passed": run.returncode == 0})
print(json.dumps({"artifacts": str(output), "cases": results}))
sys.exit(0 if all(r["passed"] for r in results) else 1)
