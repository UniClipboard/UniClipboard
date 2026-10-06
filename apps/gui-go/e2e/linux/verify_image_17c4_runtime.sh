#!/bin/sh
# Strict acceptance of the 17c4 runtime image: the toolkit libraries (GTK, WebKitGTK, JavaScriptCore, libsoup, cairo, pango) must be ABSENT (otherwise the AppImage E2E proves nothing),
# and the services the E2E relies on must be present and working.
set -eu
[ -z "$(dpkg --audit)" ] || { echo "dpkg --audit not clean" >&2; exit 1; }
for lib in libwebkit2gtk libjavascriptcoregtk libgtk-3 libgdk-3 libgtk-4 libsoup libcairo.so libpango libgdk_pixbuf; do
  if ldconfig -p | grep -q "$lib"; then echo "host unexpectedly provides $lib" >&2; ldconfig -p | grep "$lib" >&2; exit 1; fi
done
[ ! -d /usr/lib/aarch64-linux-gnu/webkit2gtk-4.1 ] && [ ! -d /usr/lib/x86_64-linux-gnu/webkit2gtk-4.1 ] || { echo "webkit2gtk helper directory present" >&2; exit 1; }
for tool in Xvfb xdotool xdpyinfo dbus-run-session fusermount3; do
  command -v "$tool" >/dev/null || { echo "missing executable: $tool" >&2; exit 1; }
done
for lib in libGL.so.1 libEGL.so.1 libX11.so.6 libwayland-client.so.0 libfuse.so.2; do
  ldconfig -p | grep -q "$lib" || { echo "host driver-stack library missing: $lib" >&2; exit 1; }
done
echo "runtime image ok: no GTK/WebKitGTK/libsoup/cairo/pango on the host"
