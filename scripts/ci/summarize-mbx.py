#!/usr/bin/env python3
"""Summarize MBX logical requests without presenting them as billed operations."""

import argparse
import json
import os
from pathlib import Path


COUNTERS = (
    "remote_manifest_lookups", "remote_action_lookups", "remote_blob_requests",
    "remote_blob_pack_requests", "remote_failures", "background_upload_failures",
    "downloaded_bytes", "uploaded_bytes", "hits", "misses", "unconsulted",
    "prefetched_actions", "restored_output_bytes",
)


def summarize(root):
    totals = dict.fromkeys(COUNTERS, 0)
    sessions = 0
    for file in sorted(root.rglob("*.json")):
        if file.name == "summary.json":
            continue
        data = json.loads(file.read_text())
        if "session_duration_ns" not in data:
            continue
        sessions += 1
        for key in COUNTERS:
            value = data.get(key, 0)
            if not isinstance(value, int) or isinstance(value, bool) or value < 0:
                raise ValueError(f"Invalid MBX counter: {key}")
            totals[key] += value
    totals["logical_reads"] = sum(totals[key] for key in COUNTERS[:4])
    snapshots = [json.loads(file.read_text()) for file in sorted(root.glob("snapshot-*.json"))]
    totals["snapshot_logical_reads"] = sum(
        item.get("index_reads", 0) + item.get("archive_reads", 0) for item in snapshots
    )
    totals["total_logical_reads"] = totals["logical_reads"] + totals["snapshot_logical_reads"]
    totals["total_downloaded_bytes"] = totals["downloaded_bytes"] + sum(
        item.get("downloaded_bytes", 0) for item in snapshots
    )
    totals["total_uploaded_bytes"] = totals["uploaded_bytes"] + sum(
        item.get("uploaded_bytes", 0) for item in snapshots
    )
    return {"sessions": sessions, **totals}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    args = parser.parse_args()
    result = summarize(args.root)
    args.root.mkdir(parents=True, exist_ok=True)
    (args.root / "summary.json").write_text(json.dumps(result, indent=2) + "\n")
    rows = ["### MBX cache requests", "", "Logical requests, not R2 billed operations; retries are excluded.", "", "| Metric | Total |", "| --- | ---: |"]
    rows.extend(f"| {key} | {value:,} |" for key, value in result.items())
    for snapshot in sorted(args.root.glob("snapshot-*.json")):
        rows.extend(["", "Snapshot transport:", "", "```json", snapshot.read_text().strip(), "```"])
    report = "\n".join(rows) + "\n"
    print(report)
    if os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as output:
            output.write(report)


if __name__ == "__main__":
    main()
