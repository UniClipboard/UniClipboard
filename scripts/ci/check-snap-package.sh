#!/usr/bin/env bash
# Install and launch the GTK4 Snap on a disposable GitHub runner, without disabling WebKit's sandbox.
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true ]] || { echo 'Run only on a disposable GitHub runner' >&2; exit 2; }
OUT="$(mkdir -p "${2:?evidence directory}" && cd "$2" && pwd)"
if [[ "$1" != --session ]]; then
  PACKAGE="$(readlink -f "$1")"
  sudo apt-get update -qq
  sudo apt-get install -y --no-install-recommends xvfb x11-utils dbus-x11 gnome-keyring libsecret-tools
  for provider in core24 gnome-46-2404 gtk-common-themes mesa-2404; do
    snap list "$provider" >/dev/null 2>&1 || sudo snap install "$provider"
  done
  sudo snap install "$PACKAGE" --dangerous
  sudo snap connect uniclipboard:password-manager-service
  sudo snap connect uniclipboard:gnome-46-2404 gnome-46-2404:gnome-46-2404
  sudo snap connect uniclipboard:gpu-2404 mesa-2404:gpu-2404
  for theme in gtk-3-themes icon-themes sound-themes; do
    sudo snap connect "uniclipboard:$theme" "gtk-common-themes:$theme"
  done
  snap connections uniclipboard >"$OUT/connections.txt"
  snap list >"$OUT/snaps.txt"
  timeout --kill-after=10s 180s dbus-run-session -- bash "$0" --session "$OUT"
  exit
fi

# The runner's HOME/keyring are disposable; no development file-store exception is used.
printf 'uc-snap-acceptance' | gnome-keyring-daemon --foreground --unlock --components=secrets >"$OUT/keyring.log" 2>&1 &
keyring_pid=$!
Xvfb :98 -screen 0 1280x800x24 -ac -nolisten tcp >"$OUT/xvfb.log" 2>&1 &
xvfb_pid=$!
run_pid=''
trap 'kill ${run_pid:-} "$xvfb_pid" "$keyring_pid" 2>/dev/null || true' EXIT
export DISPLAY=:98 GDK_BACKEND=x11 GSK_RENDERER=cairo LIBGL_ALWAYS_SOFTWARE=1 UC_DISABLE_SYSTEM_CLIPBOARD=1
for _ in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
keyring_ready=false
for _ in $(seq 100); do
  if dbus-send --session --dest=org.freedesktop.DBus --print-reply /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q 'boolean true'; then
    keyring_ready=true; break
  fi
  sleep 0.1
done
[[ "$keyring_ready" == true ]] || { echo 'Private Secret Service did not start' >&2; exit 1; }
timeout --kill-after=10s 100s snap run uniclipboard >"$OUT/gui.log" 2>&1 &
run_pid=$!
web_pid=''
for _ in $(seq 120); do
  for candidate in $(pgrep -u "$(id -u)" -f '[W]ebKitWebProcess' || true); do
    executable="$(sudo readlink "/proc/$candidate/exe" 2>/dev/null || true)"
    case "$executable" in */webkitgtk-6.0/WebKitWebProcess) web_pid="$candidate"; break ;; esac
  done
  [[ -z "$web_pid" ]] || break
  sleep 0.5
done
[[ -n "$web_pid" ]] || { echo 'Snap never started the GTK4 WebKit process' >&2; exit 1; }
sleep 15
kill -0 "$web_pid"
sudo cat "/proc/$web_pid/maps" >"$OUT/webkit-maps.txt"
grep -E '/snap/uniclipboard/[^/]+/usr/lib/.*libwebkitgtk-6.0' "$OUT/webkit-maps.txt"
grep -E '/snap/(gnome-46-2404/[^/]+|uniclipboard/[^/]+/gnome-platform)/usr/lib/.*libgtk-4' "$OUT/webkit-maps.txt"
xwininfo -root -tree >"$OUT/windows.txt"
grep -q '"UniClipboard"' "$OUT/windows.txt"
find "$HOME/snap/uniclipboard" -name daemon.conn -print >"$OUT/connection-paths.txt"
[[ -s "$OUT/connection-paths.txt" ]]
printf 'PASS: confined GTK4 Snap, WebKit helper, platform GTK, window and daemon connection\n' | tee "$OUT/result.txt"
