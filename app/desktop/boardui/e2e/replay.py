#!/usr/bin/env python3
"""Replay the isolated BoardUI reference and canonical native software-view cases.

The output directory must be new. Failures and the first failing artifacts stay
in that directory. No retries, production application, or system clipboard.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import sys
import time
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[4]
PACKAGE = ROOT / "app/desktop/boardui"


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path: Path, value) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def cpu_seconds(value: str) -> float:
    days = 0
    if "-" in value:
        prefix, value = value.split("-", 1)
        days = int(prefix)
    result = 0.0
    for component in value.split(":"):
        result = result * 60 + float(component)
    return days * 86400 + result


def owned_test_processes(parent: int) -> list[dict]:
    """Read only descendants into retained metrics; never retain other commands."""
    if os.name != "posix":
        return []
    try:
        raw = subprocess.check_output(
            ["ps", "-ww", "-axo", "pid=,ppid=,rss=,time=,command="], text=True,
            stderr=subprocess.DEVNULL, timeout=5,
        )
    except (OSError, subprocess.SubprocessError):
        return []
    rows = {}
    for line in raw.splitlines():
        parts = line.split(None, 4)
        if len(parts) != 5:
            continue
        try:
            pid, ppid, rss = map(int, parts[:3])
            rows[pid] = (ppid, rss * 1024, cpu_seconds(parts[3]), parts[4])
        except ValueError:
            continue
    owned = {parent}
    while True:
        added = {pid for pid, row in rows.items() if row[0] in owned} - owned
        if not added:
            break
        owned.update(added)
    result = []
    for pid in sorted(owned):
        if pid not in rows:
            continue
        _, rss, cpu, command = rows[pid]
        # Exclude compilers, go commands and the outer testscript driver. The
        # measured process is the actual BoardUI TestDesktopAcceptance binary.
        if ".test" in command and "TestDesktopAcceptance" in command and "boardui-" in command:
            result.append(dict(pid=pid, rss_bytes=rss, cpu_seconds=cpu))
    return result


def stop_owned(process: subprocess.Popen) -> None:
    if process.poll() is not None:
        return
    if os.name == "posix":
        os.killpg(process.pid, signal.SIGTERM)
    else:
        process.terminate()
    try:
        process.wait(timeout=15)
    except subprocess.TimeoutExpired:
        if os.name == "posix":
            os.killpg(process.pid, signal.SIGKILL)
        else:
            process.kill()
        process.wait(timeout=10)


def execute(name: str, command: list[str], out: Path, manifest: dict,
            *, cwd: Path = ROOT, sample: bool = False, timeout: int = 1800, environment: dict | None = None) -> None:
    print("RUN " + name, flush=True)
    started = time.monotonic()
    step = dict(name=name, command=command, status="RUNNING", samples=[])
    manifest["steps"].append(step)
    process = None
    try:
        with (out / (name + ".log")).open("w", encoding="utf-8") as log:
            process = subprocess.Popen(command, cwd=cwd, env=environment, stdout=log, stderr=subprocess.STDOUT,
                                       start_new_session=os.name == "posix")
            while process.poll() is None:
                if time.monotonic() - started > timeout:
                    raise TimeoutError(name + " exceeded its time limit")
                if sample:
                    for item in owned_test_processes(process.pid):
                        step["samples"].append(dict(elapsed_seconds=time.monotonic() - started, **item))
                time.sleep(0.1)
            step["returncode"] = process.returncode
            if process.returncode:
                raise RuntimeError(name + " failed; inspect " + name + ".log")
        step["status"] = "PASS"
    except BaseException as error:
        step["status"] = "FAIL"
        step["failure"] = str(error)
        raise
    finally:
        if process is not None:
            stop_owned(process)
        step["elapsed_seconds"] = time.monotonic() - started
        samples = step.pop("samples")
        if sample:
            step["process_resource_scope"] = "sampled software-view Go E2E test process; race-enabled canonical run; excludes build, reference browser and production roles"
            step["process_resource_status"] = "MEASURED" if samples else "NOT_RUN"
            step["sample_count"] = len(samples)
            if samples:
                step["peak_sampled_rss_bytes"] = max(s["rss_bytes"] for s in samples)
                step["max_sampled_cpu_seconds"] = max(s["cpu_seconds"] for s in samples)
                write_json(out / (name + "-resources.json"), dict(scope=step["process_resource_scope"], samples=samples))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, help="new artifact directory under ignored .build by default")
    parser.add_argument("--node", type=Path, help="explicit Node >=22.12 executable for the reference tooling")
    parser.add_argument("--port", type=int, default=4177)
    parser.add_argument("--reference-space", type=int, default=0, help="owned TaskSpace ID retained by a prior failed reference capture")
    parser.add_argument("--native-only", action="store_true", help="skip reference capture; visual comparison remains NOT_RUN")
    args = parser.parse_args()
    run_id = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:8]
    out = (args.out or ROOT / ".build/boardui-e2e" / run_id).resolve()
    if out.exists():
        parser.error("output directory must be new; use a new directory to retain first failures")
    out.mkdir(parents=True, mode=0o700)
    manifest = dict(version=1, run_id=run_id, status="RUNNING", steps=[],
                    scope="official React reference versus mygo software view",
                    native_window="NOT_RUN", os_keyboard_ime_accessibility="NOT_RUN",
                    windows_native_memory="NOT_RUN", system_clipboard="NOT_RUN",
                    visual_comparison="NOT_RUN", host=platform.platform(), python=sys.version,
                    source_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                    dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
                    go=subprocess.check_output(["go", "version"], cwd=ROOT, text=True).strip())
    source_paths = [p for p in PACKAGE.rglob("*") if p.is_file() and "__pycache__" not in p.parts]
    source_paths += [ROOT / p for p in ("go.mod", "go.sum", "app/desktop/e2e/run.py", "app/desktop/e2e/scenario_test.go", "app/desktop/cmd/boardui-demo/main.go")]
    manifest["source_sha256"] = {str(p.relative_to(ROOT)): sha(p) for p in sorted(source_paths)}
    manifest["font_sha256"] = {str(p.relative_to(ROOT)): sha(p) for p in (PACKAGE / "assets").rglob("*") if p.suffix.lower() in (".ttf", ".otf", ".woff", ".woff2")}
    vite = None
    vite_log = None
    try:
        asset_lock = json.loads((PACKAGE / "assets/source-lock.json").read_text())
        checks = {PACKAGE / "assets/Inter.ttf": asset_lock["font"]["sha256"],
                  PACKAGE / "reference/package-lock.json": asset_lock["compiler"]["lock_sha256"]}
        checks.update({PACKAGE / "assets" / name: value for name, value in asset_lock["notices"].items()})
        checks.update({PACKAGE / "reference" / name: value for name, value in asset_lock["fixture"]["files"].items()})
        for path, expected in checks.items():
            if sha(path) != expected:
                raise RuntimeError("locked input identity mismatch: " + str(path.relative_to(ROOT)))
        manifest["asset_lock_verified"] = True
        manifest["fixture"] = dict(locale="en-US", themes=["light", "dark"], scales=[1, 2],
                                    viewport_dip=[800, 760], narrow_viewport_dip=[360, 720])
        if not args.native_only:
            node_file = ROOT / ".build/boardui-reference/node-path.txt"
            node = args.node or (Path(node_file.read_text().strip()) if node_file.is_file() else Path(shutil.which("node") or ""))
            if not node.is_file():
                raise RuntimeError("provide --node with a Node >=22.12 executable")
            node = node.resolve()
            version = subprocess.check_output([str(node), "--version"], text=True).strip()
            components = tuple(int(v) for v in version.lstrip("v").split(".")[:2])
            if components < (22, 12):
                raise RuntimeError("reference requires Node >=22.12; provide --node; selected " + version)
            manifest["node"] = dict(path=str(node), version=version, sha256=sha(node))
            reference_environment = dict(os.environ, PATH=str(node.parent) + os.pathsep + os.environ.get("PATH", ""))
            execute("reference-prepare", [sys.executable, str(PACKAGE / "reference/prepare.py")], out, manifest, environment=reference_environment)
            reference_dir = ROOT / ".build/boardui-reference"
            vite_log = (out / "reference-server.log").open("w", encoding="utf-8")
            vite = subprocess.Popen([str(node), str(reference_dir / "node_modules/vite/bin/vite.js"),
                                     "--host", "127.0.0.1", "--port", str(args.port), "--strictPort"],
                                    cwd=reference_dir, stdout=vite_log, stderr=subprocess.STDOUT,
                                    start_new_session=os.name == "posix")
            ready = False
            for _ in range(100):
                if vite.poll() is not None:
                    raise RuntimeError("reference server exited; inspect reference-server.log")
                try:
                    with urllib.request.urlopen("http://127.0.0.1:" + str(args.port), timeout=1) as response:
                        ready = response.status == 200
                except OSError:
                    pass
                if ready:
                    break
                time.sleep(0.1)
            if not ready:
                raise RuntimeError("reference server did not become ready within ten seconds")
            execute("reference-capture", [sys.executable, str(PACKAGE / "reference/capture.py"),
                                         "--out", str(out / "reference"), "--port", str(args.port), "--space", str(args.reference_space)], out, manifest)
        for case in ("boardui-settings", "boardui-states"):
            execute(case, [sys.executable, "app/desktop/e2e/run.py", "run", "--case", case,
                           "--out", str(out / case)], out, manifest, sample=True)
        comparison = PACKAGE / "e2e/compare.py"
        if not args.native_only and comparison.is_file():
            execute("visual-compare", [sys.executable, str(comparison), "--reference", str(out / "reference"),
                                       "--native", str(out),
                                       "--out", str(out / "visual")], out, manifest)
            manifest["visual_comparison"] = "see comparison output and per-checkpoint verdicts"
        else:
            manifest["visual_comparison_reason"] = "native-only run" if args.native_only else "comparison script absent; no visual acceptance claim"
        if not args.native_only and manifest["visual_comparison"] == "NOT_RUN":
            raise RuntimeError("required visual comparison did not run")
        manifest["status"] = "PASS"
    except BaseException as error:
        manifest["status"] = "FAIL"
        manifest["failure"] = str(error)
    finally:
        if vite is not None:
            stop_owned(vite)
        if vite_log is not None:
            vite_log.close()
        manifest["owned_reference_server_cleanup"] = "PASS" if vite is None or vite.poll() is not None else "FAIL"
        manifest["source_unchanged"] = all((ROOT / p).is_file() and sha(ROOT / p) == value for p, value in manifest["source_sha256"].items())
        if not manifest["source_unchanged"]:
            manifest["status"] = "FAIL"
            manifest["failure"] = "BoardUI source changed during replay"
        write_json(out / "replay.json", manifest)
        files = sorted(p for p in out.rglob("*") if p.is_file() and p.name != "SHA256SUMS")
        (out / "SHA256SUMS").write_text("".join(sha(p) + "  " + str(p.relative_to(out)) + "\n" for p in files), encoding="utf-8")
        print(manifest["status"] + ": " + str(out / "replay.json"))
    return 0 if manifest["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
