#!/usr/bin/env bash
# Final verification of slice 17c12 (Linux AppImage system proxy) on ONE package built from a clean commit. Every step's complete output and exit code goes to
# <outdir>/logs and <outdir>/steps.txt; nothing is deleted or overwritten (the outdir must be empty); a tag is never reused.
#   run_17c12.sh <outdir>
# Stages: build + package ONE AppImage (provenance: head.txt, status.txt, dirty.diff, hashes) -> content check -> the proxy matrix (Ubuntu and Fedora images, portable and
#         non-portable, the dedicated PAC / two-GUI / dynamic / SOCKS drivers) -> regressions (17c10 helpers, 17c7 TLS, 17c5 portable, update feed).
# The proxy runner's scenario tables are the source of the scenario lists (nothing is hand-copied here). Needs the images of 17c12 (ubuntu, ubuntu-session, ubuntu-session-socks,
# fedora, fedora-session), 17c10 helpers images, 17c7 runtime images, uc-gui-go-linux-build:17c2, bun, Docker.
set -uo pipefail
export UC_LINUX_IMAGE="${UC_LINUX_IMAGE:-uc-gui-go-linux-build:17c2}"
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
R="$ROOT/apps/gui-go/e2e/linux/run.sh"
E2E="$ROOT/apps/gui-go/e2e"
FEED_SRC="${UC_FEED_INPUTS:?UC_FEED_INPUTS: a directory with pubkey.b64 and good.sig.b64 (the 17c5 update feed inputs)}"
JOBS="${UC_JOBS:-4}"
out="${1:?outdir}"
[ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out/logs" "$out/inputs"; out="$(cd "$out" && pwd)"
[ -z "$(git -C "$ROOT" status --porcelain)" ] || { echo "the checkout is dirty: the run must be reproducible from a commit" >&2; exit 2; }
git -C "$ROOT" rev-parse HEAD > "$out/inputs/head.txt"; git -C "$ROOT" status --porcelain > "$out/inputs/status.txt"; git -C "$ROOT" diff HEAD > "$out/inputs/dirty.diff"
[ ! -s "$out/inputs/dirty.diff" ] || { echo "dirty.diff is not empty" >&2; exit 2; }
cp "$FEED_SRC/pubkey.b64" "$FEED_SRC/good.sig.b64" "$out/inputs/" && shasum -a 256 "$out/inputs/pubkey.b64" "$out/inputs/good.sig.b64" > "$out/inputs/feed-inputs.sha256"
step() { local name="$1"; shift; echo "== $name" | tee -a "$out/steps.txt"; "$@" > "$out/logs/$name.log" 2>&1; local rc=$?; echo "   rc=$rc" | tee -a "$out/steps.txt"; return $rc; }
step daemon-sha docker run --rm --platform linux/arm64 -v uc-gui-go-linux-cache:/cache ubuntu:24.04 sh -c 'sha256sum /cache/out-release/uniclipd; head -4 /cache/out-release/build-evidence.txt'
grep -q ea0f0bcb53e949ba94fc71e97748103e4f7136b4a354924991d686cc4d29f6c6 "$out/logs/daemon-sha.log" || { echo "daemon SHA-256 differs from the pinned release daemon" >&2; exit 1; }
for img in uc-gui-go-linux-proxy:17c12-ubuntu uc-gui-go-linux-proxy:17c12-ubuntu-session uc-gui-go-linux-proxy:17c12-ubuntu-session-socks uc-gui-go-linux-proxy:17c12-fedora uc-gui-go-linux-proxy:17c12-fedora-session uc-gui-go-linux-proxy:17c12-fedora-session-socks; do
  docker image inspect "$img" --format '{{.Id}}' > "$out/inputs/image-$(echo "$img" | tr ':/' '__').id" || { echo "missing image $img" >&2; exit 1; }
done
step frontend-e2e bash -c "cd '$ROOT' && VITE_GUI_GO_E2E=1 bun --bun run --cwd apps/gui-go build" || exit 1
step build-gui "$R" release-e2e-build || exit 1
step package-v1 "$R" package-appimage "$out/v1" || exit 1
V1="$out/v1/pkg/E2E-UniClipboard_1.1.1_arm64.AppImage"; M1="$out/v1/pkg/package-manifest.json"
shasum -a 256 "$V1" "$M1" | tee "$out/inputs/package.sha256"
step content-v1 "$R" appimage-content-check "$out/content-v1" "$V1" "$M1"; content=$?

# ---- the proxy matrix: one job per line "name|image|runner|run.sh-args(UC_PROXY_ARGS)|extra"; the lists come from the runner's own tables
python3 -I - "$E2E" > "$out/inputs/matrix.txt" <<'PY'
import sys
sys.path.insert(0, sys.argv[1])
import linux_appimage_proxy_run as m
t = list(m.scenarios())
default = [n for n in t if n not in m.P8_VARIANTS and n not in m.GNOME_VARIANTS]
p8 = [n for n in t if n in m.P8_VARIANTS]
errors = sorted(m.PAC_ERROR_MODES)
pac_single = [n for n in sorted(m.PAC_SCENARIOS) if n not in errors]
plain_gnome = [n for n in t if n in m.GNOME_VARIANTS and n not in m.PAC_SCENARIOS]
groups = [('default', default), ('p8', p8), ('gnome', plain_gnome), ('pac-errors', errors)] + [('pac-' + n, [n]) for n in pac_single]
for dist, imgp, imgn in (('ubuntu', 'ubuntu', 'ubuntu-session'), ('fedora', 'fedora', 'fedora-session')):
    for mode in ('portable', 'nonportable'):
        for g, names in groups:
            if not names:
                continue
            if mode == 'portable' and names == ['gs-sys-pac-owned']:
                continue  # needs a KNOWN bus: covered by the dedicated <dist>-owned-portable job below (final-2 recorded this scheduling as a skip with 0 requirements; that record is kept)
            image = imgn if mode == 'nonportable' else imgp
            flags = ('--nonportable ' if mode == 'nonportable' else '') + '--require --require-env --scenarios ' + ','.join(names)
            print(f'{dist}-{mode}-{g}|uc-gui-go-linux-proxy:17c12-{image}|linux_appimage_proxy_run.py|{flags}|feed')
    sess = f'uc-gui-go-linux-proxy:17c12-{imgn}'
    for name, runner, args in (('two-normal', 'linux_appimage_pac_two_run.py', ''), ('two-kill', 'linux_appimage_pac_two_run.py', '--kill'), ('owned-portable', 'linux_appimage_pac_two_run.py', '--owned'),
                               ('dynamic', 'linux_appimage_proxy_dynamic_run.py', ''), ('guard-downstream', 'linux_appimage_guard_downstream_run.py', '')):
        print(f'{dist}-{name}|{sess}|{runner}|{args}|')
    print(f'{dist}-socks|{sess}-socks|linux_appimage_proxy_dynamic_run.py|--socks|')
PY
run_job() { # name|image|runner|args|feed
  IFS='|' read -r name image runner args feed <<< "$1"
  local d="$out/$name"; mkdir -p "$d"
  [ "$feed" = feed ] && cp -r "$out/inputs/pubkey.b64" "$out/inputs/good.sig.b64" "$d/" && mkdir -p "$d/feed-inputs" && mv "$d/pubkey.b64" "$d/good.sig.b64" "$d/feed-inputs/"
  UC_PROXY_IMAGE="$image" UC_PROXY_RUNNER="$runner" UC_PROXY_ARGS="$args" "$R" appimage-proxy-e2e "$d" "$V1" "$M1" > "$out/logs/$name.stdout" 2>&1
  echo "$name rc=$?" >> "$out/steps.txt"
}
export -f run_job; export out R V1 M1
echo "== proxy matrix ($(wc -l < "$out/inputs/matrix.txt") jobs, $JOBS parallel)" | tee -a "$out/steps.txt"
# NUL-separated, one argument per job (BSD xargs -I limits the replacement string to 255 bytes: the first attempt ran zero jobs)
grep -v '^$' "$out/inputs/matrix.txt" | tr '\n' '\0' | xargs -0 -n1 -P "$JOBS" bash -c 'run_job "$1"' _
njobs=$(grep -vc '^$' "$out/inputs/matrix.txt"); ran=$(grep -c ' rc=' "$out/steps.txt" || true)
recorded=0; while IFS='|' read -r jn _; do [ -z "$jn" ] || { grep -q "^$jn rc=" "$out/steps.txt" && recorded=$((recorded+1)); }; done < "$out/inputs/matrix.txt"
echo "matrix jobs listed=$njobs recorded=$recorded" | tee -a "$out/steps.txt"  # counted BY NAME (the first version's [a-z-] pattern missed the *-p8 jobs)


# ---- regressions on the SAME package
step package-v2 "$R" package-appimage "$out/v2" --update-marker v2-installed || exit 1
V2="$out/v2/pkg/E2E-UniClipboard_1.1.1_arm64.AppImage"; shasum -a 256 "$V1" "$V2" "$V2.tar.gz" | tee "$out/inputs/update-packages.sha256"
step feed "$R" appimage-feed "$out/feed" "$V2.tar.gz" || exit 1
rcs=""
for combo in ubuntu-generic ubuntu-gnome fedora-generic fedora-gnome; do
  d="${combo%-*}"; m="${combo#*-}"
  step "helpers-$combo" env UC_HELPERS_IMAGE="uc-gui-go-linux-runtime-helpers:17c10-$d" UC_HELPERS_DESKTOP="$m" "$R" appimage-helpers-e2e "$out/helpers-$combo" "$V1" "$M1"; rcs="$rcs helpers-$combo=$?"
done
step tls-ubuntu env UC_TLS_IMAGE=uc-gui-go-linux-runtime:17c7 "$R" appimage-tls-e2e "$out/tls-ubuntu" "$V1" "$M1"; tlsu=$?
step tls-fedora env UC_TLS_IMAGE=uc-gui-go-linux-runtime-fedora:17c7 "$R" appimage-tls-e2e "$out/tls-fedora" "$V1" "$M1"; tlsf=$?
step e2e-portable "$R" appimage-portable-e2e "$out/e2e-portable" "$V1" "$out/feed" "$M1"; portable=$?
echo "regress:$rcs content=$content tls-ubuntu=$tlsu tls-fedora=$tlsf portable=$portable" | tee -a "$out/steps.txt"
( cd "$out" && find . -type f \( -name '*.AppImage' -o -name 'package-manifest.json' -o -name 'appimage-assertions.json' -o -name 'content-check.json' \) -not -path '*/squashfs-root/*' -print0 | sort -z | xargs -0 shasum -a 256 > SHA256SUMS.txt )
# exit status: 0 only if every assertion file says functionalPassed (proxy jobs) or passed (regressions)
bad=0
for f in "$out"/*/appimage-assertions.json; do
  d="$(basename "$(dirname "$f")")"
  case "$d" in helpers-*|tls-*|e2e-portable) key=passed;; *) key=functionalPassed;; esac
  python3 -I -c "import json,sys;sys.exit(0 if json.load(open(sys.argv[1])).get(sys.argv[2]) is True else 1)" "$f" "$key" || { echo "NOT PASSED ($key): $f" | tee -a "$out/steps.txt"; bad=1; }
done
python3 -I -c "import json,sys;sys.exit(0 if json.load(open(sys.argv[1])).get('passed') is True else 1)" "$out/content-v1/content-check.json" || { echo "NOT PASSED: content-v1" | tee -a "$out/steps.txt"; bad=1; }
exit $bad
