#!/usr/bin/env bash
# Run an opt-in Desktop command through pinned mbx without changing global Cargo.
set -euo pipefail

MBX_VERSION=1.18.0

fail() {
  printf 'mbx: %s\n' "$1" >&2
  exit 1
}

# Archive SHA-256 values are from the v1.18.0 SHA256SUMS release manifest.
pinned_release() {
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) echo aarch64-apple-darwin 70ab23933b3745205125174120bd930c0116c1ef3ac38786122e5651636b9aea ;;
    Linux-x86_64) echo x86_64-unknown-linux-gnu 92833c87261ea0c65fee52898ea67605b152b516fc0a3d8dee5a276601a8924c ;;
    Linux-aarch64 | Linux-arm64)
      echo aarch64-unknown-linux-gnu ab0f4af3e98295bc35f448d453cfe15931bb02342d48163b2ede94583f937cdc ;;
    *) fail "unsupported platform: $(uname -s)-$(uname -m)" ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

install_mbx() {
  local triple expected root archive staging
  read -r triple expected <<<"$(pinned_release)"
  root="${UC_TOOLS_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/uniclipboard-engine/tools}/mbx/$MBX_VERSION/$triple"
  if [[ ! -x "$root/mbx" ]]; then
    mkdir -p "$(dirname "$root")"
    staging=$(mktemp -d "$root.staging.XXXXXX")
    archive="$staging/mbx-$triple.tar.gz"
    curl -fsSL --retry 3 -o "$archive" \
      "https://github.com/jdx/mr-boxington/releases/download/v$MBX_VERSION/mbx-$triple.tar.gz"
    if [[ "$(sha256_of "$archive")" != "$expected" ]]; then
      rm -rf "$staging"
      fail "download checksum mismatch for mbx $MBX_VERSION"
    fi
    tar -xzf "$archive" -C "$staging" mbx
    mkdir -p "$root"
    mv "$staging/mbx" "$root/mbx"
    rm -rf "$staging"
  fi
  mkdir -p "$root/shim"
  ln -sfn "$root/mbx" "$root/shim/cargo"
  MBX_BIN="$root/mbx"
  MBX_SHIM="$root/shim"
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" || $# -eq 0 ]]; then
  printf 'Usage: bun mbx:exec -- <command...>\nExample: bun mbx:exec -- bun wails:dev:profile a\n'
  exit 0
fi
[[ "${1:-}" == "--exec" ]] || fail "expected --exec"
shift
[[ "${1:-}" == "--" ]] && shift
[[ $# -gt 0 ]] || fail "missing command"

if [[ -n "${MBX_CACHE_DIR:-}" && ! -d "$(dirname "$MBX_CACHE_DIR")" ]]; then
  fail "MBX_CACHE_DIR parent is missing: $(dirname "$MBX_CACHE_DIR")"
fi

install_mbx
[[ "$("$MBX_BIN" --version)" == "mbx $MBX_VERSION" ]] || fail "cached mbx version does not match $MBX_VERSION"

# mbx forwards to an existing rustc wrapper, so remove sccache only here.
unset RUSTC_WRAPPER CARGO_BUILD_RUSTC_WRAPPER
export MBX_TARGET_VIEWS=0 MBX_TARGET_SEED=0 MBX_LEARNED_INCREMENTAL=0
export MBX_GC_MAX_SIZE="${MBX_GC_MAX_SIZE:-20GiB}"
export MBX_DISPLAY="${MBX_DISPLAY:-plain}" MBX_SUMMARY=full
export PATH="$MBX_SHIM:$PATH"

set +e
"$@"
status=$?
printf 'mbx: cumulative cache statistics (this store, all worktrees):\n'
"$MBX_BIN" stats
exit "$status"
