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
root=$(bash "$(dirname "${BASH_SOURCE[0]}")/engine-tools.sh")
# The repository Cargo entry loads CI credentials only in compiler processes.
# shellcheck source=/dev/null
source "$root/scripts/build-cache/env.sh"
exec "$@"
