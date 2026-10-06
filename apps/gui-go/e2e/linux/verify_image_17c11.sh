#!/bin/sh
# Strict acceptance of the 17c11 real-application images: the applications are the distribution's own packages and are not stubs.
set -eu
for tool in xdg-open xdg-mime gio strace readelf python3 nautilus loupe gnome-keyring-daemon secret-tool dbus-daemon dbus-send xwininfo xprop; do command -v "$tool" >/dev/null || { echo "missing executable: $tool" >&2; exit 1; }; done
browser="${UC_BROWSER_BIN:-epiphany}"
command -v "$browser" >/dev/null || { echo "missing browser: $browser" >&2; exit 1; }
[ "$(file -b "$(readlink -f "$(command -v "$browser")")" 2>/dev/null | head -c3 || echo ELF)" != "POS" ] || { echo "$browser is a script stub" >&2; exit 1; }
! command -v snap >/dev/null || { echo "snap present: the browser could be a stub" >&2; exit 1; }
for d in org.gnome.Nautilus.desktop; do [ -e "/usr/share/applications/$d" ] || { echo "missing $d" >&2; exit 1; }; done
[ -s /etc/machine-id ] || [ -s /var/lib/dbus/machine-id ] || { echo 'no machine-id' >&2; exit 1; }
echo "real-app image ok: $(. /etc/os-release; echo "$PRETTY_NAME"); $("$browser" --version 2>&1 | head -1); $(nautilus --version); $(loupe --version 2>&1 | head -1)"
