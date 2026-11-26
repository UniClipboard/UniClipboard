#!/usr/bin/env bash
set -euo pipefail
: "${AUR_SSH_PRIVATE_KEY:?AUR_SSH_PRIVATE_KEY is required}"
: "${AUR_HOST_FINGERPRINTS:?AUR_HOST_FINGERPRINTS is required}"
python tools/release/package_channel.py check-current --manifest dist/channel/release-manifest.json
root=$PWD
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
umask 077
printf '%s\n' "$AUR_SSH_PRIVATE_KEY" > "$work/aur"
unset AUR_SSH_PRIVATE_KEY
ssh-keyscan -t ed25519 aur.archlinux.org > "$work/known_hosts" 2>/dev/null
fingerprint=$(ssh-keygen -lf "$work/known_hosts" | awk '{print $2}')
tr ',' '\n' <<< "$AUR_HOST_FINGERPRINTS" | tr -d ' ' | grep -qxF "$fingerprint"
export GIT_SSH_COMMAND="ssh -i $work/aur -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=$work/known_hosts"
git -c init.defaultBranch=master clone ssh://aur@aur.archlinux.org/uniclipboard-git.git "$work/pkg"
cd "$work/pkg"
cp "$root/dist/aur-build/PKGBUILD" "$root/dist/aur-build/.SRCINFO" .
git add PKGBUILD .SRCINFO
if git diff --cached --quiet; then
    exit 0
fi
git -c user.name="UniClipboard Release" -c user.email=release@uniclipboard.app commit -m "Update to $RELEASE_IDENTITY"
git push origin HEAD:master
[[ "$(git ls-remote origin refs/heads/master | cut -f1)" == "$(git rev-parse HEAD)" ]]
