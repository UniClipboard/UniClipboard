#!/bin/sh
set -eu
MAC_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
swift build --package-path "$MAC_DIR" -c release
BIN_DIR=$(swift build --package-path "$MAC_DIR" -c release --show-bin-path)
APP_DIR="$MAC_DIR/.build/Unic.app"
mkdir -p "$APP_DIR/Contents/MacOS"
cp "$BIN_DIR/Unic" "$APP_DIR/Contents/MacOS/Unic"
cat > "$APP_DIR/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.unic.swiftui.preview</string>
<key>CFBundleName</key><string>Unic</string>
<key>CFBundleDisplayName</key><string>UniClipboard</string>
<key>CFBundleExecutable</key><string>Unic</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$APP_DIR"
printf '%s\n' "$APP_DIR"
