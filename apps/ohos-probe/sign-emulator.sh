#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
deveco_contents="${DEVECO_STUDIO_CONTENTS:-/Applications/DevEco-Studio.app/Contents}"
toolchains_lib="$deveco_contents/sdk/default/openharmony/toolchains/lib"
java="$deveco_contents/jbr/Contents/Home/bin/java"
keytool="$deveco_contents/jbr/Contents/Home/bin/keytool"
sign_tool="$toolchains_lib/hap-sign-tool.jar"
keystore="$toolchains_lib/OpenHarmony.p12"
profile_template="$toolchains_lib/UnsgnedReleasedProfileTemplate.json"
profile_certificate="$toolchains_lib/OpenHarmonyProfileRelease.pem"
unsigned_hap="${1:-$project_root/entry/build/default/outputs/default/entry-default-unsigned.hap}"
signed_hap="${2:-$project_root/entry/build/default/outputs/default/entry-default-signed.hap}"
signing_dir="$project_root/build/signing"
password="${OHOS_TEST_KEYSTORE_PASSWORD:-123456}"

for file in "$java" "$keytool" "$sign_tool" "$keystore" "$profile_template" "$profile_certificate" "$unsigned_hap"; do
  if [[ ! -e "$file" ]]; then
    echo "required signing input is unavailable: $file" >&2
    exit 1
  fi
done
if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required to prepare the emulator signing profile" >&2
  exit 1
fi

mkdir -p "$signing_dir"
not_before="$(date +%s)"
not_after="$((not_before + 31536000))"
unsigned_profile="$signing_dir/profile.json"
signed_profile="$signing_dir/profile.p7b"
app_certificate="$signing_dir/application.cer"

jq \
  --arg bundle_name "app.uniclipboard.engineprobe" \
  --argjson not_before "$not_before" \
  --argjson not_after "$not_after" \
  '.validity["not-before"] = $not_before
   | .validity["not-after"] = $not_after
   | .["bundle-info"]["bundle-name"] = $bundle_name' \
  "$profile_template" > "$unsigned_profile"

: > "$app_certificate"
for alias in \
  "openharmony application root ca" \
  "openharmony application ca"; do
  "$keytool" \
    -exportcert \
    -rfc \
    -alias "$alias" \
    -keystore "$keystore" \
    -storetype PKCS12 \
    -storepass "$password" >> "$app_certificate"
done
jq -r '.["bundle-info"]["distribution-certificate"]' \
  "$profile_template" >> "$app_certificate"

"$java" -jar "$sign_tool" sign-profile \
  -mode localSign \
  -keyAlias "openharmony application profile release" \
  -keyPwd "$password" \
  -profileCertFile "$profile_certificate" \
  -inFile "$unsigned_profile" \
  -signAlg SHA256withECDSA \
  -keystoreFile "$keystore" \
  -keystorePwd "$password" \
  -outFile "$signed_profile"

"$java" -jar "$sign_tool" sign-app \
  -mode localSign \
  -keyAlias "openharmony application release" \
  -keyPwd "$password" \
  -appCertFile "$app_certificate" \
  -profileFile "$signed_profile" \
  -profileSigned 1 \
  -inFile "$unsigned_hap" \
  -signAlg SHA256withECDSA \
  -keystoreFile "$keystore" \
  -keystorePwd "$password" \
  -outFile "$signed_hap" \
  -compatibleVersion 24 \
  -signCode 1

"$java" -jar "$sign_tool" verify-app \
  -inFile "$signed_hap" \
  -outCertChain "$signing_dir/verified-cert-chain.cer" \
  -outProfile "$signing_dir/verified-profile.p7b"
