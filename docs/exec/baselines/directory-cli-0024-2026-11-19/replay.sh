#!/bin/sh
set -eu
# Run from the repository root. Supply an existing authorized SSH alias and a fresh output path.
remote_alias=${1:?usage: replay.sh SSH_ALIAS NEW_OUTPUT_DIRECTORY}
receipt_root=${2:?usage: replay.sh SSH_ALIAS NEW_OUTPUT_DIRECTORY}
if [ -e "$receipt_root" ]; then echo 'Output already exists' >&2; exit 1; fi
mkdir -p "$receipt_root"
receipt_root=$(cd "$receipt_root" && pwd)
private_root=$(mktemp -d /tmp/unic-f-replay-XXXXXX)
trap 'rm -rf "$private_root"' EXIT HUP INT TERM
python3 - "$private_root/lab.toml" "$remote_alias" <<'PY'
import json, pathlib, sys
pathlib.Path(sys.argv[1]).write_text('''version = 1
[nodes.mac]
transport = "local"
[nodes.linux]
transport = "ssh"
host = ''' + json.dumps(sys.argv[2]) + '''
[suite]
topology = "mesh"
repetitions = 1
receive_timeout_seconds = 180
scenarios = ["directory"]
directory_bytes = [0]
text_bytes = []
png_pixels = []
file_bytes = []
html_bytes = []
''')
PY
go build -o "$private_root/unic-e2e" ./e2e/real
python3 tools/directorye2e/run.py --output "$receipt_root/process"
"$private_root/unic-e2e" run --lab "$private_root/lab.toml" --out "$receipt_root/functional"
python3 - "$private_root/lab.toml" "$receipt_root/functional/public/manifest.json" <<'PY2'
import json, pathlib, sys
m = json.loads(pathlib.Path(sys.argv[2]).read_text())
assert m['status'] == 'passed' and len(m['directory_checks']) == 32
p = pathlib.Path(sys.argv[1])
p.write_text(p.read_text().replace('scenarios = ["directory"]', 'scenarios = ["pairwise"]').replace('directory_bytes = [0]', 'directory_bytes = [0, 1024, 16777216]'))
PY2
"$private_root/unic-e2e" run --lab "$private_root/lab.toml" --out "$receipt_root/performance"
UNIC_HEADLESS_E2E=1 UNIC_HEADLESS_ARTIFACTS="$receipt_root/headless" \
  go test -race ./e2e/headless -run '^TestProductionHeadless$' -v -count=1
go vet ./e2e/real ./internal/localapi
go run ./tools/docscheck
