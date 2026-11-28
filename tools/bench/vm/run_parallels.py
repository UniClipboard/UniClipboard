#!/usr/bin/env python3
"""Orchestrate Windows desktop resource benchmarks inside an existing Parallels VM.

Cross-compiles Track A (production) and Track B (desktop_e2e,bench_probe) binaries
on the macOS host, stages them onto Guest local NTFS storage via \\Mac\\Home, runs
`unic-bench.exe doctor` and `unic-bench.exe run` directly via `prlctl exec` without
spawning extra focus-stealing Guest console windows, collects reports, and strictly
restores Guest processes, temp directories, and initial VM state in `finally`.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import time
import uuid

PE_MACHINES = {
    0x8664: "amd64",
    0xAA64: "arm64",
    0x014C: "386",
}

HARVEST_ARTIFACTS = (
    "report.json",
    "report.md",
    "report.html",
    "SHA256SUMS",
)


class BenchmarkError(RuntimeError):
    """Raised when VM orchestration, binary verification, or cleanup fails."""


def find_repo_root(start: Path | None = None) -> Path:
    cur = (start or Path(__file__).resolve()).resolve()
    if cur.is_file():
        cur = cur.parent
    while True:
        if (cur / "go.mod").is_file():
            return cur
        if cur.parent == cur:
            raise BenchmarkError(f"could not locate go.mod from {start or __file__}")
        cur = cur.parent


def ps_quote(value: str) -> str:
    return "'" + str(value).replace("'", "''") + "'"


def encode_powershell(script: str) -> str:
    full = (
        "$ErrorActionPreference='Stop';"
        "$ProgressPreference='SilentlyContinue';"
        "[Console]::OutputEncoding=[Text.UTF8Encoding]::new();\n" + script
    )
    return base64.b64encode(full.encode("utf-16le")).decode("ascii")


def host_path_to_mac_share(path: Path, home_dir: Path | None = None) -> str:
    home = (home_dir or Path.home()).resolve()
    resolved = path.resolve()
    try:
        rel = resolved.relative_to(home)
    except ValueError as exc:
        raise BenchmarkError(
            f"staging path {resolved} must be inside host home directory {home} to map to \\\\Mac\\Home"
        ) from exc
    rel_win = "\\".join(rel.parts)
    return r"\\Mac\Home" if not rel_win else r"\\Mac\Home" + "\\" + rel_win


def parse_vm_state(info_output: str) -> str:
    match = re.search(r"^\s*State:\s*(\S+)", info_output, re.MULTILINE)
    if not match:
        raise BenchmarkError(f"unable to parse VM State from prlctl list -i output:\n{info_output}")
    return match.group(1).strip().lower()


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while chunk := f.read(1024 * 1024):
            h.update(chunk)
    return h.hexdigest()


def inspect_pe_arch(path: Path) -> str:
    with path.open("rb") as f:
        dos = f.read(64)
        if len(dos) < 64 or dos[:2] != b"MZ":
            raise BenchmarkError(f"{path} is not a valid PE file (missing MZ header)")
        pe_off = struct.unpack_from("<I", dos, 0x3C)[0]
        f.seek(pe_off)
        sig_and_coff = f.read(6)
        if len(sig_and_coff) < 6 or sig_and_coff[:4] != b"PE\x00\x00":
            raise BenchmarkError(f"{path} is not a valid PE file (missing PE signature)")
        machine = struct.unpack_from("<H", sig_and_coff, 4)[0]
    arch = PE_MACHINES.get(machine)
    if not arch:
        raise BenchmarkError(f"{path} has unsupported PE machine 0x{machine:04x}")
    return arch


def binary_specs() -> list[dict]:
    return [
        {
            "key": "unic",
            "pkg": "./app/service/cmd/unic",
            "relpath": Path("bin") / "prod" / "unic.exe",
            "tags": [],
            "gui": False,
        },
        {
            "key": "unicd",
            "pkg": "./app/service/cmd/unicd",
            "relpath": Path("bin") / "prod" / "unicd.exe",
            "tags": [],
            "gui": False,
        },
        {
            "key": "unic-desktop",
            "pkg": "./app/desktop/cmd/unic-desktop",
            "relpath": Path("bin") / "prod" / "unic-desktop.exe",
            "tags": [],
            "gui": True,
        },
        {
            "key": "unicd-bench-probe",
            "pkg": "./app/service/cmd/unicd",
            "relpath": Path("bin") / "e2e-probe" / "unicd.exe",
            "tags": ["bench_probe"],
            "gui": False,
        },
        {
            "key": "unic-desktop-e2e-probe",
            "pkg": "./app/desktop/cmd/unic-desktop",
            "relpath": Path("bin") / "e2e-probe" / "unic-desktop.exe",
            "tags": ["desktop_e2e", "bench_probe"],
            "gui": True,
        },
        {
            "key": "unic-bench",
            "pkg": "./tools/bench/cmd/unic-bench",
            "relpath": Path("bin") / "unic-bench.exe",
            "tags": [],
            "gui": False,
        },
    ]


def build_go_command(repo_root: Path, stage_dir: Path, arch: str, spec: dict) -> tuple[list[str], dict[str, str], Path]:
    out_path = stage_dir / spec["relpath"]
    ldflags = "-s -w"
    if spec["gui"]:
        ldflags += " -H windowsgui"
    cmd = ["go", "build", "-trimpath", "-buildvcs=false", f"-ldflags={ldflags}"]
    if spec["tags"]:
        cmd.extend(["-tags", ",".join(spec["tags"])])
    cmd.extend(["-o", str(out_path), spec["pkg"]])

    env = os.environ.copy()
    env["CGO_ENABLED"] = "0"
    env["GOOS"] = "windows"
    env["GOARCH"] = arch
    return cmd, env, out_path


def verify_staged_binaries(stage_dir: Path, expected_arch: str) -> dict[str, dict]:
    manifest: dict[str, dict] = {}
    for spec in binary_specs():
        p = stage_dir / spec["relpath"]
        if not p.is_file():
            raise BenchmarkError(f"missing staged binary: {p}")
        arch = inspect_pe_arch(p)
        if arch != expected_arch:
            raise BenchmarkError(f"{p} PE arch mismatch: got {arch}, want {expected_arch}")
        manifest[spec["key"]] = {
            "relpath": str(spec["relpath"]),
            "arch": arch,
            "sha256": sha256_file(p),
            "size_bytes": p.stat().st_size,
            "tags": list(spec["tags"]),
        }
    return manifest


def build_guest_unic_bench_args(
    guest_root: str,
    suite: str,
    track: str,
    dataset: str,
    profile: str,
    repetitions: int,
) -> list[str]:
    bench_exe = f"{guest_root}\\bin\\unic-bench.exe"
    guest_out = f"{guest_root}\\out"
    args = [
        bench_exe,
        "run",
        "--suite",
        suite,
        "--track",
        track,
        "--profile",
        profile,
        "--repetitions",
        str(repetitions),
        "--output",
        guest_out,
        "--unic",
        f"{guest_root}\\bin\\prod\\unic.exe",
        "--unicd",
        f"{guest_root}\\bin\\prod\\unicd.exe",
        "--desktop",
        f"{guest_root}\\bin\\prod\\unic-desktop.exe",
        "--unicd-probe",
        f"{guest_root}\\bin\\e2e-probe\\unicd.exe",
        "--desktop-e2e",
        f"{guest_root}\\bin\\e2e-probe\\unic-desktop.exe",
    ]
    if dataset.strip():
        args.extend(["--dataset", dataset.strip()])
    return args


def build_guest_cleanup_ps(guest_root: str) -> str:
    return (
        f"$root={ps_quote(guest_root)};"
        "$prefix=$root+'\\';"
        "$owned=@(Get-CimInstance Win32_Process | Where-Object {"
        "$_.ExecutablePath -and $_.ExecutablePath.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)"
        "});"
        "foreach($p in $owned){"
        "$proc=Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue;"
        "if($proc){Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue;$proc.WaitForExit(10000)|Out-Null}"
        "};"
        "if(Test-Path -LiteralPath $root){Remove-Item -LiteralPath $root -Recurse -Force}"
    )


def build_guest_harvest_ps(guest_out_dir: str, share_out: str) -> str:
    names_ps = ",".join(ps_quote(name) for name in HARVEST_ARTIFACTS)
    return (
        f"$outDir={ps_quote(guest_out_dir)};"
        f"$shareOut={ps_quote(share_out)};"
        f"foreach($name in @({names_ps}))"
        "{"
        "$src=[IO.Path]::Combine($outDir,$name);"
        "if(Test-Path -LiteralPath $src){Copy-Item -LiteralPath $src -Destination ([IO.Path]::Combine($shareOut,$name)) -Force}"
        "};"
        "$pm=[IO.Path]::Combine($outDir,'probe-metrics');"
        "if(Test-Path -LiteralPath $pm){Copy-Item -LiteralPath $pm -Destination ([IO.Path]::Combine($shareOut,'probe-metrics')) -Recurse -Force}"
    )


def restore_vm_commands(vm: str, initial_state: str) -> list[list[str]]:
    state = initial_state.strip().lower()
    if state == "suspended":
        return [["prlctl", "suspend", vm]]
    if state == "stopped":
        return [["prlctl", "stop", vm]]
    return []


def run_cmd(cmd: list[str], cwd: Path | None = None, env: dict[str, str] | None = None, check: bool = True) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, cwd=str(cwd) if cwd else None, env=env, text=True, capture_output=True, check=check)


def run_prlctl_ps(vm: str, script: str) -> subprocess.CompletedProcess:
    encoded = encode_powershell(script)
    cmd = [
        "prlctl",
        "exec",
        vm,
        "--current-user",
        "powershell.exe",
        "-NoProfile",
        "-NonInteractive",
        "-WindowStyle",
        "Hidden",
        "-ExecutionPolicy",
        "Bypass",
        "-EncodedCommand",
        encoded,
    ]
    return run_cmd(cmd)


def get_vm_state(vm: str) -> str:
    res = run_cmd(["prlctl", "list", "-i", vm])
    return parse_vm_state(res.stdout)


def wait_vm_state(vm: str, target_state: str, timeout_sec: float = 120.0) -> None:
    deadline = time.monotonic() + timeout_sec
    last_state = ""
    while time.monotonic() < deadline:
        last_state = get_vm_state(vm)
        if last_state == target_state:
            return
        time.sleep(1.5)
    raise BenchmarkError(f"VM {vm!r} did not reach state {target_state!r} within {timeout_sec}s (last={last_state!r})")


def wait_guest_exec_ready(vm: str, timeout_sec: float = 120.0) -> None:
    deadline = time.monotonic() + timeout_sec
    while time.monotonic() < deadline:
        proc = subprocess.run(
            ["prlctl", "exec", vm, "--current-user", "cmd.exe", "/c", "echo ready"],
            text=True,
            capture_output=True,
        )
        if proc.returncode == 0 and "ready" in proc.stdout:
            return
        time.sleep(2.0)
    raise BenchmarkError(f"VM {vm!r} guest tools/prlctl exec not ready within {timeout_sec}s")


def cross_compile_all(repo_root: Path, stage_dir: Path, arch: str) -> dict[str, dict]:
    for spec in binary_specs():
        cmd, env, out_path = build_go_command(repo_root, stage_dir, arch, spec)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        print(f"[win-vm] building windows/{arch} {spec['relpath']} ({','.join(spec['tags']) or 'prod'})", flush=True)
        res = run_cmd(cmd, cwd=repo_root, env=env, check=False)
        if res.returncode != 0:
            raise BenchmarkError(f"go build failed for {spec['relpath']}:\n{res.stdout}\n{res.stderr}")
    return verify_staged_binaries(stage_dir, arch)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run unic Windows desktop resource benchmarks inside Parallels VM")
    parser.add_argument("--vm", default="Windows 11", help="Parallels VM name or UUID (default: Windows 11)")
    parser.add_argument("--arch", default="arm64", choices=["arm64", "amd64"], help="Windows target GOARCH (default: arm64)")
    parser.add_argument("--suite", default="all", help="Comma-separated scenarios or 'all'")
    parser.add_argument("--track", default="both", choices=["production", "interactive", "both"], help="Benchmark track")
    parser.add_argument("--dataset", default="", help="Comma-separated dataset presets (default: profile default)")
    parser.add_argument("--profile", default="default", choices=["default", "soak"], help="Timing profile")
    parser.add_argument("--repetitions", type=int, default=3, help="Steady-state repetitions")
    parser.add_argument("--output", default="", help="Host artifact output directory (default: .build/bench/win-<arch>)")
    parser.add_argument("--repo-root", default="", help="Optional repository root path")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    repo_root = Path(args.repo_root).resolve() if args.repo_root else find_repo_root()
    output_dir = Path(args.output) if args.output else (repo_root / ".build" / "bench" / f"win-{args.arch}")
    if not output_dir.is_absolute():
        output_dir = (repo_root / output_dir).resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    run_id = f"{time.strftime('%Y%m%d-%H%M%S', time.gmtime())}-{uuid.uuid4().hex[:8]}"
    # Always stage inside $HOME/.cache so Parallels \\Mac\Home sharing can always see it.
    stage_dir = Path.home() / ".cache" / "unic-bench-vm-stage" / run_id
    stage_dir.mkdir(parents=True, exist_ok=True)

    initial_state = ""
    vm_state_changed = False
    guest_root = ""
    guest_created = False
    cleanup_errors: list[str] = []

    try:
        binary_manifest = cross_compile_all(repo_root, stage_dir, args.arch)
        (stage_dir / "build-manifest.json").write_text(json.dumps(binary_manifest, indent=2) + "\n", encoding="utf-8")

        initial_state = get_vm_state(args.vm)
        print(f"[win-vm] VM {args.vm!r} initial state: {initial_state}", flush=True)
        if initial_state == "suspended":
            run_cmd(["prlctl", "resume", args.vm])
            vm_state_changed = True
            wait_vm_state(args.vm, "running")
        elif initial_state == "stopped":
            run_cmd(["prlctl", "start", args.vm])
            vm_state_changed = True
            wait_vm_state(args.vm, "running")
        elif initial_state != "running":
            raise BenchmarkError(f"unsupported initial VM state {initial_state!r} for {args.vm!r}")

        wait_guest_exec_ready(args.vm)

        share_bin = host_path_to_mac_share(stage_dir / "bin")
        share_out_dir = stage_dir / "collected"
        share_out_dir.mkdir(parents=True, exist_ok=True)
        share_out = host_path_to_mac_share(share_out_dir)

        prep_ps = (
            "$tempRoot=[IO.Path]::Combine($env:LOCALAPPDATA,'Temp');"
            f"$guestRoot=[IO.Path]::Combine($tempRoot,{ps_quote('unic-bench-' + run_id)});"
            "if(Test-Path -LiteralPath $guestRoot){throw ('Guest collision: '+$guestRoot)};"
            "New-Item -ItemType Directory -Path $guestRoot -Force | Out-Null;"
            "$dstBin=[IO.Path]::Combine($guestRoot,'bin');"
            f"Copy-Item -LiteralPath {ps_quote(share_bin)} -Destination $dstBin -Recurse -Force;"
            "New-Item -ItemType Directory -Path ([IO.Path]::Combine($guestRoot,'out')) -Force | Out-Null;"
            "Write-Output ('GUEST_ROOT='+$guestRoot)"
        )
        prep_res = run_prlctl_ps(args.vm, prep_ps)
        for line in prep_res.stdout.splitlines():
            if line.startswith("GUEST_ROOT="):
                guest_root = line.split("=", 1)[1].strip()
        if not guest_root:
            raise BenchmarkError(f"failed to resolve GUEST_ROOT from VM prep:\n{prep_res.stdout}\n{prep_res.stderr}")
        guest_created = True
        print(f"[win-vm] staged Guest NTFS root: {guest_root}", flush=True)

        # Run unic-bench doctor directly (pure Go UTF-8, zero extra console window).
        doc_res = subprocess.run(
            ["prlctl", "exec", args.vm, "--current-user", f"{guest_root}\\bin\\unic-bench.exe", "doctor", "--json"],
            text=True,
            capture_output=True,
        )
        (output_dir / "doctor.json").write_text(doc_res.stdout, encoding="utf-8")
        if doc_res.returncode != 0:
            raise BenchmarkError(f"unic-bench.exe doctor failed (exit {doc_res.returncode}):\n{doc_res.stdout}\n{doc_res.stderr}")

        bench_args = build_guest_unic_bench_args(
            guest_root=guest_root,
            suite=args.suite,
            track=args.track,
            dataset=args.dataset,
            profile=args.profile,
            repetitions=args.repetitions,
        )
        print(f"[win-vm] running inside Guest: {' '.join(bench_args)}", flush=True)
        run_proc = subprocess.run(
            ["prlctl", "exec", args.vm, "--current-user", *bench_args],
            text=True,
        )

        # Always harvest any artifacts produced before cleanup, even on scenario failure.
        harvest_ps = build_guest_harvest_ps(guest_root + "\\out", share_out)
        run_prlctl_ps(args.vm, harvest_ps)

        for item in HARVEST_ARTIFACTS:
            src = share_out_dir / item
            if src.is_file():
                shutil.copy2(src, output_dir / item)
        src_pm = share_out_dir / "probe-metrics"
        if src_pm.is_dir():
            dst_pm = output_dir / "probe-metrics"
            if dst_pm.exists():
                shutil.rmtree(dst_pm)
            shutil.copytree(src_pm, dst_pm)

        (output_dir / "build-manifest.json").write_text(json.dumps(binary_manifest, indent=2) + "\n", encoding="utf-8")

        if run_proc.returncode != 0:
            print(f"[win-vm] unic-bench.exe run exited with code {run_proc.returncode}", file=sys.stderr)
            return run_proc.returncode
        if not (output_dir / "report.json").is_file():
            raise BenchmarkError("unic-bench.exe completed cleanly but report.json was not harvested")
        print(f"[win-vm] collected Windows benchmark artifacts -> {output_dir}", flush=True)
        return 0
    finally:
        if guest_created and guest_root:
            try:
                run_prlctl_ps(args.vm, build_guest_cleanup_ps(guest_root))
            except Exception as exc:
                cleanup_errors.append(f"guest cleanup failed: {exc}")

        if vm_state_changed and initial_state in ("suspended", "stopped"):
            try:
                for cmd in restore_vm_commands(args.vm, initial_state):
                    run_cmd(cmd)
                wait_vm_state(args.vm, initial_state, timeout_sec=120.0)
            except Exception as exc:
                cleanup_errors.append(f"VM restore to {initial_state!r} failed: {exc}")

        try:
            shutil.rmtree(stage_dir, ignore_errors=True)
        except Exception as exc:
            cleanup_errors.append(f"host stage cleanup failed: {exc}")

        if cleanup_errors:
            print("[win-vm] cleanup errors: " + "; ".join(cleanup_errors), file=sys.stderr)


if __name__ == "__main__":
    raise SystemExit(main())
