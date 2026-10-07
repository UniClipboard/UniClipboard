#!/bin/sh
# Image check: weston is present, its protocol list has no wlr-layer-shell, and the host still has NO GTK/WebKitGTK (the AppImage's own libraries must be the ones that run).
set -eu
command -v weston >/dev/null && weston --version
! ldconfig -p | grep -q 'libgtk-3.so.0' || { echo "host GTK3 is installed: the image would hide a missing bundled library" >&2; exit 1; }
! ldconfig -p | grep -q 'libwebkit2gtk' || { echo "host WebKitGTK is installed" >&2; exit 1; }
! ldconfig -p | grep -q 'libgtk-layer-shell' || { echo "host gtk-layer-shell is installed: not a control for the missing library" >&2; exit 1; }
! strings /usr/bin/weston /usr/lib/*/weston/*.so /usr/lib/*/libweston-13/*.so 2>/dev/null | grep -q zwlr_layer_shell_v1 || { echo "weston advertises wlr-layer-shell: not a control for the missing protocol" >&2; exit 1; }
command -v ss >/dev/null && command -v python3 >/dev/null
echo image-ok
