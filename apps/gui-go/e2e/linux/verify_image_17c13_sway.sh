#!/bin/sh
set -eu
! ldconfig -p | grep -q 'libgtk-layer-shell' || { echo "the host still has libgtk-layer-shell" >&2; exit 1; }
! find / -xdev -name 'libgtk-layer-shell*' 2>/dev/null | grep -q . || { echo "a libgtk-layer-shell file is still on the host" >&2; exit 1; }
command -v sway >/dev/null && command -v ss >/dev/null && command -v wayland-info >/dev/null
echo image-ok
