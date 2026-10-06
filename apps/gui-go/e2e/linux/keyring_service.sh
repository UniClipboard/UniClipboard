#!/bin/sh
# Runs inside uc-gui-go-linux-keyring:17c4: a private session bus on the shared volume (/bus/bus) with an unlocked gnome-keyring
# Secret Service. Stays in the foreground; the orchestrator stops the container by its exact name.
set -eu
rm -f /bus/bus /bus/ready
dbus-daemon --session --address=unix:path=/bus/bus --nofork --print-pid > /bus/dbus.pid &
for i in $(seq 50); do [ -S /bus/bus ] && break; sleep 0.1; done
export DBUS_SESSION_BUS_ADDRESS=unix:path=/bus/bus
# One long-lived daemon that owns org.freedesktop.secrets for the whole run. The first version ran `gnome-keyring-daemon --unlock`
# (which hands over to a bus-activated instance) and the final 17c4 run showed the cost: that instance can exit, D-Bus then activates a
# fresh one whose login keyring is LOCKED, it asks for a prompt (gcr-prompter needs a display, there is none) and every request that
# needs a secret blocks (final-70dd6dcbe/e2e-full: `/encryption/state` never completed, `uniclip space status` timed out).
# The login password creates and unlocks the default collection without a prompt (an isolated, throwaway keyring inside this container).
printf 'uc-e2e-throwaway' | gnome-keyring-daemon --foreground --unlock --components=secrets > /bus/keyring.env 2> /bus/keyring.err &
for i in $(seq 100); do
  dbus-send --session --dest=org.freedesktop.DBus --print-reply /org/freedesktop/DBus org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets 2>/dev/null | grep -q 'boolean true' && break
  sleep 0.1
done
# Readiness is a real store and lookup through the Secret Service API, not the daemon being alive.
printf 'probe-secret' | secret-tool store --label=uc-e2e-probe uc-e2e probe
[ "$(secret-tool lookup uc-e2e probe)" = probe-secret ]
secret-tool clear uc-e2e probe
echo ready > /bus/ready
exec sleep infinity
