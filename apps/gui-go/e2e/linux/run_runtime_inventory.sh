#!/usr/bin/env bash
# Runtime library inventory of one AppImage on one clean host image (issue #1903), then the content assertions against that inventory.
#   run_runtime_inventory.sh <new out dir> <AppImage> <package-manifest.json> <host image>
# The host image is any Linux image with python3, Xvfb, dbus-daemon, FUSE 2 and the host-owned libraries the AppImage leaves out
# (Dockerfile.17c4-runtime builds Debian/Ubuntu ones, Dockerfile.runtime-opensuse the openSUSE one). Set UC_ANALYSIS_IMAGE to an image with python3 and binutils (readelf) when the host image has none.
# Native architecture only: a container for another architecture runs under emulation and proves nothing about that architecture.
set -euo pipefail
out="${1:?new out dir}"; appimage="${2:?AppImage}"; manifest="${3:?package-manifest.json}"; image="${4:?host image}"
root="$(cd "$(dirname "$0")/../../../.." && pwd)"
[ ! -e "$out" ] || { echo "$out exists" >&2; exit 2; }
mkdir -p "$out"; out="$(cd "$out" && pwd)"
appimage="$(cd "$(dirname "$appimage")" && pwd)/$(basename "$appimage")"; manifest="$(cd "$(dirname "$manifest")" && pwd)/$(basename "$manifest")"
# Failure model C7: an emulated container reports the emulated architecture in uname, so compare the engine's own architecture with the image's.
engine_arch="$(docker version --format '{{.Server.Arch}}')"; image_arch="$(docker image inspect "$image" --format '{{.Architecture}}')"
[ "$engine_arch" = "$image_arch" ] || { echo "image $image is $image_arch but the engine is $engine_arch: that would run under emulation" >&2; exit 3; }
docker image inspect "$image" --format '{{.Id}} {{.Architecture}}' > "$out/host-image.txt"
echo "docker-engine-arch: $engine_arch (native: same as the image)" >> "$out/host-image.txt"
docker run --rm --init --device /dev/fuse --cap-add SYS_ADMIN --security-opt apparmor=unconfined \
  -v "$root:/work:ro" -v "$appimage:/in/app.AppImage:ro" -v "$out:/out" "$image" bash -c '
set -euo pipefail
id uc >/dev/null 2>&1 || useradd --create-home --uid 1500 uc
{ cat /etc/os-release; uname -m; ldd --version 2>&1 | sed -n 1p; ldconfig -p | grep -E "libglib-2.0.so|libGL.so.1|libEGL.so.1|libgbm.so|libwayland-client.so"
  (dpkg-query -W libglib2.0-0t64 libglib2.0-0 libgl1-mesa-dri 2>/dev/null || rpm -q glibc glib2-tools Mesa-dri 2>/dev/null) || true; } > /out/host.txt 2>&1
install -d -o uc -m 755 /home/uc/mounted /home/uc/extracted /home/uc/content
for d in mounted extracted content; do install -o uc -m 755 /in/app.AppImage /home/uc/$d/app.AppImage; done
chown -R uc /out
status=0
su uc -c "python3 /work/apps/gui-go/e2e/linux/runtime_library_inventory.py observe /home/uc/mounted/app.AppImage /out/mounted" > /out/observe-mounted.log 2>&1 || status=1
su uc -c "python3 /work/apps/gui-go/e2e/linux/runtime_library_inventory.py observe /home/uc/extracted/app.AppImage /out/extracted --extract" > /out/observe-extracted.log 2>&1 || status=1
cd /home/uc/content && su uc -c "./app.AppImage --appimage-extract" > /out/extract-content.log 2>&1 && cp -a squashfs-root /out/squashfs-root
exit $status
' > "$out/run.log" 2>&1 && rc=0 || rc=$?
# Classification and assertions only read files and run readelf: any image with python3 and binutils will do (the clean host need not have them).
analysis="${UC_ANALYSIS_IMAGE:-$image}"
docker run --rm -v "$root:/work:ro" -v "$manifest:/in/package-manifest.json:ro" -v "$out:/out" "$analysis" bash -c '
set -uo pipefail
status=0
python3 -B /work/apps/gui-go/e2e/linux/runtime_library_inventory.py merge /out/squashfs-root /out/runtime-inventory.json /out/mounted/observation.json /out/extracted/observation.json > /out/merge.log 2>&1 || status=1
python3 -B /work/apps/gui-go/e2e/linux/appimage_content_check.py /out/squashfs-root /in/package-manifest.json /out/content-check.json --runtime-inventory /out/runtime-inventory.json > /out/content-check.log 2>&1 || status=1
rm -rf /out/squashfs-root  # reproducible from the AppImage; its hashes are in runtime-inventory.json
exit $status
' >> "$out/run.log" 2>&1 && true || rc=$?
echo "analysis-image: $analysis" >> "$out/host-image.txt"
echo "rc=$rc" | tee "$out/rc.txt"
exit "$rc"
