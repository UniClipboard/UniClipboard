#!/usr/bin/env python3
"""Replay production BoardUI migration and existing companion scenarios once.

Only canonical Desktop cases execute. Every output directory must be new. This
wrapper never retries failed scenarios or turns software evidence into native
platform acceptance. It does not invoke the reference-library demo.
"""
import argparse
import datetime
import hashlib
import importlib.util
import json
from pathlib import Path
import platform
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[3]
# The canonical catalogue remains the only view-scenario inventory. A full
# production migration replay covers every isolated software view plus its
# real-service/build companions, without launching native-window cases.
_spec = importlib.util.spec_from_file_location("desktop_catalogue", ROOT / "app/desktop/e2e/run.py")
_catalogue = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_catalogue)
CASES = tuple(case["id"] for case in _catalogue.cases()
              if case["mode"] == "view" and case["environment"] == "isolated") + (
    "data-binding", "storage-cleanup-settings", "first-install-setup", "production-boundary",
)



def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def runtime_input(relative):
    """Select runtime/module/resource/harness inputs, excluding narrative docs."""
    path = Path(relative)
    if path.parts and path.parts[0] in ("docs", "prototype"):
        return False
    if relative.startswith("app/desktop/boardui/reference/"):
        return False
    if relative in ("go.mod", "go.sum", "go.work", "go.work.sum", "Makefile"):
        return True
    # Includes Go embed payloads, native resources and the exact declarative
    # acceptance inputs. README/AGENTS/Markdown edits must not invalidate a run.
    return path.suffix.lower() in (
        ".go", ".mod", ".sum", ".s", ".c", ".h", ".m", ".mm", ".syso",
        ".py", ".txtar", ".json", ".toml", ".lua", ".sql", ".svg", ".png",
        ".jpg", ".jpeg", ".ico", ".icns", ".ttf", ".otf", ".woff", ".woff2",
        ".txt", ".html", ".bin", ".dat", ".db",
    )


def runtime_hashes():
    names = subprocess.check_output(
        ["git", "ls-files", "-co", "--exclude-standard", "-z"], cwd=ROOT,
    ).split(b"\0")
    result = {}
    for name in sorted(set(names)):
        if not name:
            continue
        relative = name.decode("utf-8")
        if not runtime_input(relative):
            continue
        path = ROOT / relative
        if not path.exists():
            continue
        if not path.resolve().is_relative_to(ROOT) or not path.is_file():
            raise RuntimeError("unsupported runtime input link: " + relative)
        result[relative] = digest(path)
    return result


def hash_changes(expected, actual):
    return dict(
        added=sorted(set(actual) - set(expected)),
        removed=sorted(set(expected) - set(actual)),
        changed=sorted(path for path in set(expected) & set(actual) if expected[path] != actual[path]),
    )


def source_fingerprint(hashes):
    return hashlib.sha256(json.dumps(hashes, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, help="new artifact directory")
    parser.add_argument("--case", choices=CASES, help="run one case; companions remain NOT_RUN")
    args = parser.parse_args()
    identity = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:8]
    out = (args.out or ROOT / ".build/boardui-production" / identity).resolve()
    if out.exists():
        parser.error("output directory must be new; retain the initial failing run")
    out.mkdir(parents=True, mode=0o700)
    receipt = dict(version=1, run_id=identity, status="RUNNING", scope="production Desktop migration with canonical view/process companions", host=platform.platform(), native_platform_ui="NOT_RUN", system_clipboard="NOT_RUN", source_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(), dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)), cases={case: {"status": "NOT_RUN"} for case in CASES}, runner_sha256=digest(Path(__file__)))
    baseline = None
    try:
        # Failure modes: each case freezes a different revision; an embed asset
        # changes between cases; source changes during a case then only later
        # cases use it. Bind all runtime inputs before/after every case and to
        # each canonical frozen manifest. Documentation-only edits are ignored.
        baseline = runtime_hashes()
        receipt["runtime_sha256"] = baseline
        receipt["runtime_fingerprint_sha256"] = source_fingerprint(baseline)
        receipt["source_consistency"] = "PASS"
        for case in CASES:
            if args.case and case != args.case:
                continue
            before = runtime_hashes()
            if before != baseline:
                receipt["source_consistency"] = "FAIL"
                receipt["source_changes"] = hash_changes(baseline, before)
                raise RuntimeError("runtime source changed before " + case)
            command = [sys.executable, "app/desktop/e2e/run.py", "run", "--case", case, "--out", str(out / case)]
            print("RUN " + case, flush=True)
            started = time.monotonic()
            with (out / (case + ".log")).open("w", encoding="utf-8") as log:
                result = subprocess.run(command, cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, timeout=2800)
            manifest_path = out / case / "manifest.json"
            if manifest_path.is_file():
                manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
                case_result = manifest.get("cases", {}).get(case, {})
                frozen = {path: value for path, value in manifest.get("source_sha256", {}).items() if runtime_input(path)}
                binding_matches = frozen == baseline
                receipt["cases"][case] = dict(status="PASS" if result.returncode == 0 and manifest.get("status") == "PASS" and case_result.get("status") == "PASS" else "FAIL", elapsed_seconds=time.monotonic()-started, canonical_report=str((out / case / "index.html").relative_to(out)), canonical_manifest_sha256=digest(manifest_path), assertions=case_result.get("assertions", []), screenshots=case_result.get("screenshots", []))
                receipt["cases"][case]["runtime_fingerprint_sha256"] = source_fingerprint(frozen)
                receipt["cases"][case]["frozen_runtime_matches"] = binding_matches
                receipt["cases"][case]["canonical_source_head"] = manifest.get("source_head")
                if not binding_matches:
                    receipt["cases"][case]["status"] = "FAIL"
                    receipt["cases"][case]["failure"] = "canonical case froze different runtime inputs"
                    receipt["cases"][case]["source_changes"] = hash_changes(baseline, frozen)
                    receipt["source_consistency"] = "FAIL"
            else:
                receipt["cases"][case] = dict(status="FAIL", elapsed_seconds=time.monotonic()-started, failure="canonical runner emitted no manifest")
            after = runtime_hashes()
            receipt["cases"][case]["worktree_runtime_unchanged"] = after == baseline
            if after != baseline:
                receipt["cases"][case]["status"] = "FAIL"
                receipt["cases"][case]["failure"] = "runtime source changed during case"
                receipt["cases"][case]["source_changes"] = hash_changes(baseline, after)
                receipt["source_consistency"] = "FAIL"
            print(receipt["cases"][case]["status"] + " " + case, flush=True)
            if receipt["cases"][case]["status"] != "PASS":
                receipt["first_failure"] = case
                break
        selected = (args.case,) if args.case else CASES
        receipt["status"] = "PASS" if all(receipt["cases"][case]["status"] == "PASS" for case in selected) else "FAIL"
        if args.case:
            receipt["acceptance_boundary"] = "one selected scenario; full production migration requires all listed companions"
    except (Exception, KeyboardInterrupt) as error:
        receipt["status"] = "FAIL"
        receipt["failure"] = str(error) or "interrupted"
    finally:
        if baseline is not None:
            try:
                final_inputs = runtime_hashes()
                receipt["final_runtime_fingerprint_sha256"] = source_fingerprint(final_inputs)
                if final_inputs != baseline:
                    receipt["status"] = "FAIL"
                    receipt["source_consistency"] = "FAIL"
                    receipt["source_changes"] = hash_changes(baseline, final_inputs)
                    receipt.setdefault("failure", "runtime source changed before final publication")
            except Exception as error:
                receipt["status"] = "FAIL"
                receipt["source_consistency"] = "FAIL"
                receipt.setdefault("failure", "cannot verify final runtime source: " + str(error))
        (out / "migration-replay.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
        files = sorted(path for path in out.rglob("*") if path.is_file() and path.name != "SHA256SUMS")
        (out / "SHA256SUMS").write_text("".join(digest(path) + "  " + path.relative_to(out).as_posix() + "\n" for path in files), encoding="utf-8")
        print(receipt["status"] + ": " + str(out / "migration-replay.json"))
    return 0 if receipt["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
