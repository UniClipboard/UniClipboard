#!/usr/bin/env bash
set -euo pipefail
: "${PACMAN_GPG_PRIVATE_KEY:?PACMAN_GPG_PRIVATE_KEY is required}"
: "${PACMAN_SIGNING_FINGERPRINT:?PACMAN_SIGNING_FINGERPRINT is required}"
pacman -Sy --noconfirm python github-cli gnupg
export GNUPGHOME=/run/keys
printf '%s\n' "$PACMAN_GPG_PRIVATE_KEY" | gpg --batch --import
unset PACMAN_GPG_PRIVATE_KEY
python tools/package/pacman.py --input dist/channel/packages \
    --output dist/channel/signed --key "$PACMAN_SIGNING_FINGERPRINT"
cd dist/channel/signed
for kind in db files; do
    # Use regular files for GitHub uploads. repo-add uses symlinks for aliases.
    rm -f "uniclipboard.$kind" "uniclipboard.$kind.sig"
    cp "uniclipboard.$kind.tar.gz" "uniclipboard.$kind"
    cp "uniclipboard.$kind.tar.gz.sig" "uniclipboard.$kind.sig"
done
cd ../../..
python tools/release/package_pacman.py --snapshot dist/channel/signed \
    --manifest dist/channel/release-manifest.json --fingerprint "$PACMAN_SIGNING_FINGERPRINT" \
    --receipt dist/channel/pacman-publication.json
