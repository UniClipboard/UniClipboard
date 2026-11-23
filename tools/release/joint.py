#!/usr/bin/env python3
"""Coordinate the canonical CLI and Desktop delivery workflows."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

from release import identity, metadata

RECEIPT = Path("dist/joint-release.json")
API_VERSION = "X-GitHub-Api-Version: 2026-03-10"


def gh(*args, payload=None):
    return subprocess.check_output(["gh", *args], input=payload, text=True).strip()


def api(path, payload=None):
    endpoint = f"repos/{os.environ['GITHUB_REPOSITORY']}/{path}"
    args = ["api", "-H", API_VERSION, endpoint]
    if payload is not None:
        args.extend(["--method", "POST", "--input", "-"])
    return json.loads(
        gh(*args, payload=json.dumps(payload) if payload is not None else None)
    )


def download(run_id, artifact, filename):
    with tempfile.TemporaryDirectory(prefix="unic-joint-release-") as directory:
        gh(
            "run",
            "download",
            str(run_id),
            "--repo",
            os.environ["GITHUB_REPOSITORY"],
            "--name",
            artifact,
            "--dir",
            directory,
        )
        return json.loads((Path(directory) / filename).read_bytes())


def plan():
    channel = os.environ["RELEASE_CHANNEL"]
    version = os.environ["RELEASE_VERSION"]
    cli = metadata("release/products.toml")
    desktop = metadata("release/products.toml", "desktop")
    if (
        channel not in ("alpha", "stable")
        or version != cli["version"]
        or version != desktop["version"]
    ):
        raise ValueError(
            "channel and both pinned product versions must match the release request"
        )
    candidate = os.environ["DESKTOP_CANDIDATE"]
    acceptance = os.environ["STABLE_ACCEPTANCE"]
    if candidate and (
        not candidate.isascii() or not candidate.isdecimal() or not int(candidate)
    ):
        raise ValueError("invalid Desktop candidate run ID")
    if channel == "stable":
        if not candidate or not acceptance:
            raise ValueError(
                "stable requires an accepted Desktop run and native receipts"
            )
        if (desktop["cli_version"], desktop["cli_build_number"]) != (
            version,
            cli["build_number"],
        ):
            raise ValueError(
                "stable Desktop must lock the CLI build selected for npm latest"
            )
        receipts = json.loads(acceptance)
        if not isinstance(receipts, dict) or set(receipts) != {
            "darwin",
            "linux",
            "windows",
        }:
            raise ValueError(
                "stable requires one native receipt for each Desktop platform"
            )
    return (
        {
            "channel": channel,
            "source": os.environ["GITHUB_SHA"],
            "cli": identity(cli),
            "desktop": identity(desktop),
        },
        desktop,
        candidate,
        acceptance,
    )


def save(receipt):
    RECEIPT.parent.mkdir(parents=True, exist_ok=True)
    RECEIPT.write_text(json.dumps(receipt, indent=2) + "\n")


def source_tag(source):
    name = f"release-source/{source}"
    ref = f"refs/tags/{name}"
    matches = api(f"git/matching-refs/tags/{name}")
    existing = next((item for item in matches if item["ref"] == ref), None)
    if existing is None:
        existing = api("git/refs", {"ref": ref, "sha": source})
    target = existing["object"]
    if (target["type"], target["sha"]) != ("commit", source):
        raise ValueError("release source tag does not point at the selected commit")
    return name


def restore(selection):
    parent = os.environ["GITHUB_RUN_ID"]
    attempt = int(os.environ["GITHUB_RUN_ATTEMPT"])
    if attempt > 1:
        artifacts = api(f"actions/runs/{parent}/artifacts?per_page=100")["artifacts"]
        prefix = "joint-release-"
        attempts = {
            int(item["name"].removeprefix(prefix)): item["name"]
            for item in artifacts
            if item["name"].startswith(prefix)
            and item["name"].removeprefix(prefix).isdigit()
            and not item["expired"]
        }
        previous = max((number for number in attempts if number < attempt), default=0)
        if previous:
            receipt = download(parent, attempts[previous], RECEIPT.name)
            if receipt["selection"] != selection:
                raise ValueError("previous run did not select the same release")
            return receipt
    return {"selection": selection, "runs": {}}


def start(receipt, step, workflow, inputs):
    runs = receipt["runs"]
    if step in runs:
        child = runs[step]
        status = api(f"actions/runs/{child['id']}")
        if status["head_sha"] != receipt["selection"]["source"]:
            raise ValueError(
                f"{step} was dispatched on a different source: {child['url']}"
            )
        if (
            status["run_attempt"] >= child["attempt"]
            and status["status"] == "completed"
            and status["conclusion"] != "success"
        ):
            gh(
                "run",
                "rerun",
                str(child["id"]),
                "--repo",
                os.environ["GITHUB_REPOSITORY"],
                "--failed",
            )
            child["attempt"] = status["run_attempt"] + 1
            save(receipt)
        return child
    result = api(
        f"actions/workflows/{workflow}/dispatches",
        {
            "ref": receipt["source_tag"],
            "inputs": {**inputs, "expected_source": receipt["selection"]["source"]},
        },
    )
    child = {"id": result["workflow_run_id"], "url": result["html_url"], "attempt": 1}
    runs[step] = child
    save(receipt)
    print(f"{step}: {child['url']}", flush=True)
    return child


def wait(receipt, step, child):
    deadline = time.monotonic() + 100 * 60
    while time.monotonic() < deadline:
        status = api(f"actions/runs/{child['id']}")
        if status["head_sha"] != receipt["selection"]["source"]:
            raise ValueError(
                f"{step} was dispatched on a different source: {child['url']}"
            )
        if (
            status["run_attempt"] >= child["attempt"]
            and status["status"] == "completed"
        ):
            child["conclusion"] = status["conclusion"]
            save(receipt)
            if child["conclusion"] != "success":
                raise ValueError(f"{step} failed: {child['url']}")
            return child["id"]
        time.sleep(15)
    raise TimeoutError(f"{step} did not finish: {child['url']}")


def deliver(receipt, desktop, candidate, acceptance):
    channel = receipt["selection"]["channel"]
    if candidate:
        status = api(f"actions/runs/{candidate}")
        workflow = api("actions/workflows/build-desktop.yml")
        if (
            status["head_sha"] != receipt["selection"]["source"]
            or status["conclusion"] != "success"
            or status["workflow_id"] != workflow["id"]
        ):
            raise ValueError(
                "Desktop candidate must be an accepted build from the selected source"
            )
    receipt["source_tag"] = source_tag(receipt["selection"]["source"])
    save(receipt)

    cli_inputs = {"channel": channel}
    if channel == "stable":
        cli_inputs.update(
            candidate_run_id=str(desktop["cli_artifact_run_id"]),
            candidate_manifest_sha256=desktop["cli_manifest_sha256"],
        )
    else:
        cli = start(receipt, "cli", "release-cli.yml", cli_inputs)

    if not candidate:
        build = start(
            receipt, "desktop_build", "build-desktop.yml", {"distribution": "true"}
        )
        candidate = wait(receipt, "desktop_build", build)
    if channel == "alpha":
        wait(receipt, "cli", cli)

    desktop_inputs = {"channel": channel, "candidate_run_id": str(candidate)}
    if acceptance:
        desktop_inputs["stable_acceptance"] = acceptance
    publication = start(
        receipt,
        "desktop_publish",
        "release-desktop.yml",
        {
            **desktop_inputs,
            "operation": "publish",
        },
    )
    wait(receipt, "desktop_publish", publication)

    revision = "auto"
    if channel == "alpha":
        stage = start(
            receipt,
            "legacy_stage",
            "prepare-legacy-desktop-alpha.yml",
            {
                "version": desktop["version"],
                "build_number": str(desktop["build_number"]),
                "stage": "true",
            },
        )
        wait(receipt, "legacy_stage", stage)
        staged = download(stage["id"], "desktop-legacy-staging", "legacy-staging.json")
        if (
            staged["status"] != "staged"
            or staged["native_receipts"] != 12
            or type(staged["expected_revision"]) is not int
            or staged["expected_revision"] < 0
        ):
            raise ValueError("legacy staging receipt is incomplete")
        revision = str(staged["expected_revision"])
    else:
        cli = start(receipt, "cli", "release-cli.yml", cli_inputs)
        wait(receipt, "cli", cli)

    promotion = start(
        receipt,
        "desktop_promote",
        "release-desktop.yml",
        {
            **desktop_inputs,
            "operation": "promote",
            "expected_revision": revision,
        },
    )
    wait(receipt, "desktop_promote", promotion)


def main():
    selection, desktop, candidate, acceptance = plan()
    receipt = restore(selection)
    receipt["status"] = "running"
    save(receipt)
    try:
        deliver(receipt, desktop, candidate, acceptance)
    except Exception:
        receipt["status"] = "failed"
        raise
    else:
        receipt["status"] = "available"
    finally:
        save(receipt)


if __name__ == "__main__":
    main()
