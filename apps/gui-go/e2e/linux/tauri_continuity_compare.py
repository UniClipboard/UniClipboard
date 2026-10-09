#!/usr/bin/env python3
"""Compare the observable state captured before and after the Tauri -> Go package transaction.

  tauri_continuity_compare.py <before-dir> <after-dir> <results.tsv> [<control-dir> [<restart-dir>]]

Every invariant is one PASS/FAIL line. Fields that legitimately change (timestamps, uptime, package version) are not compared.
Settings keys the Go-era daemon adds are reported as information, never as a pass for a changed user value.
"""

import json
import sys
from pathlib import Path

before_dir, after_dir, results_path = (Path(a) for a in sys.argv[1:4])
control_dir = Path(sys.argv[4]) if len(sys.argv) > 4 else None
restart_dir = Path(sys.argv[5]) if len(sys.argv) > 5 else None
rows, failures = [], 0


def load(directory, name):
    return json.loads((directory / name).read_text())


def check(name, ok, detail=""):
    global failures
    rows.append(("PASS" if ok else "FAIL", name, str(detail)[:400]))
    failures += 0 if ok else 1


def unwrap(result):
    return result["body"]["data"] if result["status"] == 200 else {"httpStatus": result["status"], "body": result["body"]}


b, a = load(before_dir, "capture.json"), load(after_dir, "capture.json")
check("after: every probe endpoint answered 200", all(a[k]["status"] == 200 for k in ("settings", "device_me", "setup_state", "encryption_state", "entries", "stats")), {k: a[k]["status"] for k in ("settings", "device_me", "setup_state", "encryption_state", "entries", "stats")})
check("device identity (peerId, deviceName) is unchanged", unwrap(b["device_me"]) == unwrap(a["device_me"]), f"{unwrap(b['device_me'])} -> {unwrap(a['device_me'])}")
check("setup state is unchanged and completed", unwrap(b["setup_state"]) == unwrap(a["setup_state"]) and unwrap(a["setup_state"]).get("hasCompleted") is True, unwrap(a["setup_state"]))
check("encryption is initialized with a ready session", unwrap(a["encryption_state"]) == {"initialized": True, "sessionReady": True}, unwrap(a["encryption_state"]))
be, ae = unwrap(b["entries"]), unwrap(a["entries"])
fields = ("id", "contentType", "preview", "sizeBytes", "isFavorited", "capturedAt")
norm = lambda es: [{k: e.get(k) for k in fields} for e in es]
check("history: same entry ids, previews, sizes and capture times", norm(be) == norm(ae), f"before={len(be)} after={len(ae)}")
check("history: stats are unchanged", unwrap(b["stats"]) == unwrap(a["stats"]), f"{unwrap(b['stats'])} -> {unwrap(a['stats'])}")
res_equal = {k: unwrap(v) for k, v in b["resources"].items()} == {k: unwrap(v) for k, v in a["resources"].items()}
check("history: decrypted content of every entry is byte-identical", res_equal and len(b["resources"]) == len(be))
bs, as_ = unwrap(b["settings"]), unwrap(a["settings"])


def lost_or_changed(old, new, path=""):
    out = []
    for key, value in old.items():
        if key not in new:
            out.append(f"{path}{key}: removed")
        elif isinstance(value, dict) and isinstance(new[key], dict):
            out += lost_or_changed(value, new[key], f"{path}{key}.")
        elif value != new[key]:
            out.append(f"{path}{key}: {value!r} -> {new[key]!r}")
    return out


def added(old, new, path=""):
    out = []
    for key, value in new.items():
        if key not in old:
            out.append(f"{path}{key}")
        elif isinstance(value, dict) and isinstance(old[key], dict):
            out += added(old[key], value, f"{path}{key}.")
    return out


changed = lost_or_changed(bs, as_)
check("settings: no value removed or changed", not changed, changed)
rows.append(("INFO", "settings keys added by the new daemon", ",".join(added(bs, as_)) or "none"))
kb, ka = load(before_dir, "keyring.json"), load(after_dir, "keyring.json")
check("keyring: same item count", len(kb) == len(ka), f"{len(kb)} -> {len(ka)}")
check("keyring: same labels, attributes and secret digests", kb == ka)
mb, ma = load(before_dir, "datafiles.json"), load(after_dir, "datafiles.json")
check("data root: no persistent file lost", not [p for p in mb if p not in ma], [p for p in mb if p not in ma])
# Files that carry the installation identity must be byte-identical; everything else may be rewritten by a migration.
identity = lambda p: p == "device_id.txt" or p == "vault/device_id.txt" or p == "vault/keyslot.json" or p == "storage/salt" or p.startswith("iroh-identity/") or p.startswith("analytics/")
changed_identity = [p for p in mb if identity(p) and ma.get(p) != mb[p]]
check("data root: identity files (device id, key slot, salt, network identity, analytics ids) are byte-identical", not changed_identity, changed_identity)
control = load(control_dir, "datafiles.json") if control_dir and (control_dir / "datafiles.json").exists() else None
if control is not None:
    churn = {p for p in mb if p in control and control[p] != mb[p]}
    rows.append(("INFO", "data root: files that change on an ordinary Tauri restart (control)", ",".join(sorted(churn)) or "none"))
    upgrade_only = sorted(p for p in mb if p in ma and ma[p] != mb[p] and p not in churn)
    rows.append(("INFO", "data root: files changed by the upgrade beyond ordinary restart churn", ",".join(upgrade_only) or "none"))
else:
    rows.append(("INFO", "data root: files whose content changed", ",".join(sorted(p for p in mb if p in ma and ma[p] != mb[p])) or "none"))
rows.append(("INFO", "data root: files added", ",".join(sorted(p for p in ma if p not in mb)) or "none"))
if restart_dir is not None and (restart_dir / "capture.json").exists():
    r = load(restart_dir, "capture.json")
    check("restart: history and device identity survive a second Go start", norm(unwrap(r["entries"])) == norm(be) and unwrap(r["device_me"]) == unwrap(b["device_me"]))
    check("restart: keyring is unchanged", load(restart_dir, "keyring.json") == kb)
lb, la = load(before_dir, "localstorage.json"), load(after_dir, "localstorage.json")
lost = sorted(f"{db}:{k}" for db, keys in lb.items() for k in keys if not any(k in other for other in la.values()))
check("web storage: every localStorage key the Tauri frontend wrote exists in the Go host's storage", not lost, lost)
rows.append(("INFO", "web storage before (databases: key counts)", json.dumps({k: len(v) for k, v in lb.items()})))
rows.append(("INFO", "web storage after (databases: key counts)", json.dumps({k: len(v) for k, v in la.items()})))
Path(results_path).write_text("".join("\t".join(r) + "\n" for r in rows))
print("".join("\t".join(r) + "\n" for r in rows), end="")
sys.exit(failures)
