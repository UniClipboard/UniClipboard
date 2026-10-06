#!/usr/bin/env python3
"""End-to-end check of the update-manifest generator's Linux architecture handling (slice 17c3).

Real generator (scripts/assemble-update-manifest.js, as a subprocess) -> real consumer (internal/update through
e2e/manifestprobe, DefaultTargets from the real GOARCH in linux/arm64 and linux/amd64 containers) -> real
registration script. Inputs are FIXTURES: real v1.1.1 release asset basenames, synthetic payloads, real minisign
signatures from a throwaway key. Nothing here touches a feed, a release or a production asset.

  e2e/update_manifest_run.py --generator <path> --baseline <path> --out <dir> [--no-containers]

The same assertions run against the unfixed generator (red) and the fixed one (green).
"""
import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
GUI = REPO / "apps/gui-go"
V = "1.1.1"
PORT = "18443"
BASE_URL = f"http://127.0.0.1:{PORT}/dl"

LIN_AMD = f"UniClipboard_{V}_amd64.AppImage"
LIN_ARM = f"UniClipboard_{V}_aarch64.AppImage"
MAC_A = "UniClipboard_aarch64-apple-darwin.app.tar.gz"
MAC_X = "UniClipboard_x86_64-apple-darwin.app.tar.gz"
WIN_X = f"UniClipboard_{V}_x64-setup.exe"
WIN_A = f"UniClipboard_{V}_arm64-setup.exe"
PKG = [f"UniClipboard_{V}_amd64.deb", f"UniClipboard_{V}_arm64.deb",
       f"UniClipboard-{V}-1.x86_64.rpm", f"UniClipboard-{V}-1.aarch64.rpm"]
NON_LINUX = [MAC_A, MAC_X, WIN_X, WIN_A] + PKG

# name -> (relpaths, expected {platform key: basename}, consume?)
SCENARIOS = {
    "S1_real_names_flat": ([LIN_ARM, LIN_AMD] + NON_LINUX,
                           {"linux-aarch64": LIN_ARM, "linux-x86_64": LIN_AMD}, True),
    "S2a_nested_amd_first": ([f"a/{LIN_AMD}", f"z/{LIN_ARM}"],
                             {"linux-aarch64": LIN_ARM, "linux-x86_64": LIN_AMD}, True),
    "S2b_nested_arm_first": ([f"a/{LIN_ARM}", f"z/{LIN_AMD}"],
                             {"linux-aarch64": LIN_ARM, "linux-x86_64": LIN_AMD}, True),
    "S2c_reverse_creation": ([LIN_AMD, LIN_ARM],
                             {"linux-aarch64": LIN_ARM, "linux-x86_64": LIN_AMD}, True),
    "S3a_arm_targz_amd_bare": ([LIN_ARM + ".tar.gz", LIN_AMD],
                               {"linux-aarch64": LIN_ARM + ".tar.gz", "linux-x86_64": LIN_AMD}, True),
    "S3b_amd_targz_arm_bare": ([LIN_AMD + ".tar.gz", LIN_ARM],
                               {"linux-aarch64": LIN_ARM, "linux-x86_64": LIN_AMD + ".tar.gz"}, True),
    "S3c_both_forms_each": ([LIN_ARM, LIN_ARM + ".tar.gz", LIN_AMD, LIN_AMD + ".tar.gz"],
                            {"linux-aarch64": LIN_ARM + ".tar.gz", "linux-x86_64": LIN_AMD + ".tar.gz"}, True),
    "S4_only_aarch64": ([LIN_ARM], {"linux-aarch64": LIN_ARM}, True),
    "S5_unknown_arch": ([LIN_AMD, f"UniClipboard_{V}_armv7.AppImage", f"UniClipboard_{V}_i686.AppImage",
                         f"UniClipboard_{V}.AppImage", f"UniClipboard_{V}_xx64.AppImage"],
                        {"linux-x86_64": LIN_AMD}, True),
    "S5b_misleading_dirs": ([f"ubuntu-22.04-arm/{LIN_AMD}", f"x86_64/{LIN_ARM}", f"arm64/UniClipboard_{V}.AppImage"],
                            {"linux-aarch64": LIN_ARM, "linux-x86_64": LIN_AMD}, True),
    "S6_duplicate_amd64": ([f"x/{LIN_AMD}", f"y/{LIN_AMD}"], "REJECT", False),
}

results = []  # (scenario, name, ok, detail)


def expect(scn, name, cond, detail=""):
    results.append({"scenario": scn, "assertion": name, "ok": bool(cond), "detail": detail})
    print(("PASS " if cond else "FAIL ") + f"[{scn}] {name}" + (f" -- {detail}" if detail and not cond else ""))


def sh(cmd, log, **kw):
    p = subprocess.run(cmd, capture_output=True, text=True, **kw)
    Path(str(log) + ".cmd").write_text(" ".join(map(str, cmd)) + "\n")
    Path(str(log) + ".stdout").write_text(p.stdout)
    Path(str(log) + ".stderr").write_text(p.stderr)
    Path(str(log) + ".rc").write_text(str(p.returncode) + "\n")
    return p


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def generate(gen, assets, out_manifest, log, notes):
    return sh(["node", gen, "--version", V, "--artifacts-dir", str(assets), "--output", str(out_manifest),
               "--base-url", BASE_URL, "--notes-file", str(notes / f"{V}.md"),
               "--zh-notes-file", str(notes / f"{V}.zh.md")], log, cwd=REPO)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--generator", required=True)
    ap.add_argument("--baseline", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--no-containers", action="store_true")
    a = ap.parse_args()
    out = Path(a.out).resolve()
    gen, base = str(Path(a.generator).resolve()), str(Path(a.baseline).resolve())
    out.mkdir(parents=True, exist_ok=False)
    (out / "bin").mkdir()
    notes = out / "notes"
    notes.mkdir()
    (notes / f"{V}.md").write_text("## 1.1.1\n\n- FIXTURE notes\n")
    (notes / f"{V}.zh.md").write_text("## 1.1.1\n\n- FIXTURE 说明\n")
    (out / "generator.sha256").write_text(f"{sha(gen)}  generator\n{sha(base)}  baseline\n")

    targets = [("host", "darwin", platform_goarch())]
    if not a.no_containers:
        targets += [("linux-arm64", "linux", "arm64"), ("linux-amd64", "linux", "amd64")]
    for _, goos, goarch in targets:
        env = dict(os.environ, CGO_ENABLED="0", GOOS=goos, GOARCH=goarch)
        r = subprocess.run(["go", "build", "-o", str(out / "bin" / f"probe-{goos}-{goarch}"), "./e2e/manifestprobe"],
                           cwd=GUI, env=env, capture_output=True, text=True)
        if r.returncode:
            sys.exit("probe build failed: " + r.stderr)
    host_probe = out / "bin" / f"probe-darwin-{platform_goarch()}"

    for scn, (rels, expected, consume) in SCENARIOS.items():
        d = out / "scenarios" / scn
        assets = d / "assets"
        assets.mkdir(parents=True)
        r = subprocess.run([str(host_probe), "fixture", str(assets), str(d / "pubkey.b64"), *rels],
                           capture_output=True, text=True)
        if r.returncode:
            sys.exit("fixture failed: " + r.stderr)
        manifest = d / "manifest.json"
        p = generate(gen, assets, manifest, d / "generate", notes)
        if expected == "REJECT":
            expect(scn, "duplicate equal-priority candidates: generator exits non-zero", p.returncode != 0,
                   f"rc={p.returncode}")
            expect(scn, "no manifest written on rejection", not manifest.exists())
            expect(scn, "stderr names both conflicting files and says conflict",
                   p.returncode != 0 and "conflict" in p.stderr.lower() and f"x/{LIN_AMD}" in p.stderr
                   and f"y/{LIN_AMD}" in p.stderr, p.stderr[-300:])
            continue
        if p.returncode != 0:
            expect(scn, "generator succeeds", False, p.stderr[-300:])
            continue
        m = json.loads(manifest.read_text())
        linux = {k: v for k, v in m["platforms"].items() if k.startswith("linux-")}
        expect(scn, "linux platform keys are exactly the expected set", set(linux) == set(expected),
               f"got {sorted(linux)} want {sorted(expected)}")
        for key, fname in expected.items():
            entry = m["platforms"].get(key)
            if not entry:
                continue
            want_sig = next((assets / r_ for r_ in rels if Path(r_).name == fname), None)
            sig_text = (Path(str(want_sig) + ".sig").read_text().strip()) if want_sig else None
            expect(scn, f"{key}: url is this architecture's file", entry["url"] == f"{BASE_URL}/{fname}",
                   entry["url"])
            expect(scn, f"{key}: signature is this architecture's .sig bytes", entry["signature"] == sig_text)
        if scn.startswith("S5"):
            for skipped in [r_ for r_ in rels if Path(r_).name not in expected.values()]:
                expect(scn, f"unrecognized AppImage skipped with a warning: {skipped}",
                       f"Skipping unrecognized .sig file: {skipped}.sig" in p.stderr, p.stderr[-200:])

        if consume:
            for tname, goos, goarch in targets:
                if goos != "linux":
                    continue
                want_key = "linux-aarch64" if goarch == "arm64" else "linux-x86_64"
                res = run_consumer(out, d, tname, goarch, "")
                Path(d / f"consume-{tname}.json").write_text(json.dumps(res, indent=2))
                fname = expected.get(want_key)
                expect(scn, f"{tname}: DefaultTargets resolves {want_key}", res.get("Targets") == [want_key],
                       str(res.get("Targets")))
                if fname:
                    want = sha(next(assets / r_ for r_ in rels if Path(r_).name == fname))
                    expect(scn, f"{tname}: real client downloads and verifies this architecture's payload",
                           res.get("PayloadSHA256") == want and not res.get("Error"),
                           f"{res.get('PayloadSHA256')} err={res.get('Error')}")
                    expect(scn, f"{tname}: other platforms' signatures are rejected for this payload",
                           "ACCEPTED" not in res.get("CrossVerify", {}).values()
                           and (len(m["platforms"]) < 2 or bool(res.get("CrossVerify"))),
                           str(res.get("CrossVerify")))
                else:
                    expect(scn, f"{tname}: no artifact for an absent architecture (never a wrong-arch payload)",
                           "no artifact" in (res.get("Error") or "") and not res.get("PayloadSHA256"), str(res))

        if scn in ("S1_real_names_flat", "S3a_arm_targz_amd_bare"):
            reg = d / "registration.json"
            r = sh(["node", "scripts/build-flare-release-registration.js", "--version", V, "--channel", "stable",
                    "--manifest", str(manifest), "--artifacts-dir", str(assets), "--source", "fixture-e2e",
                    "--output", str(reg)], d / "registration", cwd=REPO)
            if r.returncode:
                expect(scn, "registration script accepts the manifest", False, r.stderr[-300:])
                continue
            arts = {x["platform"]: x for x in json.loads(reg.read_text())["artifacts"]}
            for key, fname in expected.items():
                x = arts.get(key)
                src = next(assets / r_ for r_ in rels if Path(r_).name == fname)
                expect(scn, f"registration {key}: filename/size/sha256 of this architecture's payload",
                       bool(x) and x["filename"] == fname and x["size"] == src.stat().st_size and x["sha256"] == sha(src),
                       str(x))

    compat(out, gen, base, notes, host_probe)

    (out / "assertions.json").write_text(json.dumps(results, indent=2))
    failed = [r for r in results if not r["ok"]]
    print(f"\n{len(results) - len(failed)}/{len(results)} assertions passed")
    (out / "SUMMARY.txt").write_text(f"{len(results) - len(failed)}/{len(results)} passed\n")
    sys.exit(1 if failed else 0)


def compat(out, gen, base, notes, host_probe):
    """macOS/Windows entries must be byte-identical to the baseline generator, in the same key order."""
    scn = "S7_compat_non_linux"
    d = out / "scenarios" / "S1_real_names_flat"
    assets = d / "assets"
    mb = out / "scenarios" / scn
    mb.mkdir(parents=True)
    generate(base, assets, mb / "baseline.json", mb / "baseline", notes)
    generate(gen, assets, mb / "current.json", mb / "current", notes)
    b, c = json.loads((mb / "baseline.json").read_text()), json.loads((mb / "current.json").read_text())
    def nl(m):
        return [(k, v) for k, v in m["platforms"].items() if not k.startswith("linux-")]

    expect(scn, "non-Linux platform entries and their order are identical to the baseline", nl(b) == nl(c),
           f"{[k for k, _ in nl(b)]} vs {[k for k, _ in nl(c)]}")
    expect(scn, "version and notes identical to the baseline",
           (b["version"], b["notes"]) == (c["version"], c["notes"]))
    expect(scn, "non-Linux keys include the macOS and Windows keys",
           {k for k, _ in nl(c)} == {"darwin-aarch64", "darwin-x86_64", "windows-aarch64", "windows-x86_64"})
    res = subprocess.run([str(host_probe), "consume", str(mb / "current.json"), str(assets),
                          str(d / "pubkey.b64"), PORT, "app"], capture_output=True, text=True)
    r = json.loads(res.stdout) if res.stdout.strip() else {}
    (mb / "consume-host-darwin.json").write_text(json.dumps(r, indent=2))
    want = sha(assets / MAC_A) if platform_goarch() == "arm64" else sha(assets / MAC_X)
    expect(scn, "host darwin consumer (real DefaultTargets, installer app) downloads and verifies its payload",
           r.get("PayloadSHA256") == want and not r.get("Error"), str(r))


def run_consumer(out, d, tname, goarch, installer):
    rel = d.relative_to(out)
    cmd = ["docker", "run", "--rm", "--network", "none", "--platform", f"linux/{goarch}",
           "-v", f"{out}:/work:ro", "ubuntu:24.04", f"/work/bin/probe-linux-{goarch}", "consume",
           f"/work/{rel}/manifest.json", f"/work/{rel}/assets", f"/work/{rel}/pubkey.b64", PORT, installer]
    p = sh(cmd, d / f"consume-{tname}-run")
    try:
        return json.loads(p.stdout)
    except Exception:
        return {"Error": "probe produced no JSON: " + p.stderr[-300:]}


def platform_goarch():
    return "arm64" if subprocess.check_output(["uname", "-m"], text=True).strip() == "arm64" else "amd64"


if __name__ == "__main__":
    main()
