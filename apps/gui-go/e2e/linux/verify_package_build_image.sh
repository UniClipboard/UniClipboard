#!/bin/sh
# Real integrity check of the packaging build image: no half-installed package, the development files the Go GUI links against
# resolve through pkg-config, and every tool and library package_linux.py needs exists. A cached layer marked DONE proves none of this.
set -eu
[ -z "$(dpkg --audit)" ] || { echo "dpkg --audit not clean" >&2; dpkg --audit >&2; exit 1; }
pkg-config --exists --print-errors gtk+-3.0 webkit2gtk-4.1 gdk-3.0 x11 dbus-1 gio-2.0
for tool in rpmbuild rpm2cpio cpio dpkg-deb file python3 git curl go readelf gcc desktop-file-validate glib-compile-schemas; do
  command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done
for lib in libgtk-layer-shell.so.0 libglib-2.0.so.0; do
  ldconfig -p | grep -q "$lib" || { echo "library missing in the loader cache: $lib" >&2; exit 1; }
done
moddir="$(pkg-config --variable=giomoduledir gio-2.0)"
for m in libgiognutls.so libgiognomeproxy.so libdconfsettings.so libgiolibproxy.so; do
  [ -f "$moddir/$m" ] || { echo "GIO module missing: $moddir/$m" >&2; exit 1; }
done
[ -x /usr/libexec/glib-pacrunner ] || { echo "glib-pacrunner missing" >&2; exit 1; }
echo "$(. /etc/os-release && echo "$PRETTY_NAME"), $(ldd --version | head -1), gtk $(pkg-config --modversion gtk+-3.0), webkit2gtk-4.1 $(pkg-config --modversion webkit2gtk-4.1), $(go version)"
