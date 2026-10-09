#!/usr/bin/env bash
# Build packaging/aur/uniclipboard-git/PKGBUILD with makepkg in a clean Arch container, against one exact commit.
#
#   build-aur-package.sh <repository-dir> <commit> <output-dir>
#
# Run as root inside `archlinux:base-devel` (a disposable container: it creates a build user with passwordless sudo,
# which makepkg --syncdeps needs to install the makedepends). The PKGBUILD's source line is rewritten to a local
# clone of <repository-dir> pinned to <commit>, so the build uses the code under review, not whatever main is at the
# time. The published PKGBUILD keeps the GitHub URL; only this copy is rewritten.
#
# Writes to <output-dir>: the package, SHA256SUMS, the PKGBUILD actually built, build.log, namcap output, the package
# file list and metadata, and manifest.txt (commit, pacman package versions of the toolchain, host). It reads from
# the Arch mirrors, GitHub (the Engine git dependency) and the Go/npm/crates
# registries, and writes to none of them.
set -euo pipefail
REPO="$(cd "$1" && pwd)"; COMMIT="$2"; OUT="$3"
[[ "$(id -u)" == 0 ]] || { echo "run as root in a disposable container" >&2; exit 2; }
mkdir -p "$OUT"; OUT="$(cd "$OUT" && pwd)"

# pacman's download sandbox needs seccomp and Landlock, which an emulated amd64 container (QEMU or Rosetta on an Apple
# silicon host) does not provide. Native runners do not need this.
if [[ "${UC_PACMAN_DISABLE_SANDBOX:-0}" == 1 ]]; then sed -i '/^\[options\]/a DisableSandbox' /etc/pacman.conf; fi

# A full upgrade, not -Sy: a partial upgrade can pair a new webkit2gtk with an old glib.
pacman -Syu --noconfirm --needed git namcap sudo
git config --system --add safe.directory '*'
id builder >/dev/null 2>&1 || useradd -m -s /bin/bash builder
echo 'builder ALL=(ALL) NOPASSWD: ALL' >/etc/sudoers.d/builder && chmod 440 /etc/sudoers.d/builder

BUILD="${UC_AUR_BUILD_DIR:-/home/builder/build}"
install -d "$BUILD" && find "$BUILD" -mindepth 1 -delete && chown builder:builder "$BUILD"
git -C "$REPO" cat-file -e "$COMMIT^{commit}"
# Clone with tags so pkgver() (git describe) matches what the AUR build computes.
sudo -u builder git clone --quiet "file://$REPO" "$BUILD/src-mirror"
sudo -u builder git -C "$BUILD/src-mirror" checkout --quiet "$COMMIT"
# The PKGBUILD under test is the one in that commit, not whatever the working tree holds.
sed -e "s#^source=.*#source=(\"\$_pkgname::git+file://$BUILD/src-mirror\#commit=$COMMIT\")#" \
  "$BUILD/src-mirror/packaging/aur/uniclipboard-git/PKGBUILD" >"$BUILD/PKGBUILD"
chown builder:builder "$BUILD/PKGBUILD"
grep -n '^source=' "$BUILD/PKGBUILD"

sudo -u builder bash -c "cd '$BUILD' && makepkg --syncdeps --noconfirm --cleanbuild --nocheck" 2>&1 | tee "$OUT/build.log"
PKG="$(ls "$BUILD"/*.pkg.tar.zst)"
[[ "$(echo "$PKG" | wc -l)" == 1 ]] || { echo "expected exactly one package, got: $PKG" >&2; exit 1; }

cp "$PKG" "$BUILD/PKGBUILD" "$OUT/"
(cd "$OUT" && sha256sum "$(basename "$PKG")" >SHA256SUMS)
pacman -Qip "$PKG" >"$OUT/package-info.txt"
pacman -Qlp "$PKG" >"$OUT/package-files.txt"
{ namcap "$BUILD/PKGBUILD"; namcap "$PKG"; } >"$OUT/namcap.txt" 2>&1 || true
{
  echo "commit=$COMMIT"
  echo "package=$(basename "$PKG")"
  echo "host=$(uname -sm)"
  pacman -Q go rust bun jq webkit2gtk-4.1 gtk3 gtk-layer-shell libsoup3 libx11
} >"$OUT/manifest.txt"
cat "$OUT/SHA256SUMS" "$OUT/manifest.txt"
