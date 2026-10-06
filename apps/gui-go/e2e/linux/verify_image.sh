#!/bin/sh
# Real integrity check of the build image: no half-installed package, the development files the Go GUI links against
# resolve through pkg-config, and every tool the E2E and packaging scripts call exists. A cached Docker layer
# marked DONE proves none of this.
set -eu
audit="$(dpkg --audit)"
if [ -n "$audit" ]; then echo "dpkg --audit reports problems:" >&2; echo "$audit" >&2; exit 1; fi
pkg-config --exists --print-errors gtk+-3.0 webkit2gtk-4.1 gdk-3.0 x11 dbus-1
for tool in Xvfb xdpyinfo xdotool xauth dbus-run-session dbus-daemon rpmbuild dpkg-deb file python3 git curl go; do
  command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done
echo "gtk3 $(pkg-config --modversion gtk+-3.0), webkit2gtk-4.1 $(pkg-config --modversion webkit2gtk-4.1), $(go version)"
