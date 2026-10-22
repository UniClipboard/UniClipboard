#!/usr/bin/env python3
"""Run isolated process/API startup checks and retain reproducible artifacts."""
import json
import hashlib
import pathlib
import shutil
import subprocess
import tempfile

module = pathlib.Path(__file__).resolve().parents[2]
output = pathlib.Path(tempfile.mkdtemp(prefix="uc-startup-e2e-build-"))
binary = output / "uniclipd-startup-e2e"
subprocess.run(["go", "build", "-o", str(binary), "./e2e/startup"], cwd=module, check=True)
shutil.copy2(binary, output / "uniclipd")
result = subprocess.run([str(binary)], cwd=module, capture_output=True, text=True)
(output / "stdout.json").write_text(result.stdout)
(output / "stderr.txt").write_text(result.stderr)
(output / "provenance.json").write_text(json.dumps({"binarySha256":hashlib.sha256(binary.read_bytes()).hexdigest(), "sourceCommit":subprocess.check_output(["git","rev-parse","HEAD"],cwd=module,text=True).strip(), "sourceDirty":bool(subprocess.check_output(["git","status","--porcelain"],cwd=module,text=True)), "goVersion":subprocess.check_output(["go","version"],cwd=module,text=True).strip()},indent=2))
print(json.dumps({"artifacts": str(output), "returncode": result.returncode}))
print(result.stdout)
raise SystemExit(result.returncode)
