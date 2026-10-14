#!/usr/bin/env bash
# Reuse the Engine cache owner at an immutable automation revision.
# Keep this pin independent of the product's Engine dependency revision.
set -euo pipefail
revision=2f9afc9956b0cda82e65c912f0c0f6dc6aa51256
root="${UC_TOOLS_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/uniclipboard-engine/tools}/cache-entry/$revision"
manifest=$(cat <<'SUMS'
723c37a5fa89c4993a551f2c37aa586265358e4bb888691a81dd5393ed02c750  mbx.sh
0c245894ef15026973febabb458a4895bf6b061b6afc1d6430557fd031469e25  env.sh
c421d187d426e8fd697ba726070dd15cb96888b6caaeb2ff21aa6e8b2a73ec18  bin/cargo
7c35314bfd3e19028a6d7445d9c593258871e4d651fa18d3bb52b09fc34e7270  start-ci-mbx.sh
23add27242f7f5abb3b9af704a2434f8dd3d9223f0fedf0f18ce1ec7c95c1e6d  prepare-ci-network.sh
SUMS
)
verify() (
  cd "$1"
  while read -r expected file; do
    if command -v sha256sum >/dev/null; then
      actual=$(sha256sum "$file" | cut -d' ' -f1)
    else
      actual=$(shasum -a 256 "$file" | cut -d' ' -f1)
    fi
    [[ "$actual" == "$expected" ]] || exit 1
  done <<< "$manifest"
)
if [[ ! -d "$root/scripts/build-cache" ]] || ! verify "$root/scripts/build-cache" 2>/dev/null; then
  mkdir -p "$(dirname "$root")"
  staging=$(mktemp -d "$root.staging.XXXXXX")
  trap 'rm -rf "$staging"' EXIT
  mkdir -p "$staging/scripts/build-cache/bin"
  while read -r _checksum file; do
    curl -fsSL --retry 3 -o "$staging/scripts/build-cache/$file" \
      "https://raw.githubusercontent.com/UniClipboard/Engine/$revision/scripts/build-cache/$file"
  done <<< "$manifest"
  verify "$staging/scripts/build-cache"
  chmod +x "$staging/scripts/build-cache/bin/cargo"
  mkdir -p "$root/scripts/build-cache/bin"
  cp "$staging/scripts/build-cache/"*.sh "$root/scripts/build-cache/"
  cp "$staging/scripts/build-cache/bin/cargo" "$root/scripts/build-cache/bin/"
fi
if [[ "${1:-}" == --ci ]]; then
  exec bash "$root/scripts/build-cache/start-ci-mbx.sh"
fi
printf '%s\n' "$root"
