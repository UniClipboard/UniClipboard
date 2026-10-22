#!/usr/bin/env bash
# Share Python/SDK setup between composite actions and pinned release automation.
set -euo pipefail
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
export UC_MBX_SNAPSHOT_SOURCE_ROOT="${UC_MBX_SNAPSHOT_SOURCE_ROOT:-${GITHUB_WORKSPACE:-$PWD}}"
python_command=python3
python_directory=bin
python_executable=python
if [[ "${RUNNER_OS:-}" == Windows ]]; then
  python_command=python
  python_directory=Scripts
  python_executable=python.exe
  for variable in RUNNER_TEMP UC_ENGINE_MBX_ENV_FILE UC_ENGINE_MBX_STATS_DIR MBX_CACHE_DIR UC_MBX_SNAPSHOT_SOURCE_ROOT; do
    export "$variable=$(cygpath -m "${!variable}")"
  done
fi
python_bin="$RUNNER_TEMP/mbx-snapshot-python/$python_directory/$python_executable"
if [[ ! -x "$python_bin" ]]; then
  if ! "$python_command" -m venv "$RUNNER_TEMP/mbx-snapshot-python"; then
    [[ "${RUNNER_OS:-}" == Linux ]] || exit 1
    apt-get update
    apt-get install -y --no-install-recommends python3-venv
    "$python_command" -m venv "$RUNNER_TEMP/mbx-snapshot-python"
  fi
fi
if ! "$python_bin" -c 'import boto3' 2>/dev/null; then
  "$python_bin" -m pip install --disable-pip-version-check -q boto3==1.43.117
fi
source "$UC_ENGINE_MBX_ENV_FILE"
"$python_bin" "$script_dir/mbx-snapshot.py" "$@"
