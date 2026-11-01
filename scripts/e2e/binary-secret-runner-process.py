#!/usr/bin/env python3
"""Real Cargo/process regression for the runner; no bus, wallet or secret access."""
import hashlib
import importlib.util
import json
import os
import pathlib
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parents[2]
RUNNER = REPO / "scripts/e2e/linux-binary-secret-storage.py"


def verify_cleanup(result, scratch):
    assert result["cleanup"]["group_gone"] and result["cleanup"]["directory_removed"]
    assert not scratch.exists()
    group = result["cleanup"].get("process_group")
    if group is not None:
        try:
            os.killpg(group, 0)
        except ProcessLookupError:
            pass
        else:
            raise AssertionError("process group survived cleanup")


def worker(mode, root):
    spec = importlib.util.spec_from_file_location("runner", RUNNER)
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    scratch = root / "scratch"
    scratch.mkdir()
    (scratch / "synthetic-wallet-marker").touch()
    if mode == "custom-target":
        fixture = root / "fixture"
        (fixture / "examples").mkdir(parents=True)
        (fixture / "Cargo.toml").write_text('[package]\nname="uc-platform"\nversion="0.0.0"\nedition="2021"\n[workspace]\n')
        (fixture / "examples/binary_secret_storage.rs").write_text('fn main() { println!("current executable"); }\n')
        subprocess.run(["cargo", "generate-lockfile", "--offline"], cwd=fixture, check=True, capture_output=True)
        stale = fixture / "target/debug/examples/binary_secret_storage"
        stale.parent.mkdir(parents=True)
        stale.write_text('#!/bin/sh\necho stale executable\n')
        stale.chmod(0o700)
        target = (root / "custom target with spaces").resolve()
        os.environ["CARGO_TARGET_DIR"] = str(target)
        runner.REPO = fixture
        binary, build = runner.build_example(root)
        assert build["exit_code"] == 0 and binary.is_relative_to(target)
        digest = hashlib.sha256(binary.read_bytes()).hexdigest()
        (scratch / "secure-storage-source.json").write_text('{"version":1,"store":"system"}')
        env = {k: v for k, v in os.environ.items() if not k.startswith(("DBUS_", "GNOME_KEYRING_"))}
        env.update(UC_BINARY_SECRET_E2E_BINARY=str(binary), UC_BINARY_SECRET_E2E_ROOT=str(scratch))
        result = runner.run_private_session([sys.executable, str(RUNNER), "--session", "unavailable"], env, scratch, timeout=5)
        cases = json.loads(result["stdout"])
        assert all(c["status"] == "PASSED" for c in cases)
        assert all(c["command"][0] == str(binary) for c in cases if "command" in c)
        verify_cleanup(result, scratch)
        assert digest == build["binary_sha256"]
        assert stale.read_text().endswith('echo stale executable\n')
        return {"build": build, "session": result, "same_executable_hashed_and_executed": True}

    child = "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(60)"
    if mode == "spawn-failure":
        result = runner.run_private_session([str(root / "missing executable")], dict(os.environ), scratch, timeout=1)
        assert result["reason"] == "FileNotFoundError" and result["cleanup"]["directory_removed"]
        verify_cleanup(result, scratch)
        return result
    if mode == "kill-escalation":
        program = "import pathlib,signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); pathlib.Path('ready').touch(); time.sleep(60)"
    else:
        # The parent reaps the child and proves the directory still exists at cleanup.
        program = (
            "import pathlib,subprocess,sys,signal,time\n"
            f"child=subprocess.Popen([sys.executable,'-c',{child!r}])\n"
            "def stop(*_):\n"
            " child.kill(); child.wait(); pathlib.Path('../cleanup-order.json').write_text(str(pathlib.Path('synthetic-wallet-marker').exists())); sys.exit(0)\n"
            "signal.signal(signal.SIGTERM,stop)\n"
            "pathlib.Path('ready').touch()\n"
            + ("sys.stdout.flush(); time.sleep(60)\n" if mode == "timeout-tree" else "raise RuntimeError('synthetic session failure')\n")
        )
        if mode == "error-tree":
            # Exercise a failed command that has reaped its descendant.
            program = program.replace("raise RuntimeError('synthetic session failure')", "stop()")
            program = program.replace("sys.exit(0)", "sys.exit(1)")
    command = [sys.executable, "-c", "import os; os.chdir(" + repr(str(scratch)) + ");\n" + program]
    result = runner.run_private_session(command, dict(os.environ), scratch, timeout=1)
    assert result["cleanup"]["group_gone"] and result["cleanup"]["leader_reaped"]
    verify_cleanup(result, scratch)
    if mode in ("timeout-tree", "kill-escalation"):
        assert result["timed_out"]
    if mode == "kill-escalation":
        assert result["cleanup"]["sigkill_sent"]
    if mode == "error-tree":
        assert result["exit_code"] == 1 and not result["timed_out"]
    if mode != "kill-escalation":
        assert (root / "cleanup-order.json").read_text() == "True"
    return result


def main():
    output = pathlib.Path(os.environ.get("UC_RUNNER_PROCESS_ARTIFACTS", REPO / "target/binary-secret-runner-process"))
    output.mkdir(parents=True, exist_ok=True)
    results = []
    for mode in ("custom-target", "timeout-tree", "kill-escalation", "error-tree", "spawn-failure"):
        with tempfile.TemporaryDirectory(prefix="uc-runner-process-") as directory:
            run = subprocess.run([sys.executable, str(pathlib.Path(__file__).resolve()), "--worker", mode, directory], capture_output=True, text=True)
            try:
                facts = json.loads(run.stdout)
            except json.JSONDecodeError:
                facts = {"reason": "worker failed", "stderr": run.stderr}
            results.append({"case": mode, "status": "PASSED" if run.returncode == 0 else "FAILED", "exit_code": run.returncode, "facts": facts})
            for name in ("build-messages.jsonl", "build.log"):
                source = pathlib.Path(directory) / name
                if source.exists():
                    (output / (mode + "-" + name)).write_bytes(source.read_bytes())
    report = {"command": ["python3", "scripts/e2e/binary-secret-runner-process.py"], "platform": sys.platform, "runner_sha256": hashlib.sha256(RUNNER.read_bytes()).hexdigest(), "cases": results}
    (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))
    return int(any(r["status"] != "PASSED" for r in results))


if __name__ == "__main__":
    if "--worker" in sys.argv:
        print(json.dumps(worker(sys.argv[-2], pathlib.Path(sys.argv[-1]))))
    else:
        raise SystemExit(main())
