#!/bin/sh
set -eu

cd "$(dirname "$0")/../../.."
output_dir="${1:-dist/migration}"
mkdir -p "$output_dir"
for target_os in darwin linux windows; do
    for target_arch in arm64 amd64; do
        suffix=""
        if [ "$target_os" = windows ]; then suffix=".exe"; fi
        CGO_ENABLED=0 GOOS="$target_os" GOARCH="$target_arch" \
            go build -trimpath -buildvcs=false \
            -o "$output_dir/unic-migrate-$target_os-$target_arch$suffix" \
            ./app/service/cmd/unic-migrate
    done
done
python3 - "$output_dir" <<'PY'
import hashlib
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
files = sorted(path for path in root.glob("unic-migrate-*") if path.is_file())
with (root / "SHA256SUMS").open("w") as output:
    for path in files:
        digest = hashlib.sha256()
        with path.open("rb") as binary:
            for chunk in iter(lambda: binary.read(1024 * 1024), b""):
                digest.update(chunk)
        output.write(f"{digest.hexdigest()}  {path.name}\n")
print(f"Built {len(files)} migration binaries in {root}")
PY
