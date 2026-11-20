#!/bin/sh
set -eu
cd "$(git rev-parse --show-toplevel)"
# Use a canonical temporary path on macOS; the cache rejects symlink parents.
export TMPDIR="$(python3 -c 'import os,tempfile; print(os.path.realpath(tempfile.gettempdir()))')"
make desktop-e2e CASE=desktop-processes
make desktop-e2e CASE=quick-panel-contract
make desktop-e2e CASE=history-search-copy
make desktop-e2e CASE=setup-onboarding
make desktop-e2e CASE=desktop-native-processes
