#!/usr/bin/env bash
# Runs INSIDE the packaging build image (repository at $UC_WORK=/work, cache at $UC_CACHE=/cache).
# Builds the SHIPPED daemon the way scripts/stage-daemon.mjs does: `cargo build --release -p uc-daemon --bin uniclipd`
# with the repository's [profile.release] (thin LTO, opt-level z) and the locked dependency graph, so the Engine comes
# from the immutable git revision recorded in Cargo.lock. Writes the binary and an evidence file to $UC_CACHE/out-release.
#
# The evidence describes only what was observed here: source head, whether the daemon inputs were dirty, the build mode
# (a CARGO_PROFILE_RELEASE_* override, which scripts/ci/configure-build-mode.mjs sets for test builds, is recorded as a test
# build and is rejected by package_linux.py), the target, the host that built it, Cargo.lock's hash and the Engine pin, and
# whether compile-time telemetry values were injected (booleans only, never the values).
set -euo pipefail
WORK="${UC_WORK:-/work}"; CACHE="${UC_CACHE:-/cache}"
cd "$WORK"
git config --global --add safe.directory "$WORK"
export CARGO_HOME="$CACHE/cargo" RUSTUP_HOME="$CACHE/rustup" CARGO_TARGET_DIR="$CACHE/target"
export PATH="$CARGO_HOME/bin:$PATH"
OUT="$CACHE/out-release"; mkdir -p "$OUT"
if ! command -v cargo >/dev/null; then
  # No toolchain is chosen here: rust-toolchain.toml in the repository pins the channel and rustup installs exactly that.
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
fi
# Snapshot transport runs in the same container, with the same compiler and CAS
# as the workload. GitHub command files stay inside this disposable container.
if [[ "${UC_COMPILE_CACHE_BACKEND:-}" == r2 ]]; then
  export RUNNER_TEMP="$CACHE/snapshot-tmp" MBX_CACHE_DIR="$CACHE/mbx"
  export GITHUB_ENV="$RUNNER_TEMP/github-env" MBX_CACHE_EXPORT_GROUP=linux-gui
  export UC_MBX_SNAPSHOT_SOURCE_ROOT="$WORK"
  mkdir -p "$RUNNER_TEMP"
  : > "$GITHUB_ENV"
  if ! bash scripts/ci/mbx-snapshot.sh restore --scene linux-gui; then
    echo '::warning::Linux snapshot setup failed; compiling with local MBX'
  fi
  # The command file contains paths only; the SDK identity remains in its
  # original restricted mount and the native policy contains no credentials.
  set -a
  source "$GITHUB_ENV"
  set +a
fi
# The source that determines the daemon: anything dirty here makes the build non-immutable.
dirty="$(git status --porcelain -- Cargo.toml Cargo.lock rust-toolchain.toml crates apps/daemon tools packages 2>/dev/null || true)"
overrides="$(env | grep '^CARGO_PROFILE_RELEASE_' | sort | tr '\n' ' ' || true)"
mode=release; [ -z "$overrides" ] || mode="test (profile override: $overrides)"
present() { [ -n "${!1:-}" ] && echo true || echo false; }
{
  echo "head=$(git rev-parse HEAD)"
  echo "daemon_source_dirty=$([ -n "$dirty" ] && echo true || echo false)"
  echo "build_mode=$mode"
  echo "command=cargo build --locked --release -p uc-daemon --bin uniclipd"
  echo "target=$(rustc --print host-tuple 2>/dev/null || rustc -vV | sed -n 's/^host: //p')"
  echo "build_host_machine=$(uname -m)"
  echo "build_host_os=$(. /etc/os-release && echo "$PRETTY_NAME")"
  echo "build_host_glibc=$(ldd --version | head -1)"
  echo "build_image=${UC_BUILD_IMAGE:-unspecified}"
  echo "cargo_lock_sha256=$(sha256sum Cargo.lock | cut -d' ' -f1)"
  echo "telemetry_sentry_dsn_injected=$(present SENTRY_DSN)"
  echo "telemetry_posthog_key_injected=$(present POSTHOG_PROJECT_KEY)"
  echo "telemetry_app_env=${APP_ENV:-}"
  grep -n 'uc-engine' Cargo.toml | head -3
  awk '/^name = "uc-engine"/{f=1} f{print} /^$/{if(f)exit}' Cargo.lock
  rustc -Vv
} > "$OUT/build-evidence.txt"
if [[ -n "${UC_ENGINE_MBX_BIN:-}" ]]; then
  export PATH="$UC_ENGINE_MBX_BIN:$PATH"
  cargo build --locked --release -p uc-daemon --bin uniclipd 2>&1 | tail -n 40
else
  UC_TOOLS_DIR="$CACHE/tools" MBX_CACHE_DIR="$CACHE/mbx" \
    bash scripts/build-cache/mbx.sh --exec cargo build --locked --release -p uc-daemon --bin uniclipd 2>&1 | tail -n 40
fi
if [[ "${UC_COMPILE_CACHE_BACKEND:-}" == r2 && "${UC_MBX_PUBLISH_SNAPSHOT:-}" == true ]]; then
  if ! bash scripts/ci/mbx-snapshot.sh save --scene linux-gui; then
    echo '::warning::Linux snapshot publication failed'
  fi
fi
cp "$CACHE/target/release/uniclipd" "$OUT/uniclipd"
sha256sum "$OUT/uniclipd" >> "$OUT/build-evidence.txt"
file "$OUT/uniclipd" >> "$OUT/build-evidence.txt"
echo done >> "$OUT/build-evidence.txt"
