#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
apps/gui-go/build.sh
python3 apps/gui-go/e2e/run.py --interactive --out "$ROOT/target/gui-go/manual-evidence"
