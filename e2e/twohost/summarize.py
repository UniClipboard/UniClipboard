#!/usr/bin/env python3
"""Render the two-host manifest without discarding failed or missing cells."""
import argparse
import collections
import hashlib
import json
from pathlib import Path
import statistics


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("manifest", type=Path)
    p.add_argument("--output", type=Path, required=True)
    opt = p.parse_args()
    m = json.loads(opt.manifest.read_text())
    groups = collections.defaultdict(list)
    for sample in m["samples"]:
        groups[sample["direction"], sample["type"], sample["bytes"]].append(sample)
    lines = ["# 0004 P1 two-host CLI acceptance", "",
             "Status: **" + m["status"] + "**. Source HEAD: `" + m["source_head"] + "`.", "",
             "Scope: " + m["scope"] + ".", "",
             "Start: " + m["started_at"] + ". End: " + m.get("finished_at", "running") + ".", "",
             "The source manifest includes uncommitted production-source content. HEAD alone does not identify this build.", "",
             "## Hosts and build", "", "- " + m.get("go", "unknown build"),
             "- macOS: " + m.get("hosts", {}).get("mac", "unknown").replace("\n", " ").replace("\r", ""),
             "- Windows: " + m.get("hosts", {}).get("windows", "unknown").replace("\n", " ").replace("\r", ""),
             "- Verified snapshot source matches recorded hashes: " + str(m.get("source_unchanged", "not verified")), "",
             "## Functional checks", ""]
    lines.extend("- PASS: " + s for s in m["checks"])
    if "failure" in m:
        lines += ["", "Failure: " + m["failure"]]
    lines += ["", "## Latency", "", m["latency_definition"] + ".", "",
              "Each cell contains three samples in the full run. Median and observed minimum/maximum are reported; three samples do not support a tail-latency percentile.", "",
              "The receipt estimate uses sender-local CLI start time and Engine receive time. It includes sender CLI startup, IPC, commit and receive processing. A native helper estimates host clock offset over a persistent SSH connection before each matrix cell; this is not a pure transport measurement.", ""]
    if "windows_clock_estimate" in m:
        c = m["windows_clock_estimate"]
        lines += [f"Initial clock-estimate uncertainty: ±{c['uncertainty_ms']:.2f} ms, plus 1 ms timestamp quantization. Each raw sample records its own uncertainty. Estimates below that bound do not resolve a precise one-way latency.", ""]
    if m.get("ssh_probe_ms"):
        lines += [f"SSH clock-probe round-trip median: {statistics.median(m['ssh_probe_ms']):.2f} ms. No overhead is subtracted from verified latency.", ""]
    if m.get("samples"):
        uncertainty = max(s.get("receipt_estimate_uncertainty_ms", 0) for s in m["samples"])
        lines += [f"Maximum per-sample receipt-estimate uncertainty: ±{uncertainty:.2f} ms. Final minus initial clock offset: {m.get("clock_offset_change_ms", 0):.2f} ms.", ""]
    lines += ["| Direction | Type | Bytes | n | Submit median ms | Verified median ms | Verified min–max ms | Receipt estimate median ms |",
              "|---|---|---:|---:|---:|---:|---:|---:|"]
    for (direction, kind, size), samples in sorted(groups.items()):
        verified = [s["verified_ms"] for s in samples]
        receipt = [s["receipt_estimate_ms"] for s in samples if "receipt_estimate_ms" in s]
        lines.append(f"| {direction} | {kind} | {size} | {len(samples)} | "
                     f"{statistics.median(s['submit_ms'] for s in samples):.2f} | "
                     f"{statistics.median(verified):.2f} | {min(verified):.2f}–{max(verified):.2f} | "
                     + (f"{statistics.median(receipt):.2f}" if receipt else "unavailable") + " |")
    lines += ["", "## Limits", ""]
    lines.extend("- " + s + "." for s in m["limits"])
    lines += ["- Direct LAN versus DERP routing was not independently established.",
              "- Negative transmission checks use bounded observation windows.",
              "- Windows restart uses forced process termination; macOS uses SIGTERM.", "",
              "## Evidence", "",
              "Manifest SHA256: `" + hashlib.sha256(opt.manifest.read_bytes()).hexdigest() + "`.", "",
              "The adjacent manifest contains source and binary hashes and every raw sample. Reproduce with the runner and instructions in `e2e/twohost/README.md`.", ""]
    opt.output.write_text("\n".join(lines))


if __name__ == "__main__":
    main()
