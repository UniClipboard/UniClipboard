#!/usr/bin/env bash
# Package the CLI release archive: `uniclip` plus the `uniclipd` daemon it
# spawns as a sibling (ADR-008 D13). macOS binaries are signed and notarized.
#
# Usage: package-cli.sh <target-triple> <uniclip-path> <uniclipd-path>
#
# Binaries are copied into a staging directory before signing, so the build
# outputs (and the Tauri sidecar staged from the same daemon) stay untouched.
# macOS requires APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, APPLE_ID,
# APPLE_PASSWORD and APPLE_TEAM_ID. Writes `archive=<file>` to GITHUB_OUTPUT.
set -euo pipefail

TARGET=$1
CLI_BIN=$2
DAEMON_BIN=$3

VERSION=$(node -p "require('./package.json').version")
STAGE="${RUNNER_TEMP:?}/cli-package-stage"
EXE=""
case "$TARGET" in
  *windows*) EXE=".exe" ;;
esac

rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$CLI_BIN" "$STAGE/uniclip$EXE"
cp "$DAEMON_BIN" "$STAGE/uniclipd$EXE"

if [[ "$TARGET" == *-apple-darwin ]]; then
  KEYCHAIN_PATH="$RUNNER_TEMP/cli-signing.keychain-db"
  CERTIFICATE_PATH="$RUNNER_TEMP/cli-signing-certificate.p12"
  cleanup() {
    security delete-keychain "$KEYCHAIN_PATH" 2>/dev/null || true
    rm -f "$CERTIFICATE_PATH"
  }
  trap cleanup EXIT

  KEYCHAIN_PASSWORD=$(openssl rand -hex 12)
  echo -n "$APPLE_CERTIFICATE" | base64 --decode -o "$CERTIFICATE_PATH"
  security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
  security set-keychain-settings -lut 21600 "$KEYCHAIN_PATH"
  security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
  security import "$CERTIFICATE_PATH" -P "$APPLE_CERTIFICATE_PASSWORD" -A -t cert -f pkcs12 -k "$KEYCHAIN_PATH"
  security set-key-partition-list -S apple-tool:,apple: -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
  security list-keychain -d user -s "$KEYCHAIN_PATH"

  IDENTITY=$(security find-identity -v -p codesigning "$KEYCHAIN_PATH" | grep "Developer ID Application" | head -1 | sed 's/.*"\(.*\)".*/\1/')
  if [ -z "$IDENTITY" ]; then
    echo "ERROR: No Developer ID Application identity found in keychain"
    security find-identity -v -p codesigning "$KEYCHAIN_PATH"
    exit 1
  fi
  echo "Signing with: $IDENTITY"
  for BIN_NAME in uniclip uniclipd; do
    codesign --force --options runtime --sign "$IDENTITY" --keychain "$KEYCHAIN_PATH" "$STAGE/$BIN_NAME"
    codesign --verify --verbose "$STAGE/$BIN_NAME"
  done

  # notarytool requires a zip archive. stapler only works on bundles, so
  # Gatekeeper checks the ticket of these bare binaries online.
  NOTARIZE_ZIP="$RUNNER_TEMP/cli-notarize.zip"
  rm -f "$NOTARIZE_ZIP"
  ditto -c -k "$STAGE" "$NOTARIZE_ZIP"
  xcrun notarytool submit "$NOTARIZE_ZIP" \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" \
    --wait
fi

WORKDIR="$(pwd)"
if [ -n "$EXE" ]; then
  ARCHIVE="uniclipboard-cli-${VERSION}-${TARGET}.zip"
  # 7z stores paths as given, so add the exes from inside the staging
  # directory to keep the archive root flat (matching the tar.gz layout).
  (cd "$STAGE" && 7z a "${WORKDIR}/${ARCHIVE}" uniclip.exe uniclipd.exe)
else
  ARCHIVE="uniclipboard-cli-${VERSION}-${TARGET}.tar.gz"
  tar -czf "$ARCHIVE" -C "$STAGE" uniclip uniclipd
fi
echo "archive=$ARCHIVE" >> "$GITHUB_OUTPUT"
echo "Packaged $ARCHIVE"
