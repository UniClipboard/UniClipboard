#!/bin/sh
# Strict acceptance of the Fedora runtime image: the toolkit libraries must be ABSENT, the host's own GLib and glib-networking module PRESENT.
set -eu
for lib in libwebkit2gtk libjavascriptcoregtk libgtk-3 libgdk-3 libgtk-4 libsoup libcairo.so libpango libgdk_pixbuf; do
  if ldconfig -p | grep -q "$lib"; then echo "host unexpectedly provides $lib" >&2; ldconfig -p | grep "$lib" >&2; exit 1; fi
done
for tool in Xvfb xdotool xdpyinfo openssl update-ca-trust dbus-launch fusermount3 useradd; do command -v "$tool" >/dev/null || { echo "missing executable: $tool" >&2; exit 1; }; done
for lib in libGL.so.1 libEGL.so.1 libGLESv2.so.2 libX11.so.6 libwayland-client.so.0 libfuse.so.2 libglib-2.0.so.0 libgio-2.0.so.0; do
  ldconfig -p | grep -q "$lib" || { echo "host library missing: $lib" >&2; exit 1; }
done
[ -s /etc/machine-id ] || { echo '/etc/machine-id is empty' >&2; exit 1; }
ls /usr/lib64/gio/modules/libgiognutls.so >/dev/null
echo "fedora runtime image ok: $(. /etc/os-release; echo "$PRETTY_NAME"), no GTK/WebKitGTK/libsoup/cairo/pango, host glib-networking present"
