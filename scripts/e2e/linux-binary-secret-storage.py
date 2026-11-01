#!/usr/bin/env python3
"""Process E2E against GNOME Keyring on private buses; never uses a host wallet."""
import hashlib
import json
import os
import pathlib
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time

REPO = pathlib.Path(__file__).resolve().parents[2]
CASES = []


def record(name, status, **facts):
    CASES.append({"case": name, "status": status, **facts})


def build_example(output):
    command = ["cargo", "build", "--locked", "--message-format=json", "-p", "uc-platform", "--example", "binary_secret_storage"]
    messages = output / "build-messages.jsonl"
    with messages.open("wb") as stdout, (output / "build.log").open("wb") as stderr:
        build = subprocess.run(command, cwd=REPO, stdout=stdout, stderr=stderr)
    facts = {"command": command, "exit_code": build.returncode}
    if build.returncode:
        return None, facts
    executables = set()
    for line in messages.read_text().splitlines():
        message = json.loads(line)
        target = message.get("target", {})
        if (message.get("reason") == "compiler-artifact"
                and target.get("name") == "binary_secret_storage"
                and "example" in target.get("kind", []) and message.get("executable")):
            path = pathlib.Path(message["executable"])
            executables.add((REPO / path).resolve())
    if len(executables) != 1:
        raise RuntimeError("Cargo must report exactly one example executable")
    binary = executables.pop()
    facts.update(executable=str(binary), binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest())
    return binary, facts


def run_private_session(command, env, scratch, timeout=180):
    """Own one POSIX session; prove its group is gone before deleting its data."""
    process = None
    cleanup = {"sigterm_sent": False, "sigkill_sent": False,
               "leader_reaped": False, "group_gone": False, "directory_removed": False}
    result = {"command": command, "exit_code": None, "timed_out": False,
              "stdout": "", "cleanup": cleanup}
    try:
        process = subprocess.Popen(command, env=env, stdout=subprocess.PIPE,
                                   stderr=subprocess.DEVNULL, text=True, start_new_session=True)
        cleanup["process_group"] = process.pid
        result["stdout"], _ = process.communicate(timeout=timeout)
        result["exit_code"] = process.returncode
    except subprocess.TimeoutExpired:
        result["timed_out"] = True
    except (Exception, KeyboardInterrupt) as error:
        result["reason"] = type(error).__name__
    finally:
        if process is None:
            cleanup.update(leader_reaped=True, group_gone=True)
        else:
            def group_exists():
                process.poll()  # Reap the direct child before checking the group.
                try:
                    os.killpg(process.pid, 0)
                    return True
                except ProcessLookupError:
                    return False

            try:
                for sig, field in ((signal.SIGTERM, "sigterm_sent"), (signal.SIGKILL, "sigkill_sent")):
                    if not group_exists():
                        break
                    try:
                        os.killpg(process.pid, sig)
                        cleanup[field] = True
                    except ProcessLookupError:
                        break
                    deadline = time.monotonic() + 5
                    while group_exists() and time.monotonic() < deadline:
                        time.sleep(0.05)
                cleanup["group_gone"] = not group_exists()
                process.wait(timeout=5)
                cleanup["leader_reaped"] = True
                result["exit_code"] = process.returncode
                if cleanup["group_gone"]:
                    # No descendant can hold the pipe open now.
                    result["stdout"], _ = process.communicate(timeout=2)
            except (OSError, subprocess.SubprocessError) as error:
                cleanup["reason"] = type(error).__name__
            finally:
                if process.stdout:
                    process.stdout.close()
        if cleanup["group_gone"] and cleanup["leader_reaped"]:
            try:
                shutil.rmtree(scratch)
                cleanup["directory_removed"] = True
            except OSError as error:
                cleanup["reason"] = type(error).__name__
        if not cleanup["directory_removed"]:
            cleanup["retained_directory"] = str(scratch)
    return result


def session(mode):
    """Each command is a new process; no payload or wallet enters the report."""
    root = pathlib.Path(os.environ["UC_BINARY_SECRET_E2E_ROOT"])
    source = root / "secure-storage-source.json"
    original = source.read_bytes()
    binary = pathlib.Path(os.environ["UC_BINARY_SECRET_E2E_BINARY"])
    daemon = None
    try:
        if mode in ("available", "denied"):
            daemon = subprocess.Popen(
                ["gnome-keyring-daemon", "--foreground", "--components=secrets", "--unlock"],
                stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            # This disposable wallet password exists only in memory and a pipe.
            daemon.stdin.write(os.urandom(32).hex().encode() + b"\n")
            daemon.stdin.close()
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                owner = subprocess.run(
                    ["dbus-send", "--session", "--print-reply", "--dest=org.freedesktop.DBus",
                     "/org/freedesktop/DBus", "org.freedesktop.DBus.NameHasOwner",
                     "string:org.freedesktop.secrets"], capture_output=True, timeout=2,
                )
                if b"boolean true" in owner.stdout:
                    break
                time.sleep(0.05)
            else:
                raise RuntimeError("private wallet did not acquire service name")

        def check(name, *args):
            run = subprocess.run([str(binary), *args], capture_output=True, timeout=15)
            # Do not retain backend error bodies, secrets, or full D-Bus messages.
            record(name, "PASSED" if run.returncode == 0 else "FAILED",
                   command=[str(binary), *args],
                   exit_code=run.returncode)

        if mode == "available":
            check("selected-system-probe", "selected")
            for kind in ("all-bytes", "empty"):
                check(f"{kind}-create", "write", kind, kind)
                check(f"{kind}-cross-process-read", "read", kind, kind)
                check(f"{kind}-create-mime", "inspect", kind, kind)
                check(f"{kind}-update", "write", kind, "updated")
                check(f"{kind}-update-read", "read", kind, "updated")
                check(f"{kind}-update-mime", "inspect", kind, "updated")
                check(f"{kind}-delete", "delete", kind)
                check(f"{kind}-absent", "absent", kind)
                check(f"{kind}-delete-absent", "delete", kind)
            for seed, inspect in (("seed", "inspect-legacy"), ("seed-no-target", "inspect-no-target")):
                for kind in ("text", "all-bytes"):
                    key = f"{seed}-{kind}"
                    check(f"{key}-fixture", seed, key, kind)
                    check(f"{key}-legacy-read", "read", key, kind)
                    check(f"{key}-update", "write", key, "updated")
                    check(f"{key}-updated-read", "read", key, "updated")
                    check(f"{key}-same-item", inspect, key, "updated")
                    check(f"{key}-delete", "delete", key)
                    check(f"{key}-absent", "absent", key)
            check("ambiguous-fixture", "seed-duplicate", "duplicate", "text")
            for operation in ("write-fails", "read-fails", "delete-fails"):
                check(f"ambiguous-{operation}", operation, "duplicate")
            check("ambiguous-items-unchanged", "inspect-duplicate", "duplicate", "text")
        else:
            suffix = "denied" if mode == "denied" else "fails"
            for operation in (f"write-{suffix}", f"read-{suffix}", f"delete-{suffix}", "selected-fails"):
                check(f"{mode}-{operation}", operation)
        record(f"{mode}-source-unchanged", "PASSED" if source.read_bytes() == original else "FAILED")
        record(f"{mode}-no-file-store", "PASSED" if not (root / "keyring").exists() else "FAILED")
    except (OSError, subprocess.SubprocessError, RuntimeError) as error:
        record(mode + "-harness", "FAILED", reason=type(error).__name__)
    finally:
        if daemon:
            daemon.terminate()
            try:
                daemon.wait(timeout=5)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait()
    print(json.dumps(CASES))
    return int(any(c["status"] == "FAILED" for c in CASES))


def main():
    output = pathlib.Path(os.environ.get("UC_BINARY_SECRET_ARTIFACTS", REPO / "target/binary-secret-storage"))
    output.mkdir(parents=True, exist_ok=True)
    # Include untracked test/driver inputs, which git diff alone omits.
    inputs = ["Cargo.lock", "crates/uc-platform/Cargo.toml",
              "crates/uc-platform/src/system_secure_storage.rs",
              "crates/uc-platform/examples/binary_secret_storage.rs",
              "scripts/e2e/linux-binary-secret-storage.py",
              "scripts/e2e/binary-secret-runner-process.py",
              "scripts/e2e/linux-binary-secret-storage.md"]
    inputs += [str(p.relative_to(REPO)) for p in (REPO / "crates/uc-platform/src/system_secure_storage").glob("*.rs")]
    patch = subprocess.run(["git", "diff", "--binary", "HEAD", "--", *inputs], cwd=REPO, capture_output=True, check=True).stdout
    for path in inputs:
        tracked = subprocess.run(["git", "ls-files", "--error-unmatch", "--", path], cwd=REPO, capture_output=True)
        if tracked.returncode:
            patch += subprocess.run(["git", "diff", "--no-index", "--binary", "--", "/dev/null", path], cwd=REPO, capture_output=True).stdout
    (output / "source.patch").write_bytes(patch)
    versions = dict(re.findall(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"', (REPO / "Cargo.lock").read_text()))
    report = {
        "source_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
        "patch_sha256": hashlib.sha256(patch).hexdigest(),
        "inputs_sha256": {p: hashlib.sha256((REPO / p).read_bytes()).hexdigest() for p in inputs},
        "dependencies": {name: versions[name] for name in ("keyring", "dbus-secret-service")},
        "command": ["python3", "scripts/e2e/linux-binary-secret-storage.py"],
        "environment": sys.platform,
        "scope": "private D-Bus and GNOME Keyring; not KDE acceptance",
        "cases": CASES,
    }
    exit_code = 1
    try:
        required = ("dbus-run-session", "dbus-send", "gnome-keyring-daemon")
        if sys.platform != "linux" or any(not shutil.which(p) for p in required):
            record("linux-process-e2e", "NOT_RUN", reason="requires Linux, D-Bus and GNOME Keyring")
            exit_code = 77
        elif not pathlib.Path("/dev/shm").is_dir():
            record("linux-process-e2e", "NOT_RUN", reason="requires /dev/shm for disposable wallets")
            exit_code = 77
        else:
            binary, report["build"] = build_example(output)
            if binary is None:
                record("build", "FAILED", exit_code=report["build"]["exit_code"])
            else:
                report["executable"] = str(binary)
                report["binary_sha256"] = report["build"]["binary_sha256"]
                report["backend_version"] = subprocess.run(["gnome-keyring-daemon", "--version"], capture_output=True, text=True).stdout.strip()
                for mode in ("available", "unavailable", "denied"):
                    scratch = tempfile.mkdtemp(prefix="uc-binary-secret-", dir="/dev/shm")
                    session_owned = False
                    try:
                        sandbox = pathlib.Path(scratch)
                        root = sandbox / "profile"
                        root.mkdir()
                        (root / "secure-storage-source.json").write_text('{"version":1,"store":"system"}')
                        env = {k: v for k, v in os.environ.items() if not k.startswith(("UC_", "DBUS_", "GNOME_KEYRING_", "XDG_")) and k not in ("APPIMAGE", "APPDIR", "UNICLIPBOARD_ENV")}
                        env.update(HOME=scratch, XDG_RUNTIME_DIR=str(sandbox / "runtime"),
                                   XDG_DATA_HOME=str(sandbox / "data"), XDG_CONFIG_HOME=str(sandbox / "config"),
                                   XDG_CACHE_HOME=str(sandbox / "cache"), XDG_STATE_HOME=str(sandbox / "state"),
                                   XDG_CURRENT_DESKTOP="GNOME", DISPLAY=":e2e",
                                   UC_PROFILE="binary-secret-e2e", UC_BINARY_SECRET_E2E_PRIVATE_BUS="1",
                                   UC_BINARY_SECRET_E2E_ROOT=str(root), UC_BINARY_SECRET_E2E_BINARY=str(binary))
                        pathlib.Path(env["XDG_RUNTIME_DIR"]).mkdir(mode=0o700)
                        policy = '<deny send_destination="org.freedesktop.secrets"/>' if mode == "denied" else ""
                        config = sandbox / "bus.conf"
                        config.write_text('<busconfig><type>session</type><listen>unix:tmpdir=/dev/shm</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/>' + policy + '</policy></busconfig>')
                        session_owned = True
                        run = run_private_session(["dbus-run-session", "--config-file=" + str(config), "--", sys.executable, str(pathlib.Path(__file__).resolve()), "--session", mode], env, scratch)
                        report.setdefault("sessions", []).append({"mode": mode, **{k: v for k, v in run.items() if k != "stdout"}})
                        try:
                            CASES.extend(json.loads(run["stdout"]))
                        except json.JSONDecodeError:
                            record(mode + "-runner", "FAILED", reason="missing structured result")
                        if run["exit_code"] or run["timed_out"] or run.get("reason") or not run["cleanup"]["directory_removed"]:
                            record(mode + "-session", "FAILED", exit_code=run["exit_code"], timed_out=run["timed_out"], cleanup=run["cleanup"])
                        if run.get("reason") == "KeyboardInterrupt":
                            break
                    except BaseException:
                        # Before process creation, only an empty synthetic sandbox exists.
                        if not session_owned:
                            shutil.rmtree(scratch, ignore_errors=True)
                        elif pathlib.Path(scratch).exists():
                            record(mode + "-cleanup", "FAILED", retained_directory=scratch)
                        raise
                exit_code = int(any(c["status"] == "FAILED" for c in CASES))
    except (OSError, subprocess.SubprocessError, RuntimeError, ValueError, KeyboardInterrupt) as error:
        record("harness", "FAILED", reason=type(error).__name__)
    finally:
        record("kde-wallet-unlock-cancel", "NOT_RUN", reason="requires isolated KDE interactive acceptance")
        report["exit_code"] = exit_code
        (output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps({"artifacts": str(output), "exit_code": exit_code, "cases": CASES}))
    return exit_code


if __name__ == "__main__":
    raise SystemExit(session(sys.argv[-1]) if "--session" in sys.argv else main())
