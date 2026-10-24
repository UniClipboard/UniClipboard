#!/usr/bin/env bash
# Reuse the Engine cache owner at an immutable automation revision.
# Keep this pin independent of the product's Engine dependency revision.
set -euo pipefail
revision=a8e21a0505159e7b71630b2178746d72f985ac76
root="${UC_TOOLS_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/uniclipboard-engine/tools}/cache-entry/$revision"
manifest=$(cat <<'SUMS'
79ac6e7476726bc86e7d9496f3152786b9e24239cdb5696dbef54452e9902254  mbx.sh
0c245894ef15026973febabb458a4895bf6b061b6afc1d6430557fd031469e25  env.sh
157f58b9df33ff4d2c635c5c9756d17594f21cc5cbac1eff0c44efc5c599bf6a  bin/cargo
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
  # Install the compiler policy before exposing any entry to later steps. The
  # transport identity stays separate even when restore is skipped or fails.
  commands=$(mktemp "$RUNNER_TEMP/desktop-mbx-commands.XXXXXX")
  GITHUB_ENV="$commands" bash "$root/scripts/build-cache/start-ci-mbx.sh"
  identity=$(sed -n 's/^UC_ENGINE_MBX_ENV_FILE=//p' "$commands")
  [[ -n "$identity" && -f "$identity" ]] || exit 1
  policy=$(mktemp "$RUNNER_TEMP/desktop-mbx-local.XXXXXX")
  cp "$(dirname "${BASH_SOURCE[0]}")/ci-local-policy.sh" "$policy"
  # Never source the identity from the compiler process tree.
  sed '/^UC_ENGINE_MBX_ENV_FILE=/d' "$commands" >> "$GITHUB_ENV"
  printf 'UC_ENGINE_MBX_ENV_FILE=%s\nUC_MBX_SNAPSHOT_ENV_FILE=%s\n' "$policy" "$identity" >> "$GITHUB_ENV"
  rm "$commands"
  exit 0
fi
printf '%s\n' "$root"
