#!/usr/bin/env bash
# Render the AUR snapshot using the PKGBUILD's own version function.
set -euo pipefail
repo="$(cd "$1" && pwd)"
output="$2"
version="$(
  source "$repo/packaging/aur/uniclipboard-git/PKGBUILD"
  _pkgname="$repo"
  pkgver
)"
[[ -n "$version" && "$version" =~ ^[[:alnum:].+_]+$ ]] || { echo 'invalid AUR version' >&2; exit 1; }
sed "s/^pkgver=.*/pkgver=$version/" "$repo/packaging/aur/uniclipboard-git/PKGBUILD" > "$output"
printf '%s\n' "$version"
