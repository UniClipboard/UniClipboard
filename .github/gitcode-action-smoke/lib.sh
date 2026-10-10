#!/usr/bin/env bash
# Helpers for gitcode-release-mirror-smoke.yml. Everything here talks to GitCode
# anonymously (the mirror repository is public) so it checks the Action's work
# independently of the Action and of any credential.
set -euo pipefail

API_BASE="${GITCODE_API_BASE:-https://api.gitcode.com/api/v5}"

# Prints the release JSON for a tag, or nothing when GitCode has no such release.
release_json() {
  local tag="$1" body status
  body=$(mktemp)
  status=$(curl -sS -m 60 -o "$body" -w '%{http_code}' \
    "$API_BASE/repos/$GITCODE_REPOSITORY/releases/tags/$tag" || echo 000)
  if [ "$status" = 200 ] && [ "$(jq -r '.tag_name // empty' "$body" 2>/dev/null)" = "$tag" ]; then
    cat "$body"
  fi
  rm -f "$body"
}

# The assets of a release in a canonical form, for before/after comparison.
assets_canonical() {
  local json
  json=$(release_json "$1")
  [ -n "$json" ] || { echo "no-release"; return 0; }
  printf '%s' "$json" | jq -S -c '.assets // []'
}

# Downloads a public address over https only, at most 3 redirects, and prints
# "<size> <sha256>".
readback() {
  local url="$1" out
  case "$url" in https://*) ;; *) echo "refusing non-https address" >&2; return 1 ;; esac
  out=$(mktemp)
  curl -sSfL --proto '=https' --proto-redir '=https' --max-redirs 3 -m 120 -o "$out" "$url"
  printf '%s %s\n' "$(wc -c < "$out" | tr -d ' ')" "$(sha256sum "$out" | cut -d' ' -f1)"
  rm -f "$out"
}

# Fails when a file contains a credential-shaped string or the literal secrets.
scan_for_leaks() {
  local file="$1"
  if grep -E -q 'access_token=|BEGIN [A-Z ]*PRIVATE KEY|OPENSSH PRIVATE' "$file"; then
    echo "credential-shaped text found in $(basename "$file")" >&2
    return 1
  fi
  if [ -n "${GITCODE_TOKEN:-}" ] && grep -F -q -- "$GITCODE_TOKEN" "$file"; then
    echo "a literal secret value found in $(basename "$file")" >&2
    return 1
  fi
}
