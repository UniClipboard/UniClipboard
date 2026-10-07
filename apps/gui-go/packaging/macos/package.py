#!/usr/bin/env python3
"""Build, sign, notarize and package the macOS release of the Go/Wails desktop host (issue #1895).

One tool, one subcommand per stage, so CI and a developer run the same steps in the same order:

  bundle    build the GUI for one architecture and assemble UniClipboard.app around the staged
            release `uniclipd` and `uniclip-quick-panel` (scripts/stage-daemon.mjs output)
  sign      sign inside-out: nested executables first, then the bundle (never `--deep`)
  notarize  submit to the Apple notary service, wait, keep the log, staple the ticket
  dmg       UniClipboard_<version>_<aarch64|x64>.dmg (signed, notarized, stapled when asked)
  archive   UniClipboard.app.tar.gz from the finished bundle (the updater signature is separate: #1896)
  keychain  create / delete the temporary keychain that holds the Developer ID certificate in CI

The bundle identity (name, identifier, version, minimum macOS, icon) comes from apps/gui-go/app.json
and apps/gui-go/icons/icon.icns only. Credentials are read from the environment and never printed.
Why this is not `wails3 tool sign` / `wails3 tool package`: see "Wails 能力审计" in apps/gui-go/README.md.
"""
import argparse
import hashlib
import json
import os
import platform
import plistlib
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]
GUI_DIR = REPO / "apps/gui-go"
CFG = json.loads((GUI_DIR / "app.json").read_text())
TRIPLES = {
    "aarch64-apple-darwin": {"goarch": "arm64", "lipo": "arm64", "dmg": "aarch64"},
    "x86_64-apple-darwin": {"goarch": "amd64", "lipo": "x86_64", "dmg": "x64"},
}
# The test variant is the same bundle layout with the E2E control plane and a distinct identity, so
# it can never collide with an installed app (single instance scope, login item, TCC grants).
VARIANTS = {
    "shipping": {"tags": "production,release", "suffix": "", "e2e": "0"},
    "acceptance": {"tags": "e2e", "suffix": ".e2e", "e2e": "1"},
}
GUI, DAEMON, HELPER = "gui-go", "uniclipd", "uniclip-quick-panel"


def log(msg):
    print(f"[package] {msg}", flush=True)


def run(cmd, *, cwd=None, env=None, capture=False, check=True, secret=()):
    shown = " ".join(str(c) for c in cmd)
    for s in secret:
        shown = shown.replace(s, "***")
    log("$ " + shown)
    p = subprocess.run([str(c) for c in cmd], cwd=cwd, env=env, text=True,
                       stdout=subprocess.PIPE if capture else None, stderr=subprocess.STDOUT if capture else None)
    if check and p.returncode != 0:
        if capture and p.stdout:
            print(p.stdout.replace(secret[0], "***") if secret else p.stdout, file=sys.stderr)
        sys.exit(f"command failed ({p.returncode}): {shown}")
    return p


def out(cmd, **kw):
    return run(cmd, capture=True, **kw).stdout.strip()


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def bundle_id(variant):
    return CFG["identifier"] + VARIANTS[variant]["suffix"]


def app_name(variant):
    return CFG["productName"] + (" E2E Test Build" if variant == "acceptance" else "")


def git_state():
    head = out(["git", "rev-parse", "HEAD"], cwd=REPO)
    status = out(["git", "status", "--porcelain"], cwd=REPO)
    diff = subprocess.run(["git", "diff", "HEAD"], cwd=REPO, capture_output=True).stdout
    return {"head": head, "dirty": bool(status), "diffSha256": hashlib.sha256(diff).hexdigest() if status else None}


def engine_pin():
    lock = (REPO / "Cargo.lock").read_text()
    sources = sorted({l.split('"')[1] for l in lock.splitlines() if l.startswith("source = ") and "Engine.git" in l})
    return sources


# --------------------------------------------------------------------------------------------- bundle
def cmd_bundle(a):
    t = TRIPLES[a.target]
    v = VARIANTS[a.variant]
    sidecars = Path(a.sidecars)
    daemon_src = sidecars / f"{DAEMON}-{a.target}"
    helper_src = sidecars / f"{HELPER}-{a.target}"
    for p in (daemon_src, helper_src):
        if not p.is_file():
            sys.exit(f"missing staged binary {p}; run `node scripts/stage-daemon.mjs --target {a.target}` first")
    for p in (daemon_src, helper_src):
        archs = out(["lipo", "-archs", p]).split()
        if archs != [t["lipo"]]:
            sys.exit(f"{p} is {archs}, expected [{t['lipo']}] for {a.target}")

    outdir = Path(a.out).resolve()
    work = outdir / "work"
    shutil.rmtree(outdir, ignore_errors=True)
    work.mkdir(parents=True)

    env = dict(os.environ)
    env["VITE_GUI_GO_E2E"] = v["e2e"]
    env["VITE_APP_VERSION"] = CFG["version"]
    log("frontend build")
    run(["bun", "--bun", "run", "--cwd", "apps/gui-go", "build"], cwd=REPO, env=env)
    dist = GUI_DIR / "frontend/dist"
    maps = sorted(str(p.relative_to(dist)) for p in dist.rglob("*.map"))
    if maps:
        sys.exit(f"source maps survived into the embedded frontend: {maps[:3]}")

    run(["go", "generate", "./buildinfo"], cwd=REPO / "packages/desktop-host-go")
    (GUI_DIR / "assets").mkdir(exist_ok=True)
    shutil.copy(GUI_DIR / "icons/tray-icon@2x.png", GUI_DIR / "assets/tray-icon@2x.png")

    pubkey = CFG["updater"]["pubkey"] if a.variant == "shipping" else ""
    minos = CFG["minimumSystemVersion"]
    genv = dict(os.environ, GOOS="darwin", GOARCH=t["goarch"], CGO_ENABLED="1", MACOSX_DEPLOYMENT_TARGET=minos,
                CGO_CFLAGS=f"-mmacosx-version-min={minos}", CGO_LDFLAGS=f"-mmacosx-version-min={minos}")
    ldflags = (f"-s -w -X main.updaterPublicKey={pubkey} -X main.productName={CFG['productName']}"
               f" -X main.bundleID={bundle_id(a.variant)}")
    gobin = work / "gui-go"
    run(["go", "build", "-tags", v["tags"], "-trimpath", "-buildvcs=false", "-ldflags", ldflags, "-o", gobin, "."],
        cwd=GUI_DIR, env=genv)

    bundle = outdir / f"{app_name(a.variant)}.app"
    macos = bundle / "Contents/MacOS"
    res = bundle / "Contents/Resources"
    macos.mkdir(parents=True)
    res.mkdir(parents=True)
    shutil.copy2(gobin, macos / GUI)
    shutil.copy2(daemon_src, macos / DAEMON)
    shutil.copy2(helper_src, macos / HELPER)
    for n in (GUI, DAEMON, HELPER):
        (macos / n).chmod(0o755)
    shutil.copy2(GUI_DIR / "icons/icon.icns", res / "icon.icns")

    plist = plistlib.loads((GUI_DIR / "Info.plist").read_bytes())
    plist.update({
        "CFBundleIdentifier": bundle_id(a.variant),
        "CFBundleName": app_name(a.variant),
        "CFBundleDisplayName": app_name(a.variant),
        "CFBundleShortVersionString": CFG["version"],
        "CFBundleVersion": CFG["version"],
        "CFBundleIconFile": "icon",
        "LSMinimumSystemVersion": minos,
        "LSApplicationCategoryType": "public.app-category.utilities",
    })
    (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(plist))
    (bundle / "Contents/PkgInfo").write_text("APPL????")

    host = platform.machine()
    prov = {
        "target": a.target, "variant": a.variant, "goTags": v["tags"], "bundleIdentifier": bundle_id(a.variant),
        "version": CFG["version"], "git": git_state(), "enginePin": engine_pin(),
        "host": {"machine": host, "macOS": platform.mac_ver()[0],
                 "nativeForTarget": host == t["lipo"] or (host == "arm64" and t["lipo"] == "arm64")},
        "toolchain": {
            "rustc": out(["rustc", "-Vv"], cwd=REPO), "cargo": out(["cargo", "-V"], cwd=REPO),
            "goInModule": out(["go", "version"], cwd=GUI_DIR), "GOTOOLCHAIN": os.environ.get("GOTOOLCHAIN", ""),
            "bun": out(["bun", "--version"]), "xcode": out(["xcodebuild", "-version"], check=False) if shutil.which("xcodebuild") else "",
            "sdk": out(["xcrun", "--show-sdk-version"], check=False),
        },
        "commands": {"goBuild": f"GOOS=darwin GOARCH={t['goarch']} CGO_ENABLED=1 MACOSX_DEPLOYMENT_TARGET={minos} "
                                f"go build -tags {v['tags']} -trimpath -buildvcs=false -ldflags '<redacted pubkey>' ."},
        "telemetryInputsPresent": {k: bool(os.environ.get(k)) for k in
                                   ("VITE_SENTRY_DSN", "VITE_APP_ENV", "SENTRY_AUTH_TOKEN", "SENTRY_ORG", "VITE_SENTRY_PROJECT")},
        "frontendSourceMapsEmbedded": False,
        "preSignSha256": {n: sha256(macos / n) for n in (GUI, DAEMON, HELPER)},
        "stagedInputsSha256": {DAEMON: sha256(daemon_src), HELPER: sha256(helper_src)},
    }
    (outdir / "provenance.json").write_text(json.dumps(prov, indent=2))
    log(f"assembled {bundle}")
    print(bundle)


# ----------------------------------------------------------------------------------------------- sign
def codesign_args(identity, keychain):
    args = ["codesign", "--force", "--sign", identity, "--options", "runtime"]
    if identity != "-":
        args += ["--timestamp"]
    if keychain:
        args += ["--keychain", keychain]
    return args


def cmd_sign(a):
    bundle = Path(a.app)
    macos = bundle / "Contents/MacOS"
    ident = bundle_id(a.variant)
    run(["xattr", "-cr", bundle])
    # Inside-out. The helper and the daemon are separate executables with their own signing
    # identifier; they are sealed into the bundle's CodeResources when the bundle is signed last.
    # `--deep` is deliberately not used: it signs everything with one identifier and entitlement set
    # and Apple deprecates it for distribution signing.
    for name, suffix in ((DAEMON, ".uniclipd"), (HELPER, ".quick-panel")):
        run(codesign_args(a.identity, a.keychain) + ["--identifier", CFG["identifier"] + suffix + VARIANTS[a.variant]["suffix"],
                                                       macos / name])
    ent = ["--entitlements", a.entitlements] if a.entitlements else []
    run(codesign_args(a.identity, a.keychain) + ["--identifier", ident] + ent + [bundle])
    run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", bundle])


# ------------------------------------------------------------------------------------------ notarize
def notary_args():
    for k in ("APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID"):
        if not os.environ.get(k):
            sys.exit(f"{k} is not set; notarization needs APPLE_ID, APPLE_PASSWORD and APPLE_TEAM_ID")
    return ["--apple-id", os.environ["APPLE_ID"], "--password", os.environ["APPLE_PASSWORD"],
            "--team-id", os.environ["APPLE_TEAM_ID"]]


def notarize_path(path, evidence_dir, staple_target=None):
    evidence = Path(evidence_dir)
    evidence.mkdir(parents=True, exist_ok=True)
    submit = path
    tmp = None
    if Path(path).is_dir():
        tmp = Path(tempfile.mkdtemp()) / (Path(path).name + ".zip")
        run(["ditto", "-c", "-k", "--keepParent", path, tmp])
        submit = tmp
    secrets = (os.environ["APPLE_PASSWORD"], os.environ["APPLE_ID"]) if os.environ.get("APPLE_PASSWORD") else ()
    p = run(["xcrun", "notarytool", "submit", submit, *notary_args(), "--wait", "--output-format", "json"],
            capture=True, check=False, secret=secrets)
    text = p.stdout or ""
    for s in secrets:
        text = text.replace(s, "***")
    stem = Path(path).name
    (evidence / f"{stem}.notary-submit.json").write_text(text)
    try:
        result = json.loads(text[text.index("{"):])
    except ValueError:
        sys.exit(f"notarytool returned no JSON for {stem}: {text[-400:]}")
    sid = result.get("id", "")
    if sid:
        log_p = run(["xcrun", "notarytool", "log", sid, *notary_args()], capture=True, check=False, secret=secrets)
        (evidence / f"{stem}.notary-log.json").write_text(log_p.stdout or "")
    if p.returncode != 0 or result.get("status") != "Accepted":
        sys.exit(f"notarization of {stem} ended with status {result.get('status')!r} (submission {sid}); see {evidence}")
    log(f"notarization Accepted for {stem} (submission {sid})")
    run(["xcrun", "stapler", "staple", staple_target or path])
    run(["xcrun", "stapler", "validate", staple_target or path])
    if tmp:
        shutil.rmtree(tmp.parent, ignore_errors=True)


def cmd_notarize(a):
    notarize_path(a.app, a.evidence)


# --------------------------------------------------------------------------------------------- dmg
def dmg_name(version, target):
    return f"{CFG['productName']}_{version}_{TRIPLES[target]['dmg']}.dmg"


def cmd_dmg(a):
    outdir = Path(a.out)
    outdir.mkdir(parents=True, exist_ok=True)
    dmg = outdir / dmg_name(CFG["version"], a.target)
    stage = Path(tempfile.mkdtemp(prefix="uc-dmg-"))
    try:
        app = Path(a.app)
        run(["ditto", app, stage / app.name])
        os.symlink("/Applications", stage / "Applications")
        if dmg.exists():
            dmg.unlink()
        run(["hdiutil", "create", "-volname", CFG["productName"], "-srcfolder", stage, "-fs", "HFS+", "-format", "UDZO", dmg])
    finally:
        shutil.rmtree(stage, ignore_errors=True)
    if a.identity:
        run(["codesign", "--force", "--sign", a.identity, "--timestamp"] + (["--keychain", a.keychain] if a.keychain else []) + [dmg])
        run(["codesign", "--verify", "--strict", "--verbose=2", dmg])
    if a.notarize:
        notarize_path(dmg, a.evidence)
    print(dmg)


# ----------------------------------------------------------------------------------------- archive
def cmd_archive(a):
    app = Path(a.app)
    dest = Path(a.out)
    dest.parent.mkdir(parents=True, exist_ok=True)
    # COPYFILE_DISABLE keeps bsdtar from adding AppleDouble `._*` entries, which would be extracted
    # next to the signed files and break the seal. The top level must be exactly one `.app`.
    env = dict(os.environ, COPYFILE_DISABLE="1")
    run(["tar", "-czf", dest, "-C", app.parent, app.name], env=env)
    print(dest)


# -------------------------------------------------------------------------------------- keychain
def cmd_keychain(a):
    if a.action == "delete":
        run(["security", "delete-keychain", a.path], check=False)
        return
    for k in ("APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD"):
        if not os.environ.get(k):
            sys.exit(f"{k} is not set")
    password = a.password or os.environ.get("KEYCHAIN_PASSWORD") or hashlib.sha256(os.urandom(32)).hexdigest()
    p12 = Path(tempfile.mkdtemp(prefix="uc-cert-")) / "certificate.p12"
    import base64
    p12.write_bytes(base64.b64decode(os.environ["APPLE_CERTIFICATE"]))
    p12.chmod(0o600)
    try:
        run(["security", "create-keychain", "-p", password, a.path])
        run(["security", "set-keychain-settings", "-lut", "21600", a.path])
        run(["security", "unlock-keychain", "-p", password, a.path], secret=(password,))
        # The same import and partition list as scripts/ci/package-cli.sh, which already signs the CLI with
        # these secrets: `-A` lets codesign use the key without a prompt, the partition list covers Apple tools.
        run(["security", "import", p12, "-P", os.environ["APPLE_CERTIFICATE_PASSWORD"], "-A", "-t", "cert", "-f", "pkcs12",
             "-k", a.path], secret=(os.environ["APPLE_CERTIFICATE_PASSWORD"],))
        run(["security", "set-key-partition-list", "-S", "apple-tool:,apple:", "-k", password, a.path],
            secret=(password,), capture=True)
        # Search list: only the temporary keychain is added in front; the login keychain stays untouched.
        existing = [l.strip().strip('"') for l in out(["security", "list-keychains", "-d", "user"]).splitlines()]
        run(["security", "list-keychains", "-d", "user", "-s", a.path, *existing])
    finally:
        shutil.rmtree(p12.parent, ignore_errors=True)
    ids = out(["security", "find-identity", "-v", "-p", "codesigning", a.path])
    lines = [l for l in ids.splitlines() if "Developer ID Application" in l]
    if len(lines) != 1:
        sys.exit(f"expected exactly one Developer ID Application identity in the temporary keychain, found {len(lines)}")
    print(lines[0].split()[1])  # the SHA-1 of the identity: unambiguous, unlike its name


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    b = sub.add_parser("bundle")
    b.add_argument("--target", required=True, choices=sorted(TRIPLES))
    b.add_argument("--variant", default="shipping", choices=sorted(VARIANTS))
    b.add_argument("--sidecars", default=str(REPO / "target/sidecar-staging"))
    b.add_argument("--out", required=True)
    b.set_defaults(fn=cmd_bundle)

    s = sub.add_parser("sign")
    s.add_argument("--app", required=True)
    s.add_argument("--variant", default="shipping", choices=sorted(VARIANTS))
    s.add_argument("--identity", default="-", help="signing identity (SHA-1 or name), '-' for ad-hoc")
    s.add_argument("--keychain", default="")
    s.add_argument("--entitlements", default="")
    s.set_defaults(fn=cmd_sign)

    n = sub.add_parser("notarize")
    n.add_argument("--app", required=True)
    n.add_argument("--evidence", required=True)
    n.set_defaults(fn=cmd_notarize)

    d = sub.add_parser("dmg")
    d.add_argument("--app", required=True)
    d.add_argument("--target", required=True, choices=sorted(TRIPLES))
    d.add_argument("--out", required=True)
    d.add_argument("--identity", default="")
    d.add_argument("--keychain", default="")
    d.add_argument("--notarize", action="store_true")
    d.add_argument("--evidence", default="")
    d.set_defaults(fn=cmd_dmg)

    r = sub.add_parser("archive")
    r.add_argument("--app", required=True)
    r.add_argument("--out", required=True)
    r.set_defaults(fn=cmd_archive)

    k = sub.add_parser("keychain")
    k.add_argument("action", choices=["create", "delete"])
    k.add_argument("--path", required=True)
    k.add_argument("--password", default="")
    k.set_defaults(fn=cmd_keychain)

    a = ap.parse_args()
    a.fn(a)


if __name__ == "__main__":
    main()
