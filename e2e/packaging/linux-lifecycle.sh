#!/bin/sh
# Run only inside a disposable Debian-family container, with fixture packages.
set -eu
package=$1
newer=$2
receipt=$3
command -v dpkg >/dev/null
package=$(realpath "$package")
newer=$(realpath "$newer")
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export HOME="$work/home" XDG_CONFIG_HOME="$work/config"
mkdir -p "$HOME" "$XDG_CONFIG_HOME/unic"
printf 'fixture-user-data\n' > "$XDG_CONFIG_HOME/unic/preserve.txt"
dpkg --compare-versions 2.0.0-2 gt 2.0.0-1
dpkg --compare-versions 2.0.1-1 gt 2.0.0-999999999
apt-get update >/dev/null
apt-get install -y "$package" >/dev/null
/usr/lib/uniclipboard/unic-desktop --version --json > "$work/desktop.json"
/usr/lib/uniclipboard/unic --version --json > "$work/cli.json"
/usr/lib/uniclipboard/unic status --json > "$work/status.json"
test ! -e "$XDG_CONFIG_HOME/unic/unic.sock"
test -L /usr/bin/uniclipboard
test ! -e /usr/bin/unic
dpkg-query -L uniclipboard > "$work/files.txt"
# A second install exercises same-build idempotency without a GUI.
apt-get install --reinstall -y "$package" >/dev/null
apt-get install -y "$newer" >/dev/null
dpkg-query -W -f='${Version}' uniclipboard | tee "$work/installed-version.txt"
test "$(cat "$work/installed-version.txt")" = 2.0.0-2
apt-get remove -y uniclipboard >/dev/null
test -f "$XDG_CONFIG_HOME/unic/preserve.txt"
test ! -e /usr/lib/uniclipboard/unic-desktop
mkdir -p "$receipt"
cp "$work/desktop.json" "$work/cli.json" "$work/status.json" "$work/files.txt" "$work/installed-version.txt" "$receipt/"
python3 - "$package" "$newer" "$receipt/report.json" <<'PYREPORT'
import hashlib, json, platform, sys
from pathlib import Path
files=[Path(sys.argv[1]),Path(sys.argv[2])]
Path(sys.argv[3]).write_text(json.dumps({
    "status":"passed", "format":"deb", "scope":"DEB nonvisual lifecycle",
    "target":"linux-"+{"aarch64":"arm64","x86_64":"amd64"}[platform.machine()],
    "artifacts_sha256":[hashlib.sha256(p.read_bytes()).hexdigest() for p in files],
    "assertions":["installation","reinstallation","upgrade","removal","data_preservation","private_cli_ownership","temporary_service_shutdown"],
    "gui":"not_run", "v1_replacement":"not_run"
},indent=2)+"\n")
PYREPORT
(cd "$receipt" && sha256sum *.json files.txt installed-version.txt > SHA256SUMS)
