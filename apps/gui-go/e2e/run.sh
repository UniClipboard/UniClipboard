#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
cd "$ROOT"
apps/gui-go/build.sh e2e
python3 apps/gui-go/e2e/run.py --out "${1:-$ROOT/target/gui-go/evidence}"
