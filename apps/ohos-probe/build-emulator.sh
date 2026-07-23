#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workspace_root="$(cd "$project_root/../.." && pwd)"
deveco_contents="${DEVECO_STUDIO_CONTENTS:-/Applications/DevEco-Studio.app/Contents}"
sdk_root="$deveco_contents/sdk"
native_root="$sdk_root/default/openharmony/native"
target="aarch64-unknown-linux-ohos"
target_dir="${UC_OHOS_TARGET_DIR:-${TMPDIR:-/tmp}/uniclipboard-ohos-target}"
rust_linker="$native_root/llvm/bin/aarch64-unknown-linux-ohos-clang"
rust_cxx="$native_root/llvm/bin/aarch64-unknown-linux-ohos-clang++"
rust_ar="$native_root/llvm/bin/llvm-ar"
cmake="$native_root/build-tools/cmake/bin/cmake"
library_dir="$project_root/entry/libs/arm64-v8a"
dist_dir="${UC_OHOS_DIST_DIR:-$target_dir/uc-ohos-napi-dist/ohos}"

for executable in "$rust_linker" "$rust_cxx" "$rust_ar" "$cmake"; do
  if [[ ! -x "$executable" ]]; then
    echo "required DevEco tool is unavailable: $executable" >&2
    exit 1
  fi
done

CARGO_TARGET_DIR="$target_dir" \
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_OHOS_LINKER="$rust_linker" \
CC_aarch64_unknown_linux_ohos="$rust_linker" \
CXX_aarch64_unknown_linux_ohos="$rust_cxx" \
AR_aarch64_unknown_linux_ohos="$rust_ar" \
CMAKE="$cmake" \
  cargo build \
    --manifest-path "$workspace_root/Cargo.toml" \
    -p uc-ohos-napi \
    --target "$target" \
    --locked

mkdir -p "$library_dir"
cp "$target_dir/$target/debug/libuc_ohos_napi.so" "$library_dir/libuc_ohos_napi.so"
mkdir -p "$dist_dir"
cp "$library_dir/libuc_ohos_napi.so" "$dist_dir/libuc_ohos_napi.so"
cp "$project_root/entry/src/main/cpp/types/libuc_ohos_napi/index.d.ts" \
  "$dist_dir/index.d.ts"
shasum -a 256 "$dist_dir/libuc_ohos_napi.so" | awk '{print $1}' \
  > "$dist_dir/uc-ohos-napi.checksum.txt"
version="$(cargo pkgid --manifest-path "$workspace_root/Cargo.toml" -p uc-ohos-napi)"
version="${version##*#}"
commit="$(git -C "$workspace_root" rev-parse HEAD)"
printf 'core-v%s\n' "$version" > "$dist_dir/core-version.txt"
printf '%s\n' "$commit" > "$dist_dir/source-commit.txt"
printf 'sdk.dir=%s\n' "$sdk_root" > "$project_root/local.properties"

cd "$project_root"
"$deveco_contents/tools/ohpm/bin/ohpm" install --all
DEVECO_SDK_HOME="$sdk_root" \
  "$deveco_contents/tools/hvigor/bin/hvigorw" \
  --mode module \
  -p product=default \
  -p module=entry@default \
  -p buildMode=debug \
  assembleHap \
  --no-daemon

"$project_root/sign-emulator.sh"
