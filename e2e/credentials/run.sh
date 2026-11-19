#!/bin/sh
# Run inside an isolated Linux container with dbus and gnome-keyring installed.
# A new HOME and dbus-run-session prevent access to personal credential items.
set -eu
credential_binary=$1
credential_home=$(mktemp -d)
trap 'rm -rf "$credential_home"' EXIT HUP INT TERM
export HOME=$credential_home
dbus-run-session -- sh -eu -c '
  printf %s synthetic-test-only | gnome-keyring-daemon --unlock --components=secrets >/dev/null
  "$1" --store
  "$1"
  "$1" --store --legacy
  "$1" --legacy
  "$1" --store --ambiguous
  "$1" --ambiguous
  "$1" --missing
  "$1" --cancelled
  DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/unic-e2e-missing-bus "$1" --unavailable
' sh "$credential_binary"
printf '%s\n' 'PASS private credential HOME and bus removed after container exit'
