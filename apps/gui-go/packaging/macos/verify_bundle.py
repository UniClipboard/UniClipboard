#!/usr/bin/env python3
"""Verify the macOS release bundle contract of the Go/Wails desktop host.

The contract is the Definition of Done of issue #1895 turned into assertions. It inspects a built
`.app` (or an archive extracted from a DMG / `.app.tar.gz`) and never launches it, so it is safe on
a developer machine. Runtime behaviour is covered by `e2e/macos_bundle_run.py`.

  verify_bundle.py --app UniClipboard.app --arch arm64 --signature developer-id --hardened --out report.json

`--signature` is what the bundle must carry: `none` (unsigned), `adhoc`, or `developer-id`.
`--notarized` additionally requires a stapled ticket and a Gatekeeper `accepted` verdict.
Every check is recorded in the report; the exit status is non-zero if any failed. Checks that
cannot apply to the requested level are not silently dropped: they are listed as `skipped`.
"""
import argparse
import hashlib
import json
import os
import plistlib
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
APP_JSON = REPO / "apps/gui-go/app.json"
ICNS = REPO / "apps/gui-go/icons/icon.icns"

# Executables inside Contents/MacOS and the code-signing identifier each one must carry. The main
# executable is signed as the bundle (identifier = bundle id); the helpers get a stable identifier
# derived from it so their designated requirements do not depend on the build path.
GUI = "gui-go"
DAEMON = "uniclipd"
HELPER = "uniclip-quick-panel"


def run(*cmd):
    p = subprocess.run(cmd, capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


class Report:
    def __init__(self):
        self.checks = []

    def add(self, name, ok, detail="", skipped=False):
        self.checks.append({"name": name, "ok": bool(ok), "detail": detail, "skipped": skipped})
        status = "SKIP" if skipped else ("PASS" if ok else "FAIL")
        print(f"[{status}] {name}" + (f": {detail}" if detail and not ok else ""))

    @property
    def failed(self):
        return [c for c in self.checks if not c["ok"] and not c["skipped"]]


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def codesign_info(path):
    code, out = run("codesign", "-d", "--verbose=4", str(path))
    return code, out


def entitlements_of(path):
    code, out = run("codesign", "-d", "--entitlements", ":-", str(path))
    return out.strip() if code == 0 else "codesign-failed"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--app", required=True)
    ap.add_argument("--arch", required=True, choices=["arm64", "x86_64"])
    ap.add_argument("--signature", default="none", choices=["none", "adhoc", "developer-id"])
    ap.add_argument("--hardened", action="store_true")
    ap.add_argument("--notarized", action="store_true")
    ap.add_argument("--identity-suffix", default="", help="bundle id suffix of a test variant, e.g. .e2e")
    ap.add_argument("--expect-team", default="")
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    r = Report()
    app = Path(args.app)
    cfg = json.loads(APP_JSON.read_text())
    bundle_id = cfg["identifier"] + args.identity_suffix

    r.add("bundle directory exists", app.is_dir(), str(app))
    if not app.is_dir():
        return finish(r, args)

    # --- Info.plist: identity, version, icon ---------------------------------------------------
    plist_path = app / "Contents/Info.plist"
    plist = plistlib.loads(plist_path.read_bytes()) if plist_path.is_file() else {}
    r.add("Info.plist present", bool(plist), str(plist_path))
    r.add("CFBundleIdentifier from app.json", plist.get("CFBundleIdentifier") == bundle_id,
          f"{plist.get('CFBundleIdentifier')!r} != {bundle_id!r}")
    r.add("CFBundleName from app.json", plist.get("CFBundleName", "").startswith(cfg["productName"]),
          repr(plist.get("CFBundleName")))
    r.add("CFBundleShortVersionString from app.json", plist.get("CFBundleShortVersionString") == cfg["version"],
          f"{plist.get('CFBundleShortVersionString')!r} != {cfg['version']!r}")
    r.add("CFBundleVersion from app.json", plist.get("CFBundleVersion") == cfg["version"],
          f"{plist.get('CFBundleVersion')!r} != {cfg['version']!r}")
    r.add("LSMinimumSystemVersion from app.json", plist.get("LSMinimumSystemVersion") == cfg["minimumSystemVersion"],
          f"{plist.get('LSMinimumSystemVersion')!r} != {cfg['minimumSystemVersion']!r}")
    r.add("CFBundleExecutable is the GUI binary", plist.get("CFBundleExecutable") == GUI,
          repr(plist.get("CFBundleExecutable")))
    icon = plist.get("CFBundleIconFile", "")
    icon_path = app / "Contents/Resources" / (icon if icon.endswith(".icns") else icon + ".icns") if icon else None
    r.add("CFBundleIconFile resolves to a bundled .icns", bool(icon_path and icon_path.is_file()), repr(icon))
    if icon_path and icon_path.is_file():
        r.add(".icns is the repository icon (icons/icon.icns)", sha256(icon_path) == sha256(ICNS))

    # --- nested executables: present, executable, single requested architecture ---------------
    macos = app / "Contents/MacOS"
    for name in (GUI, DAEMON, HELPER):
        p = macos / name
        r.add(f"Contents/MacOS/{name} present and executable", p.is_file() and os.access(p, os.X_OK), str(p))
        if p.is_file():
            code, out = run("lipo", "-archs", str(p))
            r.add(f"{name} is a single-arch {args.arch} Mach-O", code == 0 and out.split() == [args.arch], out.strip())
            _, vt = run("vtool", "-show-build", str(p))
            m = re.search(r"minos (\d+)\.(\d+)", vt)
            ok = bool(m) and (int(m.group(1)), int(m.group(2))) <= tuple(int(x) for x in cfg["minimumSystemVersion"].split("."))
            r.add(f"{name} minimum macOS <= {cfg['minimumSystemVersion']}", ok, m.group(0) if m else vt[-120:])
    extra = sorted(x.name for x in macos.iterdir()) if macos.is_dir() else []
    r.add("Contents/MacOS holds exactly the three executables", extra == sorted([GUI, DAEMON, HELPER]), str(extra))

    # --- no packaging debris -------------------------------------------------------------------
    debris = [str(p.relative_to(app)) for p in app.rglob("*") if p.name.startswith("._") or p.name == ".DS_Store"]
    r.add("no AppleDouble / .DS_Store files", not debris, str(debris[:5]))
    code, out = run("xattr", "-r", str(app))
    quarantine = [l for l in out.splitlines() if "com.apple.quarantine" in l]
    r.add("no quarantine attribute inside the bundle", not quarantine, str(quarantine[:3]))

    # --- signatures ----------------------------------------------------------------------------
    targets = {GUI: bundle_id, DAEMON: f"{cfg['identifier']}.uniclipd" + args.identity_suffix,
               HELPER: f"{cfg['identifier']}.quick-panel" + args.identity_suffix}
    if args.signature == "none":
        r.add("bundle is unsigned", run("codesign", "-v", str(app))[0] != 0)
    else:
        code, out = run("codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app))
        r.add("codesign --verify --deep --strict", code == 0, out.strip()[-300:])
        teams = set()
        for name, ident in targets.items():
            path = app if name == GUI else macos / name
            code, info = codesign_info(path)
            got = re.search(r"^Identifier=(.*)$", info, re.M)
            r.add(f"{name}: signing identifier {ident}", bool(got) and got.group(1) == ident, got.group(1) if got else info[-200:])
            flags = re.search(r"flags=0x[0-9a-f]+\(([^)]*)\)", info)
            has_runtime = bool(flags) and "runtime" in flags.group(1)
            if args.hardened:
                r.add(f"{name}: hardened runtime flag", has_runtime, flags.group(0) if flags else "no flags")
            if args.signature == "developer-id":
                auth = re.findall(r"^Authority=(.*)$", info, re.M)
                r.add(f"{name}: Developer ID Application authority",
                      bool(auth) and auth[0].startswith("Developer ID Application:"), str(auth[:1]))
                r.add(f"{name}: secure timestamp", bool(re.search(r"^Timestamp=", info, re.M)))
                team = re.search(r"^TeamIdentifier=(.*)$", info, re.M)
                teams.add(team.group(1) if team else None)
            else:
                r.add(f"{name}: ad-hoc signature", "Signature=adhoc" in info, info[-200:])
            ent = entitlements_of(path)
            # Minimal entitlements: the Tauri host that shipped before this one carried none, and the
            # hardened-runtime acceptance run (e2e/macos_bundle_run.py) needs none.
            r.add(f"{name}: no entitlements", ent != "codesign-failed" and ("<key>" not in ent), str(ent)[:200])
        if args.signature == "developer-id":
            r.add("one team identifier across all code", len(teams) == 1 and None not in teams, str(teams))
            if args.expect_team:
                r.add("team identifier is the expected one", teams == {args.expect_team}, str(teams))
        # Nested code must be signed by the same identity as the bundle (sealed resources cover it).
        code, out = run("codesign", "-d", "-r-", str(app))
        r.add("designated requirement (identifier for Developer ID, cdhash for ad-hoc)", code == 0 and (f'identifier "{bundle_id}"' in out if args.signature == "developer-id" else "cdhash" in out), out.strip()[-200:])

    if args.notarized:
        code, out = run("xcrun", "stapler", "validate", str(app))
        r.add("stapler validate", code == 0, out.strip()[-300:])
        code, out = run("spctl", "-a", "-vv", "-t", "exec", str(app))
        r.add("spctl accepts the bundle (Notarized Developer ID)",
              code == 0 and "accepted" in out and "Notarized Developer ID" in out, out.strip()[-300:])
    else:
        r.add("notarization checks", True, "not requested", skipped=True)

    return finish(r, args)


def finish(r, args):
    failed = r.failed
    print(f"{len(r.checks) - len(failed)}/{len(r.checks)} checks passed")
    if args.out:
        Path(args.out).write_text(json.dumps({"app": args.app, "arch": args.arch, "signature": args.signature,
                                              "hardened": args.hardened, "notarized": args.notarized,
                                              "passed": not failed, "checks": r.checks}, indent=2))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
