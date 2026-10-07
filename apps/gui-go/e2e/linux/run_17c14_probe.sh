#!/usr/bin/env bash
# Runs INSIDE uc-gui-go-linux-build:17c9-wm with the repo at /work, cache volume at /cache and the output dir at /out.
# Usage: run_17c14_probe.sh MODE [fatal]   MODE=update|settray; "fatal" sets G_DEBUG=fatal-criticals to capture the stack.
set -euo pipefail
MODE=$1; FATAL=${2:-}
export GOPATH=/cache/gopath GOFLAGS=-mod=mod CGO_ENABLED=1
git config --global --add safe.directory /work
cd /work/apps/gui-go
go build -tags gtk3 -o /out/tray_probe_$MODE ./e2e/linux/tray_probe
[[ -n $FATAL ]] && export G_DEBUG=fatal-criticals
cat > /out/inner_$MODE.sh <<'INNER'
set -u
MODE=$1
python3 /work/apps/gui-go/e2e/linux/tray_probe/sni_host.py --log /out/host_$MODE.jsonl --duration 16 \
  --click "Probe action@5" --click "Probe quit@13" &
HOST=$!
sleep 1
/out/tray_probe_$MODE -icon /work/apps/gui-go/icons/tray-icon@2x.png -refresh "$MODE" >/out/app_$MODE.log 2>&1 &
APP=$!
wait $APP; echo "app-exit=$?" >/out/app_exit_$MODE.txt
wait $HOST || true
INNER
rm -f /out/*_$MODE.jsonl
xvfb-run -a dbus-run-session -- bash /out/inner_$MODE.sh "$MODE"
echo "--- app exit"; cat /out/app_exit_$MODE.txt
