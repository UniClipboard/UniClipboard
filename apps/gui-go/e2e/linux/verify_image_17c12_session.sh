#!/bin/sh
set -eu
for tool in gnome-keyring-daemon secret-tool dbus-daemon tinyproxy curl proxy dconf gsettings; do command -v "$tool" >/dev/null || { echo "missing executable: $tool" >&2; exit 1; }; done
echo "session image ok: $(gnome-keyring-daemon --version | head -1)"
