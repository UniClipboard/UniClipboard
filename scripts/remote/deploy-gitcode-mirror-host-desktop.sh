#!/bin/sh
# Installs the Desktop GitCode mirror wrapper and upload script on the mirror
# host, owned by root so the CI key cannot change them. Run by a maintainer
# after either file changes; the workflow refuses to run against a script
# that differs from its commit.
#
# Installs alongside Mobile's own gitcode-mirror-host.py /
# mirror-android-apk-to-gitcode.mjs under the same /opt/uniclip-mirror/
# directory and the same unprivileged uniclip-mirror user — this is the
# Desktop-specific pair of files, under different names, so neither product
# can overwrite or impersonate the other's forced command.
#
#   scripts/remote/deploy-gitcode-mirror-host-desktop.sh sha   # an ssh alias with root access
set -eu
host=${1:?usage: deploy-gitcode-mirror-host-desktop.sh <ssh host with root access>}
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)

scp "$here/gitcode-mirror-host-desktop.py" "$host:/tmp/gitcode-mirror-host-desktop.py"
scp "$repo/scripts/mirror-desktop-installers-to-gitcode.mjs" "$host:/tmp/mirror-desktop-installers-to-gitcode.mjs"
ssh "$host" 'set -e
install -d -m 755 -o root -g root /opt/uniclip-mirror
install -m 755 -o root -g root /tmp/gitcode-mirror-host-desktop.py /opt/uniclip-mirror/gitcode-mirror-host-desktop.py
install -m 644 -o root -g root /tmp/mirror-desktop-installers-to-gitcode.mjs /opt/uniclip-mirror/mirror-desktop-installers-to-gitcode.mjs
rm -f /tmp/gitcode-mirror-host-desktop.py /tmp/mirror-desktop-installers-to-gitcode.mjs
sha256sum /opt/uniclip-mirror/gitcode-mirror-host-desktop.py /opt/uniclip-mirror/mirror-desktop-installers-to-gitcode.mjs'
echo "expected script sha256: $(shasum -a 256 "$repo/scripts/mirror-desktop-installers-to-gitcode.mjs" | cut -d' ' -f1)"
