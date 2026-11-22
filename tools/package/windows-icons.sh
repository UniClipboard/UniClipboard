#!/bin/sh
# Regenerate the checked-in Windows executable resources from the app ICO.
set -eu
cd "$(dirname "$0")/../.."
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
printf '1 ICON "%s/app/desktop/internal/appicons/icon.ico"\n' "$PWD" > "$work/icon.rc"
"${WINDRES:-x86_64-w64-mingw32-windres}" -i "$work/icon.rc" -O res -o "$work/icon.res"
for arch in amd64 arm64; do
  machine=ARM64
  if [ "$arch" = amd64 ]; then machine=X64; fi
  "${LLVM_CVTRES:-llvm-cvtres}" /machine:"$machine" /timestamp:0 /out:"app/desktop/cmd/unic-desktop/icon_windows_$arch.syso" "$work/icon.res"
done
