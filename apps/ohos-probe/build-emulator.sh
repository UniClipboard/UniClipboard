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
har_root="$project_root/engine"
har_library_dir="$har_root/libs/arm64-v8a"
declaration_source="$workspace_root/crates/uc-ohos-napi/ohos/index.d.ts"
entry_declaration_dir="$project_root/entry/src/main/cpp/types/libuc_ohos_napi"
har_declaration_dir="$har_root/src/main/cpp/types/libuc_ohos_napi"
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

mkdir -p "$library_dir" "$har_library_dir" "$entry_declaration_dir" "$har_declaration_dir"
cp "$target_dir/$target/debug/libuc_ohos_napi.so" "$library_dir/libuc_ohos_napi.so"
cp "$target_dir/$target/debug/libuc_ohos_napi.so" "$har_library_dir/libuc_ohos_napi.so"
cp "$declaration_source" "$entry_declaration_dir/index.d.ts"
cp "$declaration_source" "$har_declaration_dir/index.d.ts"
cp "$entry_declaration_dir/oh-package.json5" "$har_declaration_dir/oh-package.json5"
cp "$entry_declaration_dir/package.json" "$har_declaration_dir/package.json"
mkdir -p "$dist_dir"
cp "$library_dir/libuc_ohos_napi.so" "$dist_dir/libuc_ohos_napi.so"
cp "$declaration_source" "$dist_dir/index.d.ts"
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

DEVECO_SDK_HOME="$sdk_root" \
  "$deveco_contents/tools/hvigor/bin/hvigorw" \
  --mode module \
  -p product=default \
  -p module=engine@default \
  -p buildMode=release \
  assembleHar \
  --no-daemon

har_path="$(find "$har_root/build" -type f -name '*.har' -print -quit)"
if [[ -z "$har_path" ]]; then
  echo "HarmonyOS HAR was not produced" >&2
  exit 1
fi
cp "$har_path" "$dist_dir/UniClipboardEngine.har"
cp "$project_root/entry/build/default/outputs/default/entry-default-signed.hap" \
  "$dist_dir/UniClipboardEngineProbe.hap"

for required_path in \
  package/Index.d.ets \
  package/libs/arm64-v8a/libuc_ohos_napi.so \
  package/oh-package.json5 \
  package/src/main/cpp/types/libuc_ohos_napi/index.d.ts; do
  if ! tar -tzf "$dist_dir/UniClipboardEngine.har" | grep -Fxq "$required_path"; then
    echo "HarmonyOS HAR is missing $required_path" >&2
    exit 1
  fi
done

if ! tar -xOzf "$dist_dir/UniClipboardEngine.har" package/oh-package.json5 \
  | grep -Fq '"name":"@uniclipboard/engine"'; then
  echo "HarmonyOS HAR has an unexpected package name" >&2
  exit 1
fi
if ! tar -xOzf "$dist_dir/UniClipboardEngine.har" package/oh-package.json5 \
  | grep -Fq "\"version\":\"$version\""; then
  echo "HarmonyOS HAR version does not match core-v$version" >&2
  exit 1
fi

shasum -a 256 "$dist_dir/UniClipboardEngine.har" | awk '{print $1}' \
  > "$dist_dir/UniClipboardEngine.har.checksum.txt"
shasum -a 256 "$dist_dir/UniClipboardEngineProbe.hap" | awk '{print $1}' \
  > "$dist_dir/UniClipboardEngineProbe.checksum.txt"

echo "OK: $dist_dir/UniClipboardEngine.har"
echo "OK: $dist_dir/UniClipboardEngineProbe.hap"
