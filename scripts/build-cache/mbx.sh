#!/usr/bin/env bash
# Run Desktop through the same pinned cache implementation as Engine.
set -euo pipefail
if [[ "${1:-}" == --help || "${1:-}" == -h || $# -eq 0 ]]; then
  printf 'Usage: bun mbx:exec -- <command...>\n'
  exit 0
fi
[[ "${1:-}" == --exec ]] || { echo 'mbx: expected --exec' >&2; exit 2; }
shift
[[ "${1:-}" == -- ]] && shift
[[ $# -gt 0 ]] || { echo 'mbx: missing command' >&2; exit 2; }
if [[ "${GITHUB_ACTIONS:-}" == true ]]; then
  # Direct CI invocations (including portability tests) may not run --ci setup.
  export UC_ENGINE_MBX_ENV_FILE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/ci-local-policy.sh"
fi
root=$(bash "$(dirname "${BASH_SOURCE[0]}")/engine-tools.sh")
# One native command tree, including Node/Python descendants, uses the Engine owner.
exec bash "$root/scripts/build-cache/mbx.sh" --exec "$@"
