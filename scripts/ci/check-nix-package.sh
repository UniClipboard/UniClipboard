#!/usr/bin/env bash
# Check a built Nix package of UniClipboard (packaging/nix) and launch its shipped form headlessly.
#
#   check-nix-package.sh <package-output-path> <evidence-dir>
#
# Runs on a disposable CI runner that has Nix and the harness tools (xvfb, xdpyinfo, dbus, gnome-keyring, libsecret-tools,
# desktop-file-utils, procps). Container evidence only: Xvfb, an unlocked throwaway gnome-keyring as the Secret Service, no
# desktop environment, no login session, and a throwaway HOME so nothing outside <evidence-dir> is read or written. A host
# without NixOS has no GPU driver directory for Nix's libraries, so a graphics failure here is a harness limit, not proof
# about NixOS. Every assertion is a line in <evidence-dir>/results.tsv (PASS|FAIL<TAB>name<TAB>detail); the exit status is
# the number of failures.
set -uo pipefail
PKG="$(readlink -f "$1")"; OUT="$2"
mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
results="$OUT/results.tsv"; : >"$results"; failures=0
ok()  { printf 'PASS\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; }
bad() { printf 'FAIL\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; failures=$((failures + 1)); }
check() { local name="$1"; shift; local rc=0; "$@" >"$OUT/check.out" 2>&1 || rc=$?; { printf '\n%s: ' "$name"; printf '%q ' "$@"; printf '\n'; cat "$OUT/check.out"; } >>"$OUT/commands.log"
  if [[ "$rc" == 0 ]]; then ok "$name"; else bad "$name" "$(tr '\n' ' ' <"$OUT/check.out" | cut -c1-300)"; fi; }

{ echo "package: $PKG"; uname -srm; . /etc/os-release; echo "host: $PRETTY_NAME"; nix --version; } | tee "$OUT/host.txt"

gui_real="$PKG/bin/.uniclipboard-wrapped"
daemon="$PKG/bin/uniclipd"

# Layout: the wrapper starts the GUI; the daemon is a plain ELF next to the real GUI executable, because the host accepts a
# running daemon only when /proc/<pid>/exe is called `uniclipd` and looks for it beside os.Executable().
check "the GUI wrapper is executable" test -x "$PKG/bin/uniclipboard"
check "the real GUI executable sits beside the daemon" test -x "$gui_real"
check "the daemon is an unwrapped ELF" bash -c "[ \"\$(head -c4 '$daemon' | od -An -c | tr -d ' ')\" = '177ELF' ]"
# Resolve with the loader the binary itself requests (the Nix glibc). The host's ldd would use the host loader against Nix libraries
# and report a GLIBC version mismatch that does not exist at run time.
unresolved() {
  local interp out
  interp="$(readelf -l "$1" | sed -n 's/.*Requesting program interpreter: \(.*\)\]/\1/p')"
  test -x "$interp" || { echo "interpreter not executable: $interp"; return 1; }
  out="$("$interp" --list "$1" 2>&1)" || { echo "$out"; return 1; }
  ! grep 'not found' <<<"$out"
}
check "no unresolved library in the GUI executable" unresolved "$gui_real"
check "no unresolved library in the daemon" unresolved "$daemon"
# The ELF ABI is authoritative; only the exact pre-migration release still uses GTK3.
if readelf -d "$gui_real" | grep -q 'libgtk-4.so.1'; then
  check "the GUI links WebKitGTK 6.0" bash -c 'readelf -d "$1" | grep -q "libwebkitgtk-6.0.so.4"' _ "$gui_real"
  check "the GTK4 GUI has no GTK3 dependencies" bash -c '! readelf -d "$1" | grep -Eq "libgtk-3|libwebkit2gtk-4.1"' _ "$gui_real"
  webkit_abi=6.0
else
  # TODO: Remove with the exact GTK3 pin in packaging/nix/sources.json.
  layer_dir="$(readelf -d "$gui_real" | sed -n 's/.*RUNPATH.*\[\(.*\)\]/\1/p' | tr ':' '\n' | grep -- '-gtk-layer-shell-' | head -1)"
  check "legacy release Layer Shell library is reachable" test -e "${layer_dir:-/nonexistent}/libgtk-layer-shell.so.0"
  webkit_abi=4.1
fi
check "the desktop entry is valid" desktop-file-validate "$PKG/share/applications/uniclipboard.desktop"
check "the icon named by the desktop entry is installed" test -f "$PKG/share/icons/hicolor/128x128/apps/uniclipboard.png"

# Shipped form: no UC_PROFILE, so the data root is the app directory under the throwaway HOME.
home="$(mktemp -d /tmp/ucnix.XXXXXX)"; mkdir -p "$home/.local/share" "$home/.config" "$home/.cache"
export HOME="$home" XDG_DATA_HOME="$home/.local/share" XDG_CONFIG_HOME="$home/.config" XDG_CACHE_HOME="$home/.cache" XDG_STATE_HOME="$home/.local/state"
export XDG_RUNTIME_DIR="$home/run"; mkdir -m700 "$XDG_RUNTIME_DIR"
# Harness only: a runner without NixOS has no /run/opengl-driver for Nix's libglvnd, so WebKitGTK gets no EGL display. Software
# painting keeps the WebView alive; this says nothing about GPU rendering on NixOS.
export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 LIBGL_ALWAYS_SOFTWARE=1
export PKG
dbus-run-session -- bash -c '
  set -u
  printf "uc-throwaway" | gnome-keyring-daemon --foreground --unlock --components=secrets >"$HOME/keyring.env" 2>"$HOME/keyring.err" &
  for i in $(seq 100); do dbus-send --session --dest=org.freedesktop.DBus --print-reply /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q "boolean true" && break; sleep 0.1; done
  printf "probe" | secret-tool store --label=uc-probe uc probe && [ "$(secret-tool lookup uc probe)" = probe ] && secret-tool clear uc probe && echo keyring-ready
  trap "kill \${gui:-} \${xvfb:-} 2>/dev/null; pkill -u \$(id -u) -x uniclipd 2>/dev/null || true" EXIT
  Xvfb :99 -screen 0 1280x800x24 -nolisten tcp & xvfb=$!
  export DISPLAY=:99; for i in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
  "$PKG/bin/uniclipboard" >"$HOME/gui.log" 2>&1 & gui=$!
  daemon=""; webkit=""
  for i in $(seq 120); do
    daemon=$(pgrep -u $(id -u) -x uniclipd | head -1); webkit=$(pgrep -u $(id -u) -f "[W]ebKitWebProcess" | head -1)
    [ -n "$daemon" ] && [ -n "$webkit" ] && break; sleep 1
  done
  sleep 15
  # The wrapper execs the real executable with the wrapper path as argv[0], so find the GUI by its /proc/<pid>/exe.
  gui_pid=""; for p in $(pgrep -u $(id -u) -f uniclipboard); do [ "$(readlink /proc/$p/exe 2>/dev/null)" = "$PKG/bin/.uniclipboard-wrapped" ] && gui_pid=$p; done
  { echo "gui_exe=$(readlink /proc/${gui_pid:-0}/exe 2>/dev/null)"; echo "daemon_exe=$(readlink /proc/${daemon:-0}/exe 2>/dev/null)"; echo "webkit_exe=$(readlink /proc/${webkit:-0}/exe 2>/dev/null)"
    echo "gui_alive=$([ -n "$gui_pid" ] && kill -0 $gui_pid 2>/dev/null && echo yes || echo no)"; echo "daemon_alive=$([ -n "$daemon" ] && kill -0 $daemon 2>/dev/null && echo yes || echo no)"
    echo "daemon_conn=$(ls "$XDG_DATA_HOME"/*/daemon.conn "$XDG_DATA_HOME"/*/*/daemon.conn 2>/dev/null | head -1)"; } >"$HOME/launch.txt"
  xwininfo -root -tree >"$HOME/windows.txt" 2>&1
  ps -eo pid,ppid,etime,args | grep -E "uniclip|WebKit" | grep -v grep >"$HOME/launch-ps.txt"
  kill $gui 2>/dev/null; sleep 3; echo "daemon_after_gui_exit=$(pgrep -u $(id -u) -x uniclipd >/dev/null && echo running || echo gone)" >>"$HOME/launch.txt"
  pkill -u $(id -u) -x uniclipd 2>/dev/null; for i in $(seq 50); do pgrep -u $(id -u) -x uniclipd >/dev/null || break; sleep .2; done; kill $xvfb 2>/dev/null; true' >"$OUT/launch-stdout.txt" 2>&1
cp "$home/gui.log" "$home/launch.txt" "$home/launch-ps.txt" "$OUT/" 2>/dev/null || true
# shellcheck disable=SC1091
. "$OUT/launch.txt" 2>/dev/null || true
grep -q '^keyring-ready$' "$OUT/launch-stdout.txt" && ok "the Secret Service of the harness is ready" || bad "the Secret Service of the harness is ready"
[[ "${gui_exe:-}" == "$gui_real" ]] && ok "GUI runs from the package" || bad "GUI runs from the package" "exe=${gui_exe:-none}"
[[ "${daemon_exe:-}" == "$daemon" ]] && ok "daemon runs from the package" || bad "daemon runs from the package" "exe=${daemon_exe:-none}"
case "${webkit_exe:-}" in /nix/store/*-webkitgtk-*/libexec/webkitgtk-6.0/WebKitWebProcess | /nix/store/*-webkitgtk-*/libexec/webkit2gtk-4.1/WebKitWebProcess) ok "WebView process runs from the package's WebKitGTK" "$webkit_exe" ;; *) bad "WebView process runs from the package's WebKitGTK" "exe=${webkit_exe:-none}" ;; esac
case "$webkit_abi:${webkit_exe:-}" in
  6.0:*/webkitgtk-6.0/WebKitWebProcess | 4.1:*/webkit2gtk-4.1/WebKitWebProcess) ok "WebKit helper ABI matches the GUI" ;;
  *) bad "WebKit helper ABI matches the GUI" ;;
esac
[[ "${gui_alive:-}" == yes && "${daemon_alive:-}" == yes ]] && ok "GUI and daemon are still alive 15 s after the WebView appeared" || bad "GUI and daemon are still alive 15 s after the WebView appeared" "gui=${gui_alive:-?} daemon=${daemon_alive:-?}"
# A live process alone also passes with no window at all: require a mapped top-level window titled UniClipboard. (The AppImage smoke's
# "Main window revealed" log line is a Tauri-era signal that the Go host never logs.)
cp "$home/windows.txt" "$OUT/" 2>/dev/null || true
sed 's/\x1b\[[0-9;]*m//g' "$OUT/gui.log" >"$OUT/gui.clean.log" 2>/dev/null || : >"$OUT/gui.clean.log"
grep -a -q '"UniClipboard"' "$OUT/windows.txt" 2>/dev/null && ok "a top-level window titled UniClipboard exists" || bad "a top-level window titled UniClipboard exists" "see windows.txt"
fatal='Segmentation fault|undefined symbol|Failed to load module|symbol lookup error|Failed to initialize GTK|panicked|Main window readiness timed out|No GSettings schemas are installed|schema .* is not installed'
fatal_hits="$(grep -a -E "$fatal" "$OUT/gui.clean.log" | cut -c1-200 | sort | uniq -c)"
[[ -z "$fatal_hits" ]] && ok "no loader, GTK or GSettings error in the GUI log" || bad "no loader, GTK or GSettings error in the GUI log" "$(echo "$fatal_hits" | tr '\n' ' ' | cut -c1-300)"
# Informational only: this runner is not NixOS, so Nix's libglvnd finds no driver (see the harness note above).
ok "informational: EGL error lines in the GUI log" "$(grep -a -c -E 'EGL_BAD_PARAMETER|Could not create default EGL display' "$OUT/gui.clean.log" 2>/dev/null || echo 0)"
[[ -n "${daemon_conn:-}" ]] && ok "daemon published its connection file under the throwaway HOME" "$daemon_conn" || bad "daemon published its connection file under the throwaway HOME" "none under $XDG_DATA_HOME"
rm -rf "${home:?}"
exit "$failures"
