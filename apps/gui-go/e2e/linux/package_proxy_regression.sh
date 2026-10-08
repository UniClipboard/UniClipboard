#!/usr/bin/env bash
# System-proxy / PAC regression of an AppImage built in the packaging image, on a host distribution (docs/architecture/gui-go-linux-ci-packaging.md, "Debian 12 build").
# Runs the existing 17c12 proxy matrix (the runner's own scenario tables are the source of the scenario lists) against ONE E2E-prefixed AppImage.
#
#   package_proxy_regression.sh <outdir> <E2E AppImage> <package-manifest.json> <feed dir with pubkey.b64 and good.sig.b64> <proxy image prefix> [dist]
#
# <proxy image prefix> names the images `<prefix>` (portable), `<prefix>-session` (non-portable, with an unlocked Secret Service) and `<prefix>-session-socks`,
# built from Dockerfile.17c12-ubuntu / -session / -session-socks with BASE_IMAGE = a clean-host image. UC_JOBS sets the parallelism (default 3).
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
R="$ROOT/apps/gui-go/e2e/linux/run.sh"; E2E="$ROOT/apps/gui-go/e2e"
out="${1:?outdir}"; V1="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"; M1="$(cd "$(dirname "$3")" && pwd)/$(basename "$3")"; FEED="$(cd "$4" && pwd)"; PFX="${5:?image prefix}"; DIST="${6:-ubuntu}"
JOBS="${UC_JOBS:-3}"
[ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out/logs" "$out/inputs"; out="$(cd "$out" && pwd)"
cp "$FEED/pubkey.b64" "$FEED/good.sig.b64" "$out/inputs/"
python3 -I - "$E2E" "$PFX" "$DIST" > "$out/inputs/matrix.txt" <<'PY'
import sys
sys.path.insert(0, sys.argv[1]); pfx, dist = sys.argv[2], sys.argv[3]
import linux_appimage_proxy_run as m
t = list(m.scenarios())
default = [n for n in t if n not in m.P8_VARIANTS and n not in m.GNOME_VARIANTS]
p8 = [n for n in t if n in m.P8_VARIANTS]
errors = sorted(m.PAC_ERROR_MODES)
pac_single = [n for n in sorted(m.PAC_SCENARIOS) if n not in errors]
plain_gnome = [n for n in t if n in m.GNOME_VARIANTS and n not in m.PAC_SCENARIOS]
groups = [('default', default), ('p8', p8), ('gnome', plain_gnome), ('pac-errors', errors)] + [('pac-' + n, [n]) for n in pac_single]
for mode in ('portable', 'nonportable'):
    for g, names in groups:
        if not names or (mode == 'portable' and names == ['gs-sys-pac-owned']):
            continue
        image = pfx + ('-session' if mode == 'nonportable' else '')
        flags = ('--nonportable ' if mode == 'nonportable' else '') + '--require --require-env --scenarios ' + ','.join(names)
        print(f'{dist}-{mode}-{g}|{image}|linux_appimage_proxy_run.py|{flags}|feed')
sess = pfx + '-session'
for name, runner, args in (('two-normal', 'linux_appimage_pac_two_run.py', ''), ('two-kill', 'linux_appimage_pac_two_run.py', '--kill'), ('owned-portable', 'linux_appimage_pac_two_run.py', '--owned'),
                           ('dynamic', 'linux_appimage_proxy_dynamic_run.py', ''), ('guard-downstream', 'linux_appimage_guard_downstream_run.py', '')):
    print(f'{dist}-{name}|{sess}|{runner}|{args}|')
print(f'{dist}-socks|{sess}-socks|linux_appimage_proxy_dynamic_run.py|--socks|')
PY
# UC_MATRIX_FILTER (an extended regex on the job line) runs a subset, e.g. 'pac' for the PAC scenarios only; the full matrix is the default.
[ -z "${UC_MATRIX_FILTER:-}" ] || { grep -E "$UC_MATRIX_FILTER" "$out/inputs/matrix.txt" > "$out/inputs/matrix.filtered.txt" && mv "$out/inputs/matrix.filtered.txt" "$out/inputs/matrix.txt"; }
run_job() {
  IFS='|' read -r name image runner args feed <<< "$1"
  local d="$out/$name"; mkdir -p "$d"
  [ "$feed" = feed ] && mkdir -p "$d/feed-inputs" && cp "$out/inputs/pubkey.b64" "$out/inputs/good.sig.b64" "$d/feed-inputs/"
  UC_PROXY_IMAGE="$image" UC_PROXY_RUNNER="$runner" UC_PROXY_ARGS="$args" "$R" appimage-proxy-e2e "$d" "$V1" "$M1" > "$out/logs/$name.stdout" 2>&1
  echo "$name rc=$?" >> "$out/steps.txt"
}
export -f run_job; export out R V1 M1
echo "jobs: $(grep -vc '^$' "$out/inputs/matrix.txt")" | tee -a "$out/steps.txt"
grep -v '^$' "$out/inputs/matrix.txt" | tr '\n' '\0' | xargs -0 -n1 -P "$JOBS" bash -c 'run_job "$1"' _
echo "failed: $(grep -c ' rc=[1-9]' "$out/steps.txt")" | tee -a "$out/steps.txt"
