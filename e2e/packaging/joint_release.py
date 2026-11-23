#!/usr/bin/env python3
"""Exercise the joint release command with disposable GitHub workflow runs."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
SOURCE = "a" * 40
RECEIPT = Path("dist/joint-release.json")
REPORT = ROOT / ".cache/joint-release-e2e"


def github_fixture():
    state_path = Path(os.environ["FIXTURE_STATE"])
    state = json.loads(state_path.read_text())
    args = sys.argv[1:]
    result = None
    if args[0] == "api":
        endpoint = next(arg for arg in args if arg.startswith("repos/"))
        route = endpoint.removeprefix("repos/UniClipboard/UniClipboard/")
        if route.startswith("git/matching-refs/"):
            result = [state["tag"]] if "tag" in state else []
        elif route == "git/refs":
            payload = json.load(sys.stdin)
            state["tag"] = {
                "ref": payload["ref"],
                "object": {"type": "commit", "sha": payload["sha"]},
            }
            result = state["tag"]
        elif route.endswith("/dispatches"):
            payload = json.load(sys.stdin)
            assert payload["ref"] == f"release-source/{SOURCE}"
            assert payload["inputs"]["expected_source"] == SOURCE
            workflow = route.split("/")[-2]
            step = payload["inputs"].get("operation", workflow)
            state["events"].append((step, payload["inputs"]))
            run_id = 100 + len(state["events"])
            result = {
                "workflow_run_id": run_id,
                "html_url": f"https://example.com/{run_id}",
            }
            state["runs"][str(run_id)] = {
                "status": "completed",
                "conclusion": "failure" if state["failure"] == step else "success",
                "head_sha": SOURCE if state["wrong_source"] != step else "b" * 40,
                "run_attempt": 1,
            }
        elif route.endswith("/artifacts?per_page=100"):
            result = {"artifacts": [{"expired": False, "name": "joint-release-1"}]}
        elif route == "actions/workflows/build-desktop.yml":
            result = {"id": 99}
        else:
            assert route.startswith("actions/runs/"), route
            result = state["runs"][route.rsplit("/", 1)[1]]
    elif args[:2] == ["run", "rerun"]:
        assert "--failed" in args
        run = state["runs"][args[2]]
        run.update(run_attempt=run["run_attempt"] + 1, conclusion="success")
        state["reruns"].append(args[2])
    elif args[:2] == ["run", "download"]:
        artifact = args[args.index("--name") + 1]
        directory = Path(args[args.index("--dir") + 1])
        if artifact == "desktop-legacy-staging":
            (directory / "legacy-staging.json").write_text(
                json.dumps(
                    {
                        "status": "staged",
                        "native_receipts": 12,
                        "expected_revision": 8,
                    }
                )
            )
        else:
            assert artifact == "joint-release-1"
            shutil.copyfile(
                state_path.parent / "previous.json", directory / RECEIPT.name
            )
    else:
        raise AssertionError(args)
    state_path.write_text(json.dumps(state))
    if result is not None:
        print(json.dumps(result))


def exercise(
    root,
    *,
    channel="alpha",
    failure="",
    wrong_source="",
    stable_lock=False,
    version=None,
):
    root.mkdir()
    (root / "release").mkdir()
    products = (ROOT / "release/products.toml").read_text()
    metadata = tomllib.loads(products)["products"]
    if stable_lock:
        cli_build = metadata["cli"]["build_number"]
        lock = metadata["desktop"]["cli_build_number"]
        products = products.replace(
            f"cli_build_number = {lock}", f"cli_build_number = {cli_build}"
        )
    (root / "release/products.toml").write_text(products)
    state_path = root / "github.json"
    state_path.write_text(
        json.dumps(
            {
                "events": [],
                "reruns": [],
                "failure": failure,
                "wrong_source": wrong_source,
                "runs": {
                    "90": {
                        "head_sha": SOURCE,
                        "conclusion": "success",
                        "workflow_id": 99,
                    }
                },
            }
        )
    )
    env = dict(
        os.environ,
        FIXTURE_STATE=str(state_path),
        PATH=f"{root.parent / 'bin'}:{os.environ['PATH']}",
        GITHUB_REPOSITORY="UniClipboard/UniClipboard",
        GITHUB_SHA=SOURCE,
        GITHUB_RUN_ID="1",
        GITHUB_RUN_ATTEMPT="1",
        RELEASE_CHANNEL=channel,
        RELEASE_VERSION=version or metadata["cli"]["version"],
        DESKTOP_CANDIDATE="90" if channel == "stable" else "",
        STABLE_ACCEPTANCE=json.dumps(dict.fromkeys(("darwin", "linux", "windows"), {}))
        if channel == "stable"
        else "",
    )

    def run(expected_ok, attempt=1):
        env["GITHUB_RUN_ATTEMPT"] = str(attempt)
        process = subprocess.run(
            [sys.executable, ROOT / "tools/release/joint.py"],
            cwd=root,
            env=env,
            text=True,
            capture_output=True,
            timeout=20,
        )
        assert (process.returncode == 0) == expected_ok, process.stdout + process.stderr
        state = json.loads(state_path.read_bytes())
        return state, process.stderr

    return run


def accept():
    checks = {}
    with tempfile.TemporaryDirectory(prefix="unic-joint-e2e-") as temporary:
        root = Path(temporary)
        (root / "bin").mkdir()
        fixture = root / "bin/gh"
        shutil.copyfile(__file__, fixture)
        fixture.chmod(0o755)
        run = exercise(root / "wrong-version", version="999999999.0.0")
        state, error = run(False)
        assert "both pinned product versions" in error
        assert "tag" not in state and not state["events"]
        checks["wrong_version"] = {"external_mutations": 0}

        run = exercise(root / "alpha")
        state, _ = run(True)
        steps = [
            "release-cli.yml",
            "build-desktop.yml",
            "publish",
            "prepare-legacy-desktop-alpha.yml",
            "promote",
        ]
        assert [event[0] for event in state["events"]] == steps
        assert (
            state["events"][2][1]["candidate_run_id"]
            == state["events"][4][1]["candidate_run_id"]
            == "102"
        )
        assert state["events"][4][1]["expected_revision"] == "8"
        checks["alpha"] = json.loads((root / "alpha" / RECEIPT).read_bytes())

        run = exercise(root / "retry", failure="prepare-legacy-desktop-alpha.yml")
        state, error = run(False)
        assert [event[0] for event in state["events"]] == steps[
            :-1
        ] and "legacy_stage failed" in error
        shutil.copyfile(root / "retry" / RECEIPT, root / "retry/previous.json")
        state, _ = run(True, attempt=2)
        assert [event[0] for event in state["events"]] == steps and state["reruns"] == [
            "104"
        ]
        checks["retry"] = json.loads((root / "retry" / RECEIPT).read_bytes())

        run = exercise(root / "wrong-source", wrong_source="build-desktop.yml")
        state, error = run(False)
        assert [event[0] for event in state["events"]] == steps[
            :2
        ] and "different source" in error
        checks["wrong_source"] = json.loads(
            (root / "wrong-source" / RECEIPT).read_bytes()
        )

        run = exercise(root / "stable", channel="stable", stable_lock=True)
        state, _ = run(True)
        assert [event[0] for event in state["events"]] == [
            "publish",
            "release-cli.yml",
            "promote",
        ]
        assert (
            state["events"][0][1]["stable_acceptance"]
            and state["events"][0][1]["candidate_run_id"] == "90"
        )
        assert state["events"][1][1]["candidate_manifest_sha256"]
        assert state["events"][2][1]["expected_revision"] == "auto"
        checks["stable"] = json.loads((root / "stable" / RECEIPT).read_bytes())

        run = exercise(
            root / "unaccepted-stable",
            channel="stable",
            failure="publish",
            stable_lock=True,
        )
        state, error = run(False)
        assert [event[0] for event in state["events"]] == [
            "publish"
        ] and "desktop_publish failed" in error
        checks["unaccepted_stable"] = json.loads(
            (root / "unaccepted-stable" / RECEIPT).read_bytes()
        )
    return checks


def main():
    REPORT.mkdir(parents=True, exist_ok=True)
    report = {
        "status": "failed",
        "scope": "joint release process and GitHub command fixture",
    }
    try:
        report["scenarios"] = accept()
        report["status"] = "passed"
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        report["source_sha"] = subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
        ).strip()
        report["controller_sha256"] = hashlib.sha256(
            (ROOT / "tools/release/joint.py").read_bytes()
        ).hexdigest()
        data = (json.dumps(report, indent=2) + "\n").encode()
        (REPORT / "report.json").write_bytes(data)
        (REPORT / "SHA256SUMS").write_text(
            f"{hashlib.sha256(data).hexdigest()}  report.json\n"
        )


if __name__ == "__main__":
    if Path(sys.argv[0]).name == "gh":
        github_fixture()
    else:
        main()
