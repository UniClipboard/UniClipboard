#!/bin/sh
# Build-time check of the 17c12 session image: the executables and packages are INSTALLED. This does NOT prove the session works: gnome-keyring-daemon cannot even print its version
# inside a plain build container on Fedora ("error dropping process capabilities"), so readiness is proven only at run time, by the runner's T1 (a real store and lookup through the
# Secret Service API in the isolated session). Every command's status is checked (the first version put the version query in a pipeline and printed "session image ok:" after it failed).
set -eu
for tool in gnome-keyring-daemon secret-tool dbus-daemon tinyproxy curl proxy dconf gsettings; do command -v "$tool" >/dev/null || { echo "missing executable: $tool" >&2; exit 1; }; done
echo "session image: gnome-keyring, secret-tool, dbus-daemon installed (readiness is proven at run time by T1, not here)"
