#!/bin/sh
# Strict acceptance of the 17c2 layer: every item is checked on its own and any failure stops the build. Tool help
# output is not used (its exit status is not a statement about the installation); package state, version output,
# executable paths and the library symbols are.
set -eu
[ -z "$(dpkg --audit)" ] || { echo "dpkg --audit not clean" >&2; dpkg --audit >&2; exit 1; }
for pkg in sway libgtk-layer-shell0 grim wtype wayland-utils imagemagick libgl1-mesa-dri python3-gi gir1.2-gtk-3.0; do
  state=$(dpkg-query -W -f='${db:Status-Status}' "$pkg")
  [ "$state" = installed ] || { echo "package $pkg state: $state" >&2; exit 1; }
done
sway --version
for tool in grim wtype wayland-info convert; do
  command -v "$tool" >/dev/null || { echo "missing executable: $tool" >&2; exit 1; }
done
python3 -c "import gi; gi.require_version('Gtk', '3.0'); from gi.repository import Gtk" || { echo "python3 GTK3 bindings unusable" >&2; exit 1; }
lib=$(ldconfig -p | awk '/libgtk-layer-shell\.so\.0/ {print $NF; exit}')
[ -n "$lib" ] || { echo "libgtk-layer-shell.so.0 not in the loader cache" >&2; exit 1; }
for sym in gtk_layer_is_supported gtk_layer_init_for_window gtk_layer_is_layer_window gtk_layer_set_namespace gtk_layer_set_layer \
           gtk_layer_set_keyboard_mode gtk_layer_get_keyboard_mode gtk_layer_set_exclusive_zone gtk_layer_set_anchor \
           gtk_layer_set_margin gtk_layer_set_monitor; do
  nm -D --defined-only "$lib" | grep -q " T $sym\$" || { echo "missing symbol $sym in $lib" >&2; exit 1; }
done
echo "image 17c2 verified: sway=$(sway --version) layer-shell=$lib"
