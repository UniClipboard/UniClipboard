#!/bin/sh
# Regenerate the checked-in Windows executable resources (app ICO and PE VERSIONINFO).
set -eu
cd "$(dirname "$0")/../.."
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT HUP INT TERM
ver=$(python3 - <<'PY'
import tomllib
p = tomllib.load(open("release/products.toml", "rb"))["products"]["desktop"]
parts = [int(x) for x in p["version"].split(".")]
if len(parts) != 3:
    raise SystemExit("desktop version must have three numeric components")
build = int(p["build_number"])
if any(v < 0 or v > 65535 for v in (*parts, build)):
    raise SystemExit("Windows version component exceeds 65535")
print(f"{parts[0]},{parts[1]},{parts[2]},{build} {parts[0]}.{parts[1]}.{parts[2]}.{build}")
PY
)
ver_csv=${ver% *}
ver_str=${ver#* }
cat > "$work/icon.rc" <<EOF
1 ICON "$PWD/app/desktop/internal/appicons/icon.ico"

1 VERSIONINFO
FILEVERSION $ver_csv
PRODUCTVERSION $ver_csv
FILEFLAGSMASK 0x3f
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "UniClipboard\0"
      VALUE "FileDescription", "UniClipboard\0"
      VALUE "FileVersion", "$ver_str\0"
      VALUE "InternalName", "unic-desktop\0"
      VALUE "OriginalFilename", "unic-desktop.exe\0"
      VALUE "ProductName", "UniClipboard\0"
      VALUE "ProductVersion", "$ver_str\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
EOF
"${WINDRES:-x86_64-w64-mingw32-windres}" -i "$work/icon.rc" -O res -o "$work/icon.res"
for arch in amd64 arm64; do
  machine=ARM64
  if [ "$arch" = amd64 ]; then machine=X64; fi
  "${LLVM_CVTRES:-llvm-cvtres}" /machine:"$machine" /timestamp:0 /out:"app/desktop/cmd/unic-desktop/icon_windows_$arch.syso" "$work/icon.res"
done
