#!/usr/bin/env bash
# Runs INSIDE the uc-gui-go-linux-build image (repository at /work, cache volume at /cache).
# Builds the SHIPPED daemon the way scripts/prepare-sidecars.mjs does: `cargo build --release -p uc-daemon --bin uniclipd`
# with the repository's [profile.release] (thin LTO, opt-level z) and the locked dependency graph, so the Engine comes
# from the immutable git revision recorded in Cargo.lock. Writes the binary and an evidence file to /cache/out-release.
set -euo pipefail
cd /work
git config --global --add safe.directory /work
export CARGO_HOME=/cache/cargo RUSTUP_HOME=/cache/rustup CARGO_TARGET_DIR=/cache/target
export PATH="$CARGO_HOME/bin:$PATH"
OUT=/cache/out-release; mkdir -p "$OUT"
# The source that determines the daemon: anything dirty here makes the build non-immutable.
dirty="$(git status --porcelain -- Cargo.toml Cargo.lock rust-toolchain.toml crates apps/daemon tools packages 2>/dev/null || true)"
{
  echo "head=$(git rev-parse HEAD)"
  echo "daemon_source_dirty=$([ -n "$dirty" ] && echo true || echo false)"
  echo "build_mode=release (scripts/ci/configure-build-mode.mjs: UC_BUILD_MODE=release, no env override)"
  echo "command=cargo build --locked --release -p uc-daemon --bin uniclipd"
  grep -n 'uc-engine' Cargo.toml | head -3
  awk '/^name = "uc-engine"/{f=1} f{print} /^$/{if(f)exit}' Cargo.lock
  rustc -Vv
} > "$OUT/build-evidence.txt"
cargo build --locked --release -p uc-daemon --bin uniclipd 2>&1 | tail -n 40
cp /cache/target/release/uniclipd "$OUT/uniclipd"
sha256sum "$OUT/uniclipd" >> "$OUT/build-evidence.txt"
file "$OUT/uniclipd" >> "$OUT/build-evidence.txt"
echo done >> "$OUT/build-evidence.txt"
