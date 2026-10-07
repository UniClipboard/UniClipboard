#!/bin/bash
# Runs INSIDE a 17c13 container image (weston or sway-without-library): starts a headless Wayland compositor, records its REAL protocol registry (wayland-info) and then
# runs native_wayland_probe.py against the AppImage. usage: native_wayland_container.sh <weston|sway> <probe mode> <out dir inside the container>
# The registry listing is the evidence for the protocol premise (zwlr_layer_shell_v1 absent on weston, present on sway); strings scans of the image are only a static precondition.
set -uo pipefail
comp="$1"; mode="$2"; out="$3"
export XDG_RUNTIME_DIR=/run/uc-xdg; mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
mkdir -p "$out"
ip route show > "$out/routes-before.txt" 2>&1
ip route add default dev eth0 > "$out/route-add.txt" 2>&1 || true   # fixture requirement of the Engine (17c11 R1), recorded
export WAYLAND_DEBUG= WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman
case "$comp" in
  weston) weston --backend=headless --renderer=pixman --socket=wayland-1 --width=1280 --height=800 > "$out/compositor.log" 2>&1 & ;;
  sway) printf 'output * resolution 1280x800\n' > /tmp/sway.conf; sway -c /tmp/sway.conf > "$out/compositor.log" 2>&1 & ;;
  *) echo "unknown compositor $comp" >&2; exit 2 ;;
esac
cpid=$!
for _ in $(seq 100); do ls "$XDG_RUNTIME_DIR"/wayland-[0-9] >/dev/null 2>&1 && break; sleep 0.2; done
ls -la "$XDG_RUNTIME_DIR" > "$out/runtime-dir.txt"
wl=$(basename "$(ls "$XDG_RUNTIME_DIR"/wayland-[0-9] | head -1)")
WAYLAND_DISPLAY="$wl" wayland-info > "$out/wayland-info.txt" 2>&1; echo "wayland-info rc=$?" >> "$out/wayland-info.txt"
{ grep -c 'zwlr_layer_shell_v1' "$out/wayland-info.txt" || true; } > "$out/layer-shell-advertised-count.txt"
python3 -I /work/apps/gui-go/e2e/native_wayland_probe.py --appimage /in/appimage.AppImage --out "$out/probe" --mode "$mode" --compositor-kind generic --seconds 8 > "$out/probe.stdout" 2>&1
rc=$?
echo "$rc" > "$out/probe.rc"
kill "$cpid" 2>/dev/null; wait "$cpid" 2>/dev/null
exit "$rc"
