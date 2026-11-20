# Pinned CLI rejection receipt

Formal Desktop staging is BLOCKED awaiting a newly accepted matching CLI
artifact. This is a successful guard check, not a successful Desktop build.

The checked-in lock selects CLI 2.0.0 build 2 from canonical run 43419452491,
artifact 14033443321 and source `8ab86f230351ba2039af3c23b96126e765f053df`.
Its exact manifest digest is
`ae2bd7d0cb96675d052544a4365b11319f0868bf01aea6afc01a4fac31e6c2be`.
The manifest was read from the real artifact ZIP using 79,632 bytes of HTTP
range requests. No binary package was downloaded. Its digest matched the lock.

The unmodified production `locked_cli` function rejected that real manifest for
all six targets. A production `stage` subprocess also exited 1 before creating
an output directory. The first mismatch was `internal/localconfig/config.go`.
The independent lifecycle-source comparison also confirmed that every pinned
target has the old `WatchOwner` implementation: EOF cancels the service and no
owner-release marker exists. The current protocol source hash differs.

`receipt.json` records the manifest, source, target errors, checked files and
cleanup. `SHA256SUMS` covers the receipt. The temporary downloaded manifest is
metadata only; no downloaded programs, new worktree or production lock change
was required.

To repeat after obtaining that exact manifest through the accepted acquisition
path, verify its SHA-256, then use a new output path:

```sh
python3 tools/package/desktop.py stage --target darwin-arm64 \
  --cli-manifest /path/to/release-manifest.json \
  --output /tmp/unic-old-lock-rejection-new-run
```

Expect the source-contract mismatch before program extraction. No compatibility
branch or gate bypass is needed. Select a real complete CLI candidate with
matching shared source before formal staging. Fresh same-source device-test
packages remain a distinct, nonrelease construction path.
