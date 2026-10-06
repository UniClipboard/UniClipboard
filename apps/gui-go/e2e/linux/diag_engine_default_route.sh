#!/usr/bin/env bash
# 17c11 diagnosis (NOT a fix, NOT a product change): the real release daemon from an AppImage extract, started as an unprivileged user with the real HOME, a private
# session bus + unlocked Secret Service and Xvfb, in four network situations. Archives the raw daemon output and the container's routes per case.
#   diag_engine_default_route.sh <outdir> <AppImage> <image>
# Cases: bridge (default Docker network), internal-noroute (--internal network: an interface, no default route), internal-route (same + `ip route add default dev eth0`),
#        none (--network none: loopback only).
set -uo pipefail
out="${1:?outdir}"; appimage="$(cd "$(dirname "${2:?AppImage}")" && pwd)/$(basename "$2")"; image="${3:?image}"
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
[ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || { echo "$out is not empty" >&2; exit 2; }
mkdir -p "$out"; out="$(cd "$out" && pwd)"
net="uc17c11-diag-$$"; docker network create --internal "$net" > "$out/network-create.txt"
inner='
set -x
useradd -m -u 1500 uc; mkdir -p /bus /tmp/run && chown uc /bus /tmp/run; chmod 700 /tmp/run
cp /in/app.AppImage /home/uc/x.AppImage; chown uc /home/uc/x.AppImage; chmod +x /home/uc/x.AppImage
cd /home/uc && su uc -c "./x.AppImage --appimage-extract" >/dev/null 2>&1
(Xvfb :99 -screen 0 1280x800x24 -nolisten tcp >/tmp/xvfb.log 2>&1 &); sleep 2
[ "$ROUTE" = 1 ] && ip route add default dev eth0
echo "== routes"; cat /proc/net/route; ip -4 addr
cat > /tmp/run.sh <<"EOS"
export HOME=/home/uc XDG_RUNTIME_DIR=/tmp/run DBUS_SESSION_BUS_ADDRESS=unix:path=/bus/bus DISPLAY=:99 XDG_SESSION_TYPE=x11
(sh /work/apps/gui-go/e2e/linux/keyring_service.sh > /tmp/keyring.log 2>&1 &)
for i in $(seq 100); do [ -e /bus/ready ] && break; sleep .2; done
cd /home/uc/squashfs-root; export APPDIR=$PWD UC_DISABLE_SYSTEM_CLIPBOARD=1 RUST_LOG=debug
timeout 15 ./usr/bin/uniclipd > /tmp/daemon.out 2>&1; echo "daemon rc=$?"
sed "s/\x1b\[[0-9;]*m//g" /tmp/daemon.out > /tmp/daemon.plain
echo "== verdict"; grep -c "engine startup failed" /tmp/daemon.plain; grep -E "engine startup failed|Error: engine error" /tmp/daemon.plain | head -3
cp /tmp/daemon.plain /out/daemon.log
EOS
su uc -c "bash /tmp/run.sh"
'
for case in bridge internal-noroute internal-route none; do
  mkdir -p "$out/$case"
  case "$case" in bridge) netargs=(); route=0 ;; internal-noroute) netargs=(--network "$net"); route=0 ;; internal-route) netargs=(--network "$net"); route=1 ;; none) netargs=(--network none); route=0 ;; esac
  docker run --rm --platform linux/arm64 ${netargs[@]+"${netargs[@]}"} --cap-add NET_ADMIN --cap-add SYS_ADMIN --device /dev/fuse --security-opt apparmor:unconfined --security-opt seccomp:unconfined \
    -e ROUTE="$route" -v "$ROOT:/work:ro" -v "$appimage:/in/app.AppImage:ro" -v "$out/$case:/out" "$image" bash -c "$inner" > "$out/$case/stdout.txt" 2>&1
  echo "$case rc=$?" | tee -a "$out/summary.txt"
  grep -A3 "== verdict" "$out/$case/stdout.txt" | tee -a "$out/summary.txt"
done
docker network rm "$net" >/dev/null
