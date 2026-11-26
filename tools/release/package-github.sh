#!/usr/bin/env bash
# Publish a package definition from the verified Stable selection.
set -euo pipefail

operation=$1
remote=$2
title=$3
shift 3
root=$PWD
identity="$PRODUCT_VERSION-build.$PRODUCT_BUILD"
branch="uniclipboard-$identity"
default=$(gh repo view "$remote" --json defaultBranchRef --jq '.defaultBranchRef.name')
base="$default"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

python3 tools/release/package_channel.py check-current --manifest dist/channel/release-manifest.json
gh auth setup-git --hostname github.com
git -C "$work" init -q
git -C "$work" config user.name "UniClipboard Release"
git -C "$work" config user.email "release@uniclipboard.app"

if [[ "$operation" == pr ]]; then
    : "${PACKAGE_FORK:?Configure a fork for this upstream package repository}"
    owner=${PACKAGE_FORK%/*}
    origin=$PACKAGE_FORK
    # Sync the fork before pushing so the token does not need workflow scope.
    gh repo sync "$origin" --source "$remote" --branch "$base"
else
    origin=$remote
    branch=$default
fi
git -C "$work" remote add origin "https://github.com/$origin.git"
git -C "$work" remote add upstream "https://github.com/$remote.git"
if [[ "$operation" == pr ]]; then
    git -C "$work" fetch --no-tags --depth=1 upstream "$base"
else
    git -C "$work" fetch --no-tags --depth=1 origin "$base"
fi
git -C "$work" switch --detach FETCH_HEAD

for mapping in "$@"; do
    src=${mapping%%=*}
    dest=${mapping#*=}
    mkdir -p "$work/$(dirname "$dest")"
    cp "$root/$src" "$work/$dest"
    git -C "$work" add -- "$dest"
done
if [[ "$operation" == pr ]]; then
    existing=$(gh pr list --repo "$remote" --state all --head "$owner:$branch" --json url,state --jq 'map(select(.state != "CLOSED")) | first | .url // ""')
    if [[ -n "$existing" ]]; then
        echo "$existing"
        exit 0
    fi
fi
if git -C "$work" diff --cached --quiet; then
    echo "Published definitions already match $identity"
    exit 0
fi

commit=(commit -qm "$title")
if [[ -n "${PACKAGE_COAUTHOR:-}" ]]; then
    commit+=(--trailer "Co-authored-by: $PACKAGE_COAUTHOR")
fi
git -C "$work" "${commit[@]}"
if [[ "$operation" == pr ]]; then
    # A rerun can resume after pushing the branch but before opening its PR.
    if [[ -n "$(git -C "$work" ls-remote --heads origin "refs/heads/$branch")" ]]; then
        git -C "$work" fetch --no-tags --depth=1 origin "$branch"
        for mapping in "$@"; do
            dest=${mapping#*=}
            git -C "$work" show "FETCH_HEAD:$dest" | cmp - "$work/$dest"
        done
    else
        git -C "$work" push origin "HEAD:refs/heads/$branch"
    fi
    args=()
    if [[ -n "${PR_BODY_FILE:-}" ]]; then
        args+=(--body-file "$PR_BODY_FILE")
    else
        printf 'Package update for %s.\n\nRelease: %s\n' \
            "$identity" "https://github.com/UniClipboard/UniClipboard/releases/tag/desktop/v$identity" > "$work/body.md"
        args+=(--body-file "$work/body.md")
    fi
    gh pr create --repo "$remote" --head "$owner:$branch" --base "$base" \
        --title "$title" "${args[@]}"
else
    git -C "$work" push origin "HEAD:refs/heads/$branch"
    [[ "$(git -C "$work" ls-remote origin "refs/heads/$branch" | cut -f1)" == "$(git -C "$work" rev-parse HEAD)" ]]
fi
