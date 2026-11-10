#!/bin/sh
# Build and exercise the complete fixture views without opening a native app.
set -eu
cd "$(dirname "$0")/../../.."
artifact_dir="${UNIC_UI_TEST_ARTIFACTS:-$PWD/.build/quick-panel-e2e}"
mkdir -p "$artifact_dir"
artifact_dir="$(cd "$artifact_dir" && pwd)"
export UNIC_UI_TEST_ARTIFACTS="$artifact_dir"
go test -race ./app/desktop/internal/desktopdemo -run '^TestQuickPanelE2E$' -count=1 -v > "$artifact_dir/test.log" 2>&1 || {
 cat "$artifact_dir/test.log"
 exit 1
}
cat "$artifact_dir/test.log"
go build -o "$artifact_dir/unic-desktop" ./app/desktop/cmd/unic-desktop
python3 - "$artifact_dir" <<'PY'
import hashlib, json, pathlib, subprocess, sys
out = pathlib.Path(sys.argv[1])
files = sorted(pathlib.Path('app/desktop/internal/desktopdemo').glob('quick_panel*.go'))
files += [pathlib.Path(p) for p in [
 'app/desktop/internal/desktopdemo/demo.go',
 'app/desktop/internal/desktopdemo/entry.go',
 'app/desktop/internal/desktopdemo/settings.go',
 'app/desktop/scripts/quick-panel-e2e.sh',
 'go.mod', 'go.sum',
]]
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
manifest = {
 'reference': '885546c7cf6f6f0d2076104efd1d90d9f73d9744',
 'head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
 'source_sha256': {str(p): sha(p) for p in files},
 'executable_sha256': sha(out / 'unic-desktop'),
 'go_version': subprocess.check_output(['go', 'version'], text=True).strip(),
 'platform': subprocess.check_output(['go', 'env', 'GOOS', 'GOARCH'], text=True).splitlines(),
 'test_command': "go test -race ./app/desktop/internal/desktopdemo -run '^TestQuickPanelE2E$' -count=1 -v",
 'native_window_acceptance': 'NOT RUN',
 'receipt': 'quick-panel-e2e.json',
}
(out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
print('Artifacts: ' + str(out))
PY
