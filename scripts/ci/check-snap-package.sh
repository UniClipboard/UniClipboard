#!/usr/bin/env bash
# Install and launch the GTK4 Snap on a disposable GitHub runner, without disabling WebKit's sandbox.
set -euo pipefail
[[ "${GITHUB_ACTIONS:-}" == true ]] || { echo 'Run only on a disposable GitHub runner' >&2; exit 2; }
OUT="$(mkdir -p "${2:?evidence directory}" && cd "$2" && pwd)"
if [[ "$1" != --session ]]; then
  PACKAGE="$(readlink -f "$1")"
  sudo apt-get update -qq
  sudo apt-get install -y --no-install-recommends xvfb x11-utils dbus-x11 dbus-user-session gnome-keyring libsecret-tools strace
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
  # Snap asks the user manager on the session bus to create its tracking cgroup.
  # A private dbus-run-session has no user manager and fails before the app starts.
  sudo loginctl enable-linger "$(id -un)"
  sudo systemctl start "user@$(id -u).service"
  export XDG_RUNTIME_DIR="/run/user/$(id -u)"
  export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
  systemctl --user start dbus.socket
  timeout --kill-after=10s 180s systemd-run --user --scope --quiet -- bash "$0" --session "$OUT"
  exit
fi

# The runner's HOME/keyring are disposable; no development file-store exception is used.
printf 'uc-snap-acceptance' | gnome-keyring-daemon --foreground --unlock --components=secrets >"$OUT/keyring.log" 2>&1 &
keyring_pid=$!
Xvfb :98 -screen 0 1280x800x24 -ac -nolisten tcp >"$OUT/xvfb.log" 2>&1 &
xvfb_pid=$!
run_pid=''
collect_evidence() {
  ps -eo pid,ppid,uid,stat,wchan:32,args >"$OUT/processes.txt"
  xwininfo -root -tree >"$OUT/windows.txt" 2>&1 || true
  # The release host redirects its logs to files before Wails initializes.
  # Console output alone cannot distinguish a launcher stall from a host error.
  while IFS= read -r -d "" file; do
    relative="${file#"$HOME/snap/uniclipboard/"}"
    mkdir -p "$OUT/application-logs/$(dirname "$relative")"
    cp "$file" "$OUT/application-logs/$relative" || true
  done < <(find "$HOME/snap/uniclipboard" -type f -path "*/logs/*" -print0 2>/dev/null)
  sudo journalctl -k --since "@$session_start" --no-pager >"$OUT/kernel.log" 2>&1 || true
}
cleanup() {
  collect_evidence
  kill ${run_pid:-} "$xvfb_pid" "$keyring_pid" 2>/dev/null || true
}
session_start=$(date +%s)
trap cleanup EXIT
export GODEBUG=inittrace=1
export DISPLAY=:98 GDK_BACKEND=x11 GSK_RENDERER=cairo LIBGL_ALWAYS_SOFTWARE=1 UC_DISABLE_SYSTEM_CLIPBOARD=1 SNAP_DESKTOP_DEBUG=1
for _ in $(seq 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.2; done
keyring_ready=false
for _ in $(seq 100); do
  if dbus-send --session --dest=org.freedesktop.DBus --print-reply /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q 'boolean true'; then
    keyring_ready=true; break
  fi
  sleep 0.1
done
[[ "$keyring_ready" == true ]] || { echo 'Runner Secret Service did not start' >&2; exit 1; }
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
if [[ -z "$web_pid" ]]; then
  if ! kill -0 "$run_pid" 2>/dev/null; then
    set +e
    wait "$run_pid"
    printf 'initial_exit_code=%s\n' "$?" >"$OUT/exit-code.txt"
    set -e
  fi
  timeout --kill-after=5s 20s snap run --shell uniclipboard -c '
    printf "HOME=%s\nSNAP_USER_DATA=%s\nSNAP_USER_COMMON=%s\nXDG_STATE_HOME=%s\nXDG_RUNTIME_DIR=%s\nDBUS_SESSION_BUS_ADDRESS=%s\n" "$HOME" "$SNAP_USER_DATA" "$SNAP_USER_COMMON" "${XDG_STATE_HOME:-}" "${XDG_RUNTIME_DIR:-}" "${DBUS_SESSION_BUS_ADDRESS:-}"
  ' >"$OUT/snap-paths.txt" 2>&1 || true
  # Observe paths, connections and exit status only; never read credential bytes.
  timeout --kill-after=5s 20s snap run --strace="-f -e trace=openat,connect,exit_group -s 120" uniclipboard >"$OUT/syscalls.log" 2>&1 || true
  echo 'Snap never started the GTK4 WebKit process' >&2
  exit 1
fi
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
