#!/usr/bin/env bash
# 17c6: fail-closed evidence for the pinned AppImage runtime. Every case runs the REAL packager (run.sh package-appimage, the same
# container, daemon evidence and GUI binary as the positive packages) in a NEW directory with its own tools directory, and asserts:
# exit code != 0, no .AppImage was produced, the log names the reason, and (cache/download cases) the rejected bytes were kept.
# Bad inputs are made by changing the INPUT (a cached runtime file) or, for a bad pin, by bind-mounting a one-line-mutated COPY of
# package_linux.py over the container's copy (the checkout stays clean and the packager has no bypass option).
#   runtime_pin_negative.sh <outdir>      needs the pinned runtime cached by an earlier package step (/cache/tools in the cache volume)
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
R="$ROOT/apps/gui-go/e2e/linux/run.sh"
out="${1:?outdir}"; [ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out"; out="$(cd "$out" && pwd)"
export UC_LINUX_IMAGE="${UC_LINUX_IMAGE:-uc-gui-go-linux-build:17c2}"
SRC="$ROOT/apps/gui-go/e2e/package_linux.py"
BASE=https://github.com/AppImage/type2-runtime/releases/download/continuous
fails=0; total=0
# seed <dir>: copy the SHA-verified appimagetool/linuxdeploy/plugin and the pinned runtime (as good-runtime) out of the cache volume
seed() { docker run --rm -v uc-gui-go-linux-cache:/cache:ro -v "$1:/dst" "$UC_LINUX_IMAGE" bash -c 'cp /cache/tools/appimagetool /cache/tools/linuxdeploy /cache/tools/linuxdeploy-plugin-gtk.sh /dst/ && cp /cache/tools/appimage-runtime-aarch64 /dst/good-runtime && chmod -R a+rwX /dst'; }
mk() { mkdir -p "$out/$1/tools"; seed "$out/$1/tools"; }
# run_case <name> <expected log substring> <expect a .rejected-* file: yes|no> <extra docker args>
run_case() {
  local name="$1" expect="$2" rejected="$3" dockerargs="$4"; total=$((total+1))
  mkdir -p "$out/$name/pkg-out"
  UC_PACKAGE_TOOLS="$out/$name/tools" UC_PACKAGE_DOCKER_ARGS="$dockerargs" "$R" package-appimage "$out/$name/pkg-out" > "$out/$name/stdout.log" 2>&1; local rc=$?
  local produced rej ok=1
  produced="$(find "$out/$name/pkg-out" -name '*.AppImage' | wc -l | tr -d ' ')"
  rej="$(ls "$out/$name/tools" | grep -c 'rejected-' || true)"
  [ "$rc" != 0 ] || ok=0
  [ "$produced" = 0 ] || ok=0
  grep -qF -- "$expect" "$out/$name/stdout.log" || ok=0
  if [ "$rejected" = yes ]; then [ "$rej" -ge 1 ] || ok=0; fi
  echo "case=$name rc=$rc appimages=$produced rejected_files=$rej expect='$expect' -> $([ $ok = 1 ] && echo PASS || echo FAIL)" | tee -a "$out/summary.txt"
  [ $ok = 1 ] || fails=$((fails+1))
}
mutate() { python3 -I - "$SRC" "$1" "$2" "$3" <<'PY'
import sys
src, dst, old, new = sys.argv[1:5]
s = open(src).read()
assert s.count(old) == 1, old
open(dst, 'w').write(s.replace(old, new))
PY
}
OFFLINE="--network none"
# 1 cached runtime is not an ELF file
mk n1-cache-garbage; printf 'this is not a runtime\n' > "$out/n1-cache-garbage/tools/appimage-runtime-aarch64"
run_case n1-cache-garbage "ELF e_machine None, expected 183" yes "$OFFLINE"
# 2 cached runtime is a real runtime of the WRONG architecture (the x86_64 asset under the aarch64 name)
mk n2-cache-wrong-arch; curl -fsSL -o "$out/n2-cache-wrong-arch/tools/appimage-runtime-aarch64" "$BASE/runtime-x86_64"
run_case n2-cache-wrong-arch "ELF e_machine 62, expected 183" yes "$OFFLINE"
# 3 cached runtime truncated (valid ELF header, wrong bytes)
mk n3-cache-truncated; head -c 900000 "$out/n3-cache-truncated/tools/good-runtime" > "$out/n3-cache-truncated/tools/appimage-runtime-aarch64"
run_case n3-cache-truncated "differs from the pinned b4ff0030" yes "$OFFLINE"
# 4 cached runtime with one flipped byte
mk n4-cache-bitflip; python3 -I - "$out/n4-cache-bitflip/tools" <<'PY'
import sys
d = sys.argv[1]
b = bytearray(open(d + '/good-runtime', 'rb').read()); b[500000] ^= 1
open(d + '/appimage-runtime-aarch64', 'wb').write(b)
PY
run_case n4-cache-bitflip "differs from the pinned b4ff0030" yes "$OFFLINE"
# 5/6 a bad pin (mutated copy of the packager, online, empty runtime cache): wrong SHA for the real arm64 runtime; wrong-architecture asset behind the arm64 pin
mk n5-bad-pin-sha; rm "$out/n5-bad-pin-sha/tools/good-runtime"
mutate "$out/n5-bad-pin-sha/package_linux.py" "b4ff0030242d0c3bb12ce40541828303cf167493f4793456f0436edd6255c39d" "b4ff0030242d0c3bb12ce40541828303cf167493f4793456f0436edd6255c39e"
run_case n5-bad-pin-sha "differs from the pinned b4ff0030242d0c3bb12ce40541828303cf167493f4793456f0436edd6255c39e" yes "-v $out/n5-bad-pin-sha/package_linux.py:/work/apps/gui-go/e2e/package_linux.py:ro"
mk n6-bad-pin-arch; rm "$out/n6-bad-pin-arch/tools/good-runtime"
mutate "$out/n6-bad-pin-arch/package_linux.py" "('runtime-aarch64', 'b4ff0030242d0c3bb12ce40541828303cf167493f4793456f0436edd6255c39d', 183)" "('runtime-x86_64', '156f4bdbde9c52d01814600013e0a273f0118dc2de98975f3c8c63427ec79074', 183)"
run_case n6-bad-pin-arch "ELF e_machine 62, expected 183" yes "-v $out/n6-bad-pin-arch/package_linux.py:/work/apps/gui-go/e2e/package_linux.py:ro"
# 7 no runtime in the cache AND no network: nothing is packaged (no implicit fallback to a download inside appimagetool)
mk n7-offline-empty-cache; rm "$out/n7-offline-empty-cache/tools/good-runtime"
run_case n7-offline-empty-cache "cannot download the pinned input" no "$OFFLINE"
echo "negative cases: $((total-fails))/$total" | tee -a "$out/summary.txt"
# 8 (positive) a corrupt cached runtime WITH network: the corrupt file is moved aside (kept), the pinned bytes are fetched again and
# verified, and the package succeeds with the pinned runtime embedded: healing is never an acceptance of the bad bytes.
mk h1-cache-bitflip-heals-online; python3 -I - "$out/h1-cache-bitflip-heals-online/tools" <<'PY'
import sys
d = sys.argv[1]
b = bytearray(open(d + '/good-runtime', 'rb').read()); b[500000] ^= 1
open(d + '/appimage-runtime-aarch64', 'wb').write(b)
PY
mkdir -p "$out/h1-cache-bitflip-heals-online/pkg-out"
UC_PACKAGE_TOOLS="$out/h1-cache-bitflip-heals-online/tools" "$R" package-appimage "$out/h1-cache-bitflip-heals-online/pkg-out" > "$out/h1-cache-bitflip-heals-online/stdout.log" 2>&1; hrc=$?
good="$(shasum -a 256 "$out/h1-cache-bitflip-heals-online/tools/good-runtime" | cut -d' ' -f1)"; healed="$(shasum -a 256 "$out/h1-cache-bitflip-heals-online/tools/appimage-runtime-aarch64" | cut -d' ' -f1)"
hrej="$(ls "$out/h1-cache-bitflip-heals-online/tools" | grep -c 'rejected-' || true)"
hok=1; [ "$hrc" = 0 ] && [ "$good" = "$healed" ] && [ "$hrej" -ge 1 ] || hok=0
echo "case=h1-cache-bitflip-heals-online rc=$hrc healed_equals_pin=$([ "$good" = "$healed" ] && echo yes || echo no) rejected_files=$hrej -> $([ $hok = 1 ] && echo PASS || echo FAIL)" | tee -a "$out/summary.txt"
[ "$fails" = 0 ] && [ "$hok" = 1 ]
