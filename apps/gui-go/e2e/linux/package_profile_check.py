#!/usr/bin/env python3
"""Real daemon API fixture for package transactions; never exports credentials."""

import json
import os
import shlex
from pathlib import Path
import sys
import time
import urllib.request

mode = sys.argv[1]
if mode == "exec":
    entry = Path.home() / ".config/autostart/UniClipboard.desktop"
    lines = entry.read_text().splitlines()
    assert "Hidden=true" not in lines and "X-GNOME-Autostart-enabled=false" not in lines
    command = shlex.split(next(line[5:] for line in lines if line.startswith("Exec=")))
    assert command == ["/usr/bin/uniclipboard", "--autostart"]
    print("\n".join(command))
    sys.exit(0)
root = Path.home() / ".local/share/app.uniclipboard.desktop"
for _ in range(300):
    try:
        conn = json.loads((root / "daemon.conn").read_text())
        base = f"http://{conn['host']}:{conn['port']}"
        urllib.request.urlopen(base + "/health", timeout=2).close()
        break
    except (OSError, ValueError, KeyError):
        time.sleep(0.2)
else:
    raise RuntimeError("daemon did not become healthy")


def request(path, body=None, token=None, method=None):
    req = urllib.request.Request(
        base + path,
        data=None if body is None else json.dumps(body).encode(),
        headers={
            "Content-Type": "application/json",
            "Authorization": ("Session " + token)
            if token
            else ("Bearer " + conn["token"]),
        },
        method=method,
    )
    with urllib.request.urlopen(req, timeout=120) as response:
        return json.load(response)["data"]


session = request("/auth/connect", {"pid": os.getpid(), "clientType": "cli"})[
    "sessionToken"
]
if mode == "seed":
    created = request(
        "/v2/setup/initialize",
        {
            "passphrase": "package-synthetic-passphrase",
            "passphraseConfirm": "package-synthetic-passphrase",
            "deviceName": "package-transaction-fixture",
        },
        session,
    )
    request(
        "/settings",
        {"general": {"autoStart": True, "autoDownloadUpdate": False}},
        session,
        "PUT",
    )
    entry = Path.home() / ".config/autostart/UniClipboard.desktop"
    entry.parent.mkdir(parents=True, exist_ok=True)
    entry.write_text(
        "[Desktop Entry]\nType=Application\nName=UniClipboard\nExec=/usr/bin/uniclipboard --autostart\nX-GNOME-Autostart-enabled=true\nHidden=false\nTerminal=false\n"
    )
    Path("/tmp/space-fixture.json").write_text(json.dumps(created))
else:
    assert mode == "verify"
settings = request("/settings", token=session)
assert settings["general"]["autoStart"] is True
assert settings["general"]["autoDownloadUpdate"] is False
encryption = request("/encryption/state", token=session)
assert encryption["initialized"] and encryption["sessionReady"]
status = request("/status", token=session)
identity = request("/device/me", token=session)
setup = request("/v2/setup/state", token=session)
assert setup["hasCompleted"] is True
if mode == "seed":
    Path("/tmp/profile-identity.json").write_text(json.dumps(identity))
else:
    assert identity == json.loads(Path("/tmp/profile-identity.json").read_text())
Path("/out/profile-" + mode + ".json").write_text(
    json.dumps(
        {
            "settings": settings,
            "status": status,
            "identity": identity,
            "setup": setup,
            "encryption": encryption,
        },
        indent=2,
    )
    + "\n"
)
print("profile-" + mode + "-verified")
