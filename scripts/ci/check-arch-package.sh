#!/usr/bin/env bash
# Install a built uniclipboard-git package in a clean Arch container and launch the shipped form headlessly.
#
#   check-arch-package.sh <package.pkg.tar.zst> <output-dir>
#
# Run as root inside a fresh `archlinux:base-devel` container that has NOT built the package (so the runtime
# dependencies the package declares are the only ones present). Container evidence only: Xvfb, an unlocked throwaway
# gnome-keyring as the Secret Service, no desktop environment, no login session. Every assertion is a line in
# <output-dir>/results.tsv (PASS|FAIL<TAB>name<TAB>detail); the exit status is the number of failures.
set -uo pipefail
PKG="$(readlink -f "$1")"; OUT="$2"
[[ "$(id -u)" == 0 ]] || { echo "run as root in a disposable container" >&2; exit 2; }
mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"
results="$OUT/results.tsv"; : >"$results"; failures=0
ok()  { printf 'PASS\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; }
bad() { printf 'FAIL\t%s\t%s\n' "$1" "${2:-}" | tee -a "$results"; failures=$((failures + 1)); }
check() { local name="$1"; shift; local rc=0; "$@" >/tmp/check.out 2>&1 || rc=$?; { printf '\n%s: ' "$name"; printf '%q ' "$@"; printf '\n'; cat /tmp/check.out; } >>"$OUT/commands.log"
  if [[ "$rc" == 0 ]]; then ok "$name"; else bad "$name" "$(tr '\n' ' ' </tmp/check.out | cut -c1-300)"; fi; }

if [[ "${UC_PACMAN_DISABLE_SANDBOX:-0}" == 1 ]]; then sed -i '/^\[options\]/a DisableSandbox' /etc/pacman.conf; fi
{ . /etc/os-release; echo "image: $PRETTY_NAME"; echo "machine: $(uname -m)"; sha256sum "$PKG"; } | tee "$OUT/host.txt"

# Harness tools first, then the package: they must not be what satisfies the package's own dependencies.
check "harness tools install" pacman -Syu --noconfirm --needed xorg-server-xvfb xorg-xdpyinfo dbus gnome-keyring libsecret procps-ng desktop-file-utils
check "package installs with its declared dependencies" pacman -U --noconfirm "$PKG"
name="$(pacman -Qpq "$PKG")"
check "one package owns the executable" bash -c "[ \"\$(pacman -Qoq /usr/bin/uniclipboard)\" = '$name' ]"
check "installed files match the package" pacman -Qkk "$name"
check "no unresolved library in the GUI executable" bash -c '! ldd /usr/bin/uniclipboard | grep "not found"'
check "no unresolved library in the daemon" bash -c '! ldd /usr/bin/uniclipd | grep "not found"'
check "the desktop entry is valid" desktop-file-validate /usr/share/applications/uniclipboard.desktop
check "the icon named by the desktop entry is installed" test -f /usr/share/icons/hicolor/128x128/apps/uniclipboard.png

# Shipped form: no profile, the real data root of this throwaway root user, the Secret Service of an unlocked throwaway keyring.
export HOME=/root; mkdir -p /root/.local/share /root/.config /root/.cache
dbus-run-session -- bash -c '
  set -u
  printf "uc-throwaway" | gnome-keyring-daemon --foreground --unlock --components=secrets >/tmp/keyring.env 2>/tmp/keyring.err &
  for i in $(seq 100); do dbus-send --session --dest=org.freedesktop.DBus --print-reply /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q "boolean true" && break; sleep 0.1; done
  printf "probe" | secret-tool store --label=uc-probe uc probe && [ "$(secret-tool lookup uc probe)" = probe ] && secret-tool clear uc probe && echo keyring-ready
  trap "kill ${gui:-} ${xvfb:-} 2>/dev/null; pkill -x uniclipd 2>/dev/null || true" EXIT
  Xvfb :99 -screen 0 1280x800x24 -nolisten tcp & xvfb=$!
  export DISPLAY=:99; for i in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
  /usr/bin/uniclipboard >/tmp/gui.log 2>&1 & gui=$!
  daemon=""; webkit=""
  for i in $(seq 120); do
    daemon=$(pgrep -x uniclipd | head -1); webkit=$(pgrep -f "[W]ebKitWebProcess" | head -1)
    [ -n "$daemon" ] && [ -n "$webkit" ] && break; sleep 1
  done
  sleep 15
  { echo "gui_exe=$(readlink /proc/$gui/exe 2>/dev/null)"; echo "daemon_exe=$(readlink /proc/$daemon/exe 2>/dev/null)"; echo "webkit_exe=$(readlink /proc/$webkit/exe 2>/dev/null)"
    echo "gui_alive=$(kill -0 $gui 2>/dev/null && echo yes || echo no)"; echo "daemon_alive=$([ -n "$daemon" ] && kill -0 $daemon 2>/dev/null && echo yes || echo no)"
    echo "daemon_conn=$(ls /root/.local/share/*/daemon.conn /root/.local/share/*/*/daemon.conn 2>/dev/null | head -1)"; } >/tmp/launch.txt
  ps -eo pid,ppid,etime,args | grep -E "uniclip|WebKit" | grep -v grep >/tmp/launch-ps.txt
  kill $gui 2>/dev/null; sleep 3; echo "daemon_after_gui_exit=$(pgrep -x uniclipd >/dev/null && echo running || echo gone)" >>/tmp/launch.txt
  pkill -x uniclipd 2>/dev/null; for i in $(seq 50); do pgrep -x uniclipd >/dev/null || break; sleep .2; done; kill $xvfb 2>/dev/null; true' >"$OUT/launch-stdout.txt" 2>&1
cp /tmp/gui.log /tmp/launch.txt /tmp/launch-ps.txt "$OUT/" 2>/dev/null || true
# shellcheck disable=SC1091
. /tmp/launch.txt 2>/dev/null || true
grep -q '^keyring-ready$' "$OUT/launch-stdout.txt" && ok "the Secret Service of the harness is ready" || bad "the Secret Service of the harness is ready"
[[ "${gui_exe:-}" == /usr/bin/uniclipboard ]] && ok "GUI runs from the installed path" || bad "GUI runs from the installed path" "exe=${gui_exe:-none}"
[[ "${daemon_exe:-}" == /usr/bin/uniclipd ]] && ok "daemon runs from the installed path" || bad "daemon runs from the installed path" "exe=${daemon_exe:-none}"
case "${webkit_exe:-}" in /usr/lib/webkit2gtk-4.1/WebKitWebProcess | /usr/lib/*/webkit2gtk-4.1/WebKitWebProcess) ok "WebView process runs from the installed WebKitGTK" "$webkit_exe" ;; *) bad "WebView process runs from the installed WebKitGTK" "exe=${webkit_exe:-none}" ;; esac
[[ "${gui_alive:-}" == yes && "${daemon_alive:-}" == yes ]] && ok "GUI and daemon are still alive 15 s after the WebView appeared" || bad "GUI and daemon are still alive 15 s after the WebView appeared" "gui=${gui_alive:-?} daemon=${daemon_alive:-?}"
[[ -n "${daemon_conn:-}" ]] && ok "daemon published its connection file" "$daemon_conn" || bad "daemon published its connection file" "none under /root/.local/share"
exit "$failures"
