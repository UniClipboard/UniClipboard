#!/usr/bin/env python3
"""Execute the production Linux probe policy in isolated fault-model processes."""
import hashlib
import json
import os
import pathlib
import subprocess
import tempfile

repo = pathlib.Path(__file__).resolve().parents[4]
subprocess.run(["cargo", "build", "-p", "uc-platform", "--example", "secure_storage_readiness", "--features", "test-helpers"], cwd=repo, check=True)
binary = repo / "target/debug/examples/secure_storage_readiness"
results = []
for mode in ("delayed", "unavailable", "service-unknown", "denied", "denied-with-owner", "binary", "mismatch", "blocked"):
    run = subprocess.run([str(binary)], env={**os.environ, "UC_READINESS_E2E_MODE": mode}, capture_output=True, text=True, timeout=6)
    results.append({"case": mode, "passed": run.returncode == 0, "stdout": run.stdout, "stderr": run.stderr})
report = {"kind": "process failure model using production probe policy; not Linux D-Bus acceptance", "binarySha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "cases": results}
output = pathlib.Path(tempfile.mkdtemp(prefix="uc-readiness-model-"))
(output / "result.json").write_text(json.dumps(report, indent=2))
print(json.dumps({"artifacts": str(output), **report}))
raise SystemExit(0 if all(r["passed"] for r in results) else 1)
