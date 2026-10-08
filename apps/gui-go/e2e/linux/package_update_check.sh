#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../../../.." && pwd)"
kind=${1:?deb or rpm}; package=${2:?test package}; pubkey=${3:?fixture public key}; out=${4:?new output directory}
case "$kind" in
 deb) image=uc-gui-go-linux-real-apps:17c11-ubuntu ;;
 rpm) image=uc-gui-go-linux-real-apps:17c11-fedora ;;
 *) exit 2 ;;
esac
[ ! -e "$out" ] || { echo "output already exists: $out" >&2; exit 2; }
package=$(cd "$(dirname "$package")" && pwd)/$(basename "$package")
pubkey=$(cd "$(dirname "$pubkey")" && pwd)/$(basename "$pubkey")
[ "$(docker info --format '{{.Architecture}}')" = aarch64 ] || { echo "requires a native arm64 Docker host" >&2; exit 2; }
mkdir -p "$out"; out=$(cd "$out" && pwd)
bus="uc-package-ui-bus-$$"; keyring="uc-package-ui-keyring-$$"
docker volume create "$bus" >/dev/null
cleanup() { docker logs "$keyring" > "$out/keyring-container.log" 2>&1 || true; docker stop "$keyring" >/dev/null 2>&1 || true; docker rm "$keyring" >/dev/null 2>&1 || true; docker volume rm "$bus" >/dev/null 2>&1 || true; }
trap cleanup EXIT
docker run -d --name "$keyring" --platform linux/arm64 -v "$bus:/bus" uc-gui-go-linux-keyring:17c4 /usr/local/bin/keyring_service.sh >/dev/null
for _ in $(seq 60); do docker run --rm -v "$bus:/bus" uc-clean-host:bookworm test -f /bus/ready && break; sleep 1; done
docker run --rm --init --platform linux/arm64 -v "$bus:/bus" -e UC_E2E_BUS=unix:path=/bus/bus -v "$ROOT:/work:ro" -v "$package:/in/package.$kind:ro" -v "$out:/out" -v "$pubkey:/in/pubkey.b64:ro" -e KIND="$kind" "$image" bash -c '
set -e
if [ "$KIND" = deb ]; then
 apt-get update -qq
 apt-get install -y -qq --no-install-recommends python3-gi python3-dbus gir1.2-gtk-3.0 x11-utils /in/package.deb > /out/install.log 2>&1
 dpkg-query -W uniclipboard > /out/package.txt
 dpkg -S /usr/bin/uniclipboard >> /out/package.txt
else
 dnf -y -q install python3-gobject python3-dbus xdpyinfo /in/package.rpm > /out/install.log 2>&1
 rpm -q uniclipboard > /out/package.txt
 rpm -qf /usr/bin/uniclipboard >> /out/package.txt
fi
cat /etc/os-release > /out/host.txt
uname -a >> /out/host.txt
sha256sum /in/package.* /usr/bin/uniclipboard /usr/bin/uniclipd > /out/hashes.txt
python3 /work/apps/gui-go/e2e/linux_package_update_run.py --kind "$KIND" --out /out --pubkey /in/pubkey.b64
' > "$out/run.log" 2>&1
