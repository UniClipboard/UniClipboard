#!/usr/bin/env bash
# 17c11 diagnosis (NOT a product change): what the host's own `xdg-open <directory>` does when XDG_CURRENT_DESKTOP is unset (generic dispatch) versus GNOME, as an
# unprivileged user with a private session bus, Xvfb and the distribution's real Nautilus. Records, per dispatch, whether xdg-open is still alive after the window
# appeared (foreground wait on the first application instance), the process tree below it, the window titles and how it ends when the application is stopped.
#   diag_xdg_open_generic.sh <outdir> <image>
set -uo pipefail
out="${1:?outdir}"; image="${2:?image}"
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
[ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out"; out="$(cd "$out" && pwd)"
inner='
useradd -m -u 1500 uc; mkdir -p /bus /tmp/run /tmp/d/uc11-diag-dir && chown -R uc /bus /tmp/run /tmp/d; chmod 700 /tmp/run
(Xvfb :99 -screen 0 1280x800x24 -nolisten tcp >/tmp/xvfb.log 2>&1 &); sleep 2
cat > /tmp/s.sh <<"EOS"
export HOME=/home/uc XDG_RUNTIME_DIR=/tmp/run DBUS_SESSION_BUS_ADDRESS=unix:path=/bus/bus DISPLAY=:99 XDG_SESSION_TYPE=x11
[ "$DESKTOP" = gnome ] && export XDG_CURRENT_DESKTOP=GNOME
(sh /work/apps/gui-go/e2e/linux/keyring_service.sh > /tmp/keyring.log 2>&1 &)
for i in $(seq 100); do [ -e /bus/ready ] && break; sleep .2; done
xdg-open /tmp/d/uc11-diag-dir > /tmp/xdg-open.out 2> /tmp/xdg-open.err &
xo=$!
for i in $(seq 40); do xwininfo -root -tree | grep -q "uc11-diag-dir" && break; sleep .5; done
echo "window titles with the name: $(xwininfo -root -tree | grep -c uc11-diag-dir)"
sleep 5
if kill -0 $xo 2>/dev/null; then echo "xdg-open ALIVE after the window appeared (+5 s), pid $xo"; else wait $xo; echo "xdg-open exited rc=$? after the window appeared"; fi
echo "-- process tree (pid ppid exe cmdline)"
for p in /proc/[0-9]*; do pid=${p#/proc/}; [ -r $p/stat ] || continue; pp=$(awk "{print \$4}" $p/stat 2>/dev/null); echo "$pid $pp $(readlink $p/exe 2>/dev/null) $(tr "\0" " " < $p/cmdline 2>/dev/null | cut -c1-120)"; done | grep -E "xdg-open|nautilus|gio" | grep -v -e grep -e s.sh
pkill -u uc nautilus; sleep 2
if kill -0 $xo 2>/dev/null; then echo "xdg-open still alive after nautilus was stopped"; else wait $xo; echo "xdg-open ended rc=$? after nautilus was stopped"; fi
cat /tmp/xdg-open.err | head -5
EOS
su uc -c "DESKTOP=$DESKTOP bash /tmp/s.sh"
'
for d in generic gnome; do
  mkdir -p "$out/$d"
  docker run --rm --init --platform linux/arm64 --shm-size 1g --security-opt apparmor:unconfined --security-opt seccomp:unconfined --security-opt systempaths=unconfined -e DESKTOP="$d" \
    -v "$ROOT:/work:ro" "$image" bash -c "$inner" > "$out/$d/stdout.txt" 2>&1
  echo "$d rc=$?" | tee -a "$out/summary.txt"; cat "$out/$d/stdout.txt" | sed -n '/window titles/,$p' | tee -a "$out/summary.txt"
done
