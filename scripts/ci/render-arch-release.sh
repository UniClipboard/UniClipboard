#!/usr/bin/env bash
# Reuse the AUR build/package functions for an immutable stable release.
set -euo pipefail
repo="$(cd "$1" && pwd)"
output="$2"
version="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["version"])' "$repo/apps/gui-go/app.json")"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'only stable releases are supported' >&2; exit 1; }
commit="$(git -C "$repo" rev-parse HEAD)"
[[ "$(git -C "$repo" rev-parse "refs/tags/v$version^{commit}")" == "$commit" ]] || {
  echo 'HEAD must equal the app.json stable version tag' >&2; exit 1;
}
cat "$repo/packaging/aur/uniclipboard-git/PKGBUILD" > "$output"
cat >> "$output" <<OVERRIDES

# Generated release identity; build and file layout remain owned by the AUR recipe.
pkgname=uniclipboard
pkgver=$version
pkgrel=1
provides=()
conflicts=('uniclipboard-git' 'uniclipboard-bin')
source=("\$_pkgname::git+https://github.com/UniClipboard/UniClipboard.git#commit=$commit")
unset -f pkgver
OVERRIDES
printf '%s\n' "$version"
