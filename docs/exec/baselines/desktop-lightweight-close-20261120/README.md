# Updated lightweight-close acceptance receipt

Product source: `ce4a0a8cf07fc15b404f20352d08406286261c04`.
Branch: `fix/desktop-lightweight-processes`. No push or publication was performed.

`summary.json` retains native ARM64 process identities, kernel creation times,
service generations, selected native-input/service events, cleanup and limits.
`artifact-hashes.json` binds the original local Full06 files. Images remain local
under `.build/windows-native-b973-v2/outer/guest-artifacts/full-06/`; programs,
private profiles and staged source are not part of this receipt. The earlier
`desktop-processes-20261120` receipt remains historical evidence for its original
source and semantics. This receipt supersedes its top-level Windows NOT_RUN.

Full06 passed all five native scenarios and six actual tray menu business clicks.
Four panel captures waited for the semantic search field, idle frame and Windows
compositor. Their native foreground crops were inspected. Full05 remains FAIL
because a later native menu capture lost foreground; its first panel rendered.
Full04 establishes lifecycle/focus only, because its panel crops preceded paint.
Actual system clipboard and paste insertion, login execution/permission prompts,
x64 native execution and installer replacement remain NOT_RUN.

## Replay

1. Verify this receipt with `shasum -a 256 -c SHA256SUMS` in this directory.
2. Use a checkout containing the recorded product commit. Run the canonical
   process cases with `make desktop-e2e CASE=desktop-lightweight-close` and
   `make desktop-e2e CASE=desktop-processes`. These fixtures do not prove native
   tray, focus or paste behavior.
3. Build a matching private ARM64 trio from the recorded clean product source:
   `GOOS=windows GOARCH=arm64 go build` for `./app/service/cmd/unic`, `./app/service/cmd/unicd`, and
   `./app/desktop/cmd/unic-desktop`. Desktop needs `-tags desktop_e2e` and
   `-ldflags=-H=windowsgui`. Record each executable's SHA-256 and `go version -m`.
4. Use the native driver from this receipt's containing commit. Build its adjacent
   `helper.go` for Windows ARM64. Keep product source and test-driver identities
   separate. Do not set `UNIC_DESKTOP_E2E_PROCESS_FIXTURE`.
5. Obtain exclusive handover of the identified Windows VM. Do not alter clipboard
   sharing, install software, grant permissions or use the personal profile.
   Parse and inspect the driver first. Follow its
   [instructions](../../../../app/desktop/e2e/testdata/windows-native/README.md).
6. Run the lightweight phase with fresh private paths. Then run the full phase
   with another fresh task root and artifact directory. Use the mandatory exact
   `-SourceSHA` and a hash-bound source receipt. Never reuse business input after
   an unknown outcome. Preserve the failed receipt before any fresh run.
7. Verify every generated SHA256SUMS entry. Inspect the native panel crops,
   process creation times, same-generation handoff and foreground target.
8. Remove only task-owned stages after cleanup PASS. Restore the VM's handed-over
   state. Full06 ended with zero task processes/profiles and a suspended VM.

Formal Desktop package staging remains BLOCKED by the accepted older CLI lock.
The package guard rejects its stale service protocol. A matching accepted CLI
artifact is required before packaging; this task did not invent a lock or publish.
