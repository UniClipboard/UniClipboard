#!/usr/bin/env python3
"""Real-daemon probe for the Tauri -> Go upgrade continuity check (issue #1900).

  tauri_continuity_probe.py init    <out.json>   initialize the profile through the running daemon
  tauri_continuity_probe.py configure <out.json> enable autostart and restore-on-startup, disable automatic update download
  tauri_continuity_probe.py capture <out.json>   record the observable state (never a secret value)

The state is read through the daemon HTTP API of whichever package is installed, so the same probe runs
against the Tauri-era daemon before the upgrade and the Go-era daemon after it. Passphrases used here are
throwaway values for the task-owned container; nothing is read from or written to a real user profile.
"""

import json
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

PASSPHRASE = "t0218-throwaway-passphrase"
DEVICE_NAME = "t0218-tauri-origin"
ROOT = Path.home() / ".local/share/app.uniclipboard.desktop"


def connect():
    last = None
    for _ in range(600):
        try:
            conn = json.loads((ROOT / "daemon.conn").read_text())
            base = f"http://{conn['host']}:{conn['port']}"
            urllib.request.urlopen(base + "/health", timeout=2).close()
            return base, conn
        except (OSError, ValueError, KeyError) as error:
            last = error
            time.sleep(0.2)
    raise RuntimeError(f"daemon did not become healthy: {last}")


base, conn = connect()


def call(path, body=None, token=None, method=None):
    request = urllib.request.Request(
        base + path,
        data=None if body is None else json.dumps(body).encode(),
        headers={
            "Content-Type": "application/json",
            "Authorization": ("Session " + token) if token else ("Bearer " + conn["token"]),
        },
        method=method,
    )
    try:
        with urllib.request.urlopen(request, timeout=120) as response:
            raw = response.read()
            return {"status": response.status, "body": json.loads(raw) if raw else None}
    except urllib.error.HTTPError as error:
        raw = error.read()
        try:
            parsed = json.loads(raw)
        except ValueError:
            parsed = raw.decode(errors="replace")
        return {"status": error.code, "body": parsed}


def data(result):
    if result["status"] != 200:
        raise RuntimeError(f"unexpected {result}")
    return result["body"]["data"]


session = data(call("/auth/connect", {"pid": os.getpid(), "clientType": "cli"}))["sessionToken"]


def get(path):
    return call(path, token=session)


mode, out = sys.argv[1], Path(sys.argv[2])
if mode == "init":
    created = data(
        call(
            "/v2/setup/initialize",
            {"passphrase": PASSPHRASE, "passphraseConfirm": PASSPHRASE, "deviceName": DEVICE_NAME},
            session,
        )
    )
    out.write_text(json.dumps({"initialize": created}, indent=2) + "\n")
    print("initialized")
    sys.exit(0)

if mode == "configure":
    result = call("/settings", {"general": {"autoStart": True, "autoDownloadUpdate": False, "restoreLastEntryOnStartup": True}}, session, "PUT")
    out.write_text(json.dumps(result, indent=2) + "\n")
    sys.exit(0 if result["status"] == 200 else 1)

assert mode == "capture"
entries = get("/clipboard/entries?limit=1000")
state = {
    "daemon_version": conn.get("version"),
    "settings": get("/settings"),
    "device_me": get("/device/me"),
    "setup_state": get("/v2/setup/state"),
    "encryption_state": get("/encryption/state"),
    "upgrade_status": get("/upgrade/status"),
    "status": get("/status"),
    "stats": get("/clipboard/stats"),
    "entries": entries,
    "resources": {},
}
listed = entries["body"]["data"] if entries["status"] == 200 else None
items = listed.get("entries", listed) if isinstance(listed, dict) else (listed or [])
for item in items:
    entry_id = item.get("id") or item.get("entryId")
    state["resources"][entry_id] = get(f"/clipboard/entries/{entry_id}/resource")
out.write_text(json.dumps(state, indent=2, sort_keys=True) + "\n")
print(f"captured entries={len(items)}")
