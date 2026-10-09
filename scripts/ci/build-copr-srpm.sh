#!/usr/bin/env bash
# Render packaging/uniclipboard.spec for one release and build the source rpm that COPR rebuilds.
#
#   build-copr-srpm.sh <upstream-version> <directory with the upstream rpms> <output-dir>
#
# <upstream-version> is the release version without the v prefix, e.g. 1.2.0 or 1.2.0-alpha.1. The directory must hold
# UniClipboard-<upstream-version>-1.x86_64.rpm and UniClipboard-<upstream-version>-1.aarch64.rpm, the names the spec lists
# as Source0 and Source1 and the names scripts/collect-release-assets.py accepts. Needs rpmbuild (package rpm-build).
set -euo pipefail
TAG="$1"; RPMS="$(cd "$2" && pwd)"; mkdir -p "$3"; OUT="$(cd "$3" && pwd)"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

# An rpm version cannot contain "-". Turn the pre-release separator into "~", which sorts below any character, so
# 1.2.0~alpha.1 is older than 1.2.0.
RPM_VERSION="$(printf '%s' "$TAG" | sed 's/-/~/')"

TOP="$(mktemp -d)"
trap 'rm -rf "$TOP"' EXIT
mkdir -p "$TOP"/{SOURCES,SPECS}
for arch in x86_64 aarch64; do
  src="$RPMS/UniClipboard-${TAG}-1.${arch}.rpm"
  [[ -f "$src" ]] || { echo "missing upstream rpm: $src" >&2; exit 1; }
  cp "$src" "$TOP/SOURCES/"
done
# The placeholders are replaced in the file itself, not only through --define: the source rpm goes into the COPR build
# chroot, where no --define exists, so the version has to be fixed inside the spec it carries.
sed -e "s/@VERSION@/${RPM_VERSION}/g" -e "s/@UPSTREAM_TAG@/${TAG}/g" \
  "$ROOT/packaging/uniclipboard.spec" >"$TOP/SPECS/uniclipboard.spec"

# One source rpm for both architectures: its file name carries no architecture, so building per architecture would
# overwrite it. The spec picks the matching Source with %ifarch.
rpmbuild -bs "$TOP/SPECS/uniclipboard.spec" --define "_topdir $TOP"
cp "$TOP"/SRPMS/*.src.rpm "$OUT/"
ls -l "$OUT"
