#!/usr/bin/env bash

set -uo pipefail

usage() {
  echo "Usage: $0 <probe-file> <scan-root> [scan-root ...]" >&2
}

if [[ $# -lt 2 ]]; then
  usage
  exit 2
fi

if ! command -v file >/dev/null 2>&1; then
  echo "Required command is unavailable: file" >&2
  exit 2
fi

probe_file=$1
shift

if [[ ! -f "$probe_file" ]]; then
  echo "Probe file does not exist: $probe_file" >&2
  exit 2
fi

probe=$(<"$probe_file")
if [[ -z "$probe" || "$probe" == *$'\n'* ]]; then
  echo "Probe file must contain one non-empty line" >&2
  exit 2
fi

redact_path() {
  local value=$1
  printf '%s' "${value//"$probe"/[REDACTED]}"
}

types_file=$(mktemp "${TMPDIR:-/tmp}/uc-plaintext-types.XXXXXX") || exit 2
trap 'rm -f "$types_file"' EXIT

found=0
scanned_files=0

for root in "$@"; do
  if [[ ! -d "$root" ]]; then
    echo "Scan root does not exist: $(redact_path "$root")" >&2
    exit 2
  fi

  echo "Scanned root: $(redact_path "$root")"
  while IFS= read -r -d '' candidate; do
    scanned_files=$((scanned_files + 1))
    mime_type=$(file -b --mime-type "$candidate" 2>/dev/null || echo "unknown")
    printf '%s\n' "$mime_type" >>"$types_file"

    if grep -a -F -q -- "$probe" "$candidate"; then
      echo "Plaintext probe found in: $(redact_path "$candidate")" >&2
      found=1
    else
      grep_status=$?
      if [[ $grep_status -gt 1 ]]; then
        echo "Failed to scan file: $(redact_path "$candidate")" >&2
        exit 2
      fi
    fi
  done < <(find "$root" -type f -print0)
done

echo "Scanned files: $scanned_files"
echo "File types:"
if [[ -s "$types_file" ]]; then
  sort -u "$types_file" | sed 's/^/  /'
else
  echo "  (none)"
fi

if [[ $found -ne 0 ]]; then
  exit 1
fi

echo "Plaintext probe scan passed"
