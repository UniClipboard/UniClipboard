#!/usr/bin/env bash
# Launch a Linux AppImage in an isolated desktop session and verify that it shows
# its window and keeps running. Nothing outside --artifacts is read or written:
# HOME and every XDG directory live in the artifacts directory, the session D-Bus
# and Secret Service are private, and outbound HTTP is pointed at a dead proxy.
#
# usage: scripts/linux-appimage-smoke.sh [options] <AppImage>
#   --display nested-hyprland|xvfb            how to get a display (default: nested-hyprland)
#   --session wayland|x11                     export WAYLAND_DISPLAY to the app or not (default: wayland)
#   --seconds N                               how long the app must stay up (default: 90)
#   --artifacts DIR                           evidence directory (default: ./appimage-smoke)
#   --env KEY=VALUE                           extra environment for the app (repeatable)
#   (the AppImage argument may also be an extracted AppDir, for hosts that cannot exec the
#   AppImage runtime, e.g. x86_64 containers under emulation)
#   --drop-bundled GLOB                       extract the AppImage and delete matching files
#                                             from usr/lib first (mechanism experiments only)
#
# nested-hyprland needs Hyprland, grim and a running parent Wayland session (WAYLAND_DISPLAY, absolute
# path allowed); its Xwayland provides the X11 display. xvfb needs Xvfb and gives a real X server.
# The AppImage's AppRun hook forces GDK_BACKEND=x11, so the window always goes through an X server
# (Xwayland under nested-hyprland); --session only decides whether WAYLAND_DISPLAY is visible to the app.
# Exit status: 0 = window shown, frontend reported ready, app stable, no loader/EGL errors;
#              1 = failed; 2 = bad usage or missing tool.
set -u

display=nested-hyprland session=wayland seconds=90 artifacts=$PWD/appimage-smoke drop=""
app_env=()
while [ $# -gt 0 ]; do
  case $1 in
    --display) display=$2; shift 2 ;;
    --session) session=$2; shift 2 ;;
    --seconds) seconds=$2; shift 2 ;;
    --artifacts) artifacts=$2; shift 2 ;;
    --env) app_env+=("$2"); shift 2 ;;
    --drop-bundled) drop=$2; shift 2 ;;
    -h|--help) sed -n '2,24p' "$0"; exit 0 ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) appimage=$1; shift ;;
  esac
done
[ -n "${appimage:-}" ] && { [ -f "$appimage" ] || [ -f "$appimage/AppRun" ]; } || { echo "AppImage not found" >&2; exit 2; }
for tool in dbus-run-session gnome-keyring-daemon; do
  command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 2; }
done

mkdir -p "$artifacts"; artifacts=$(cd "$artifacts" && pwd); appimage=$(readlink -f "$appimage")
home=$artifacts/home; rm -rf "$home"; mkdir -p "$home"
log=$artifacts/app.log; : > "$log"
# Short path: Wayland and Hyprland sockets must stay under the 108-byte sun_path limit.
runtime=$(mktemp -d /tmp/ucsmoke.XXXXXX); chmod 700 "$runtime"
compositor_pid="" app_sid="" wayland_name="" x_display=""

# Stop everything that was started with this run's private HOME. The daemon sidecar
# detaches from the session, so match on the environment instead of the session id.
stop_sandbox() {
  local pid
  for pid in $(ls /proc | grep -E '^[0-9]+$'); do
    [ "$pid" = "$$" ] && continue
    tr '\0' '\n' < "/proc/$pid/environ" 2>/dev/null | grep -qx "HOME=$home" && kill -KILL "$pid" 2>/dev/null
  done
}

finish() {
  [ -n "$app_sid" ] && pkill -KILL -s "$app_sid" 2>/dev/null
  stop_sandbox
  # The AppImage runtime mounts itself with FUSE; release a mount left behind by SIGKILL.
  for mount_point in $(awk -v type="fuse.$(basename "$appimage")" '$3 == type {print $2}' /proc/self/mounts); do
    fusermount -uz "$mount_point" 2>/dev/null
  done
  [ -n "$compositor_pid" ] && kill "$compositor_pid" 2>/dev/null
  sleep 1; fusermount -uz "$runtime/gvfs" 2>/dev/null; rm -rf "$runtime"
}
trap finish EXIT

# ---- display -------------------------------------------------------------------
case $display in
  nested-hyprland)
    for tool in Hyprland grim hyprctl; do command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 2; }; done
    [ -n "${WAYLAND_DISPLAY:-}" ] || { echo "nested-hyprland needs a parent Wayland session" >&2; exit 2; }
    parent=$WAYLAND_DISPLAY; case $parent in /*) ;; *) parent=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/$parent ;; esac
    printf 'monitor=,1280x800@60,0x0,1\n' > "$runtime/hypr.conf"
    before=$(ls /tmp/.X11-unix 2>/dev/null | sort)
    env XDG_RUNTIME_DIR="$runtime" WAYLAND_DISPLAY="$parent" HYPRLAND_INSTANCE_SIGNATURE= \
      setsid Hyprland -c "$runtime/hypr.conf" > "$artifacts/compositor.log" 2>&1 &
    compositor_pid=$!
    for _ in $(seq 40); do [ -S "$runtime/wayland-1" ] && break; sleep 0.5; done
    [ -S "$runtime/wayland-1" ] || { echo "nested Hyprland did not start" >&2; exit 1; }
    wayland_name=wayland-1
    sleep 2
    x_display=$(comm -13 <(echo "$before") <(ls /tmp/.X11-unix | sort) | grep -E '^X[0-9]+$' | head -1 | sed 's/^X/:/')
    sig=$(ls "$runtime/hypr" | head -1)
    ;;
  xvfb)
    command -v Xvfb >/dev/null || { echo "missing tool: Xvfb" >&2; exit 2; }
    Xvfb :77 -screen 0 1280x800x24 -nolisten tcp > "$artifacts/compositor.log" 2>&1 &
    compositor_pid=$!; sleep 2; x_display=:77; session=x11
    ;;
  *) echo "unknown --display: $display" >&2; exit 2 ;;
esac

# ---- application ---------------------------------------------------------------
target=$appimage
[ -d "$appimage" ] && target=$appimage/AppRun
if [ -n "$drop" ]; then
  rm -rf "$artifacts/extracted"; mkdir -p "$artifacts/extracted"
  if [ -d "$appimage" ]; then cp -a "$appimage" "$artifacts/extracted/squashfs-root"
  else (cd "$artifacts/extracted" && "$appimage" --appimage-extract > /dev/null); fi
  find "$artifacts/extracted/squashfs-root/usr/lib" -maxdepth 1 -name "$drop" -print -delete > "$artifacts/dropped.txt"
  target=$artifacts/extracted/squashfs-root/AppRun
fi

env_args=(HOME="$home" XDG_RUNTIME_DIR="$runtime" XDG_DATA_HOME="$home/.local/share"
  XDG_CONFIG_HOME="$home/.config" XDG_CACHE_HOME="$home/.cache" XDG_STATE_HOME="$home/.local/state"
  PATH=/usr/local/bin:/usr/bin:/bin LANG=C.UTF-8 NO_COLOR=1 DISPLAY="$x_display"
  HTTP_PROXY=http://127.0.0.1:9 HTTPS_PROXY=http://127.0.0.1:9 ALL_PROXY=http://127.0.0.1:9)
if [ "$session" = wayland ] && [ -n "$wayland_name" ]; then
  env_args+=(WAYLAND_DISPLAY="$wayland_name" XDG_SESSION_TYPE=wayland)
else
  env_args+=(XDG_SESSION_TYPE=x11)
fi
env_args+=("${app_env[@]}")

launcher='eval "$(printf smoke | gnome-keyring-daemon --unlock --components=secrets)"; exec "$0"'
env -i "${env_args[@]}" setsid dbus-run-session -- bash -c "$launcher" "$target" > "$log" 2>&1 &
launch_pid=$!
sleep 1; app_sid=$(ps -o sid= -p "$launch_pid" | tr -d ' ')

# ---- observe -------------------------------------------------------------------
app_alive() { [ -n "$app_sid" ] && pgrep -s "$app_sid" -x uniclipboard > /dev/null; }
window_title() {
  case $display in
    nested-hyprland)
      env XDG_RUNTIME_DIR="$runtime" HYPRLAND_INSTANCE_SIGNATURE="$sig" hyprctl clients -j 2>/dev/null \
        | python3 -c 'import json,sys
for c in json.load(sys.stdin):
    if "uniclip" in (c.get("class","")+c.get("initialClass","")+c.get("title","")).lower():
        print(c.get("class") or c.get("title")); break' ;;
    xvfb)
      command -v xdotool >/dev/null && DISPLAY=$x_display xdotool search --onlyvisible --name UniClipboard 2>/dev/null | head -1 ;;
  esac
}

shot() {
  case $display in
    nested-hyprland) WAYLAND_DISPLAY="$runtime/wayland-1" grim "$artifacts/$1.png" 2>/dev/null ;;
    xvfb) command -v import >/dev/null && DISPLAY=$x_display import -window root "$artifacts/$1.png" 2>/dev/null ;;
  esac
}

window="" first_window=""
start=$(date +%s)
while [ $(( $(date +%s) - start )) -lt "$seconds" ]; do
  app_alive || break
  if [ -z "$window" ]; then
    window=$(window_title); [ -n "$window" ] && first_window=$(( $(date +%s) - start ))
  fi
  sleep 2
done
alive=no; app_alive && alive=yes
[ "$alive" = yes ] && shot final
[ -z "$window" ] && window=$(window_title)

# ---- evidence ------------------------------------------------------------------
{
  echo "appimage=$appimage"; [ -f "$appimage" ] && echo "sha256=$(sha256sum "$appimage" | cut -d' ' -f1)"
  echo "display=$display session=$session x_display=$x_display wayland=$wayland_name"
  echo "date=$(date -u +%FT%TZ)"; uname -srm; grep -E '^(PRETTY_NAME|ID|VERSION_ID)=' /etc/os-release
  ldd --version | head -1
  for pkg in glib2 libglib2.0-0t64 libglib2.0-0 gvfs mesa libegl-mesa0 wayland libwayland-client0 webkit2gtk-4.1; do
    pacman -Q "$pkg" 2>/dev/null; dpkg-query -W "$pkg" 2>/dev/null
  done
  host_wl=$(ldconfig -p | awk '/libwayland-client.so.0/ {print $NF; exit}')
  echo "host libwayland-client: ${host_wl:-none} $(readlink -f "${host_wl:-/nonexistent}" 2>/dev/null)"
} > "$artifacts/environment.txt" 2>&1
[ -f "$artifacts/dropped.txt" ] || :

sed 's/\x1b\[[0-9;]*m//g' "$log" > "$artifacts/app.clean.log"
fatal='Segmentation fault|EGL_BAD_PARAMETER|Could not create default EGL display|undefined symbol|Failed to load module|symbol lookup error|Failed to initialize GTK|panicked|Main window readiness timed out'
fatal_hits=$(grep -a -E "$fatal" "$artifacts/app.clean.log" | sed 's/^\(.\{200\}\).*/\1/' | sort | uniq -c)
frontend_ready=no; grep -a -q 'Main window revealed' "$artifacts/app.clean.log" && frontend_ready=yes
daemon_ready=no; grep -a -q 'to_state=Ready' "$artifacts/app.clean.log" && daemon_ready=yes
daemon_engine=ok; grep -a -q 'engine startup failed' "$home"/.local/state/*/logs/uniclipboard-daemon.json.* 2>/dev/null && daemon_engine=failed
{
  echo "app_alive_at_end=$alive"; echo "frontend_ready=$frontend_ready"; echo "window=${window:-none} first_seen_after_s=${first_window:-never}"
  echo "daemon_ws_ready=$daemon_ready daemon_engine=$daemon_engine"; echo "fatal_log_lines:"; echo "${fatal_hits:-  none}"
} > "$artifacts/result.txt"
cat "$artifacts/environment.txt" "$artifacts/result.txt"

if [ "$alive" = yes ] && [ -n "$window" ] && [ "$frontend_ready" = yes ] && [ -z "$fatal_hits" ]; then
  echo "RESULT: PASS"; exit 0
fi
echo "RESULT: FAIL"; exit 1
