#!/bin/sh
# Strict acceptance of the 17c10 host-helper images: the real helpers exist and are the distribution's own, and the toolkit libraries are still absent.
set -eu
for tool in xdg-open xdg-mime gio strace readelf python3; do command -v "$tool" >/dev/null || { echo "missing executable: $tool" >&2; exit 1; }; done
for lib in libwebkit2gtk libgtk-3 libsoup; do
  if ldconfig -p | grep -q "$lib"; then echo "host unexpectedly provides $lib" >&2; exit 1; fi
done
ldconfig -p | grep -q libgio-2.0.so.0 || { echo "host libgio missing" >&2; exit 1; }
[ -s /etc/machine-id ] || [ -s /var/lib/dbus/machine-id ] || { echo 'no machine-id' >&2; exit 1; }
gio version >/dev/null
[ -s /usr/share/mime/mime.cache ] || { echo 'shared-mime-info database missing' >&2; exit 1; }
xdg-open --version | head -1
echo "helper image ok: $(. /etc/os-release; echo "$PRETTY_NAME"); gio $(gio version); $(command -v xdg-open)"
