#!/bin/sh
# Real integrity check of the build image: no half-installed package, the development files the Go GUI links against
# resolve through pkg-config, and every tool the E2E and packaging scripts call exists. A cached Docker layer
# marked DONE proves none of this.
set -eu
pkg-config --atleast-version=4.10 gtk4
audit="$(dpkg --audit)"
if [ -n "$audit" ]; then echo "dpkg --audit reports problems:" >&2; echo "$audit" >&2; exit 1; fi
pkg-config --exists --print-errors gtk4 webkitgtk-6.0 x11 dbus-1
for tool in Xvfb xdpyinfo xdotool xauth dbus-run-session dbus-daemon rpmbuild dpkg-deb file python3 git curl go; do
  command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done
echo "gtk4 $(pkg-config --modversion gtk4), webkitgtk-6.0 $(pkg-config --modversion webkitgtk-6.0), $(go version)"
