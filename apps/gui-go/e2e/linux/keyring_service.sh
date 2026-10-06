#!/bin/sh
# Runs inside uc-gui-go-linux-keyring:17c4: a private session bus on the shared volume (/bus/bus) with an unlocked gnome-keyring
# Secret Service. Stays in the foreground; the orchestrator stops the container by its exact name.
set -eu
rm -f /bus/bus /bus/ready
dbus-daemon --session --address=unix:path=/bus/bus --nofork --print-pid > /bus/dbus.pid &
for i in $(seq 50); do [ -S /bus/bus ] && break; sleep 0.1; done
export DBUS_SESSION_BUS_ADDRESS=unix:path=/bus/bus
# The login password creates and unlocks the default collection without a prompt (an isolated, throwaway keyring inside this
# container; no display exists for a prompter).
printf 'uc-e2e-throwaway' | gnome-keyring-daemon --unlock --components=secrets > /bus/keyring.env
# Readiness is a real store and lookup through the Secret Service API, not the daemon being alive.
printf 'probe-secret' | secret-tool store --label=uc-e2e-probe uc-e2e probe
[ "$(secret-tool lookup uc-e2e probe)" = probe-secret ]
secret-tool clear uc-e2e probe
echo ready > /bus/ready
exec sleep infinity
