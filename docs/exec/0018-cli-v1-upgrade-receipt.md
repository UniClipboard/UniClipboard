# CLI v1 upgrade notice and migration delivery

## Requested behavior

The selected CLI release is `2.0.0 (1)`. Show a v1 incompatibility warning only after detecting an existing known v1 data directory. A fresh installation must not receive the warning. Keep the current task focused on CLI publication.

## Implementation

- Share the existing platform-specific default v1 path resolver between the notice and importer. Resolving a path does not create it.
- Check the default v1 directory and the active `UC_PROFILE` directory with stat calls. Do not inspect credentials or database contents.
- Show the notice on interactive stderr only. Suppress it for `--json`, missing directories and regular files.
- Provide backup, preflight and import instructions. Do not require acknowledgement or start migration automatically.
- Make `unic migrate` and `unic migrate --help` print an offline guide before accessing a helper, profile or credential.
- Ship the existing `unic-migrate` helper in CLI archives and npm platform packages. Verify its native executable identity.
- Preserve the helper's partial-result exit code 3. Keep the importer's source preservation, resumed record identity and reopened-target verification.
- Run the packaged release/upgrade E2E on the Linux amd64 CLI build target and upload its report/checksum.

Custom or portable v1 directories outside the known paths require `--source`. Named profile imports require the matching source and credential service. Migration imports supported local history and tags; it does not import settings, identities, trust or favorites. Users must upgrade peers to v2 and pair again.

## Verification

The release process E2E passed 40 checks. It runs actual Go programs, npm pack/install and the shipped migration helper. A temporary home and pseudoterminals establish directory detection and clean JSON behavior. The migration source uses the existing fixed encrypted Rust fixture. No personal profile or system credential is accessed.

Migration acceptance covers offline help, preflight without a target, partial import with exit 3, source hash preservation, repaired-record resume and duplicate-free retries. The final replay used clean commit `4f4f846449aa58fe239a91d0413b0ac6220db660`. Its 41 source hashes match the worktree. The final replay and source hashes are recorded in the [small check receipt](baselines/0018-cli-upgrade-checks.json).

Production headless E2E passed in 47.31 seconds. Full Go race tests passed with `TMPDIR=/private/tmp`. The initial broad run had one subprocess receive timeout; the focused replay and subsequent full run passed. The canonical temp directory avoids the previously observed macOS symlinked-temp cache failures.

Changed-package lint, formatting, Ruff and actionlint passed. Full repository lint still has 39 previously documented unrelated findings; the output is identical to the independent original-source result. No unit tests were added or changed.

Replay:

```sh
python3 e2e/release/run.py
TMPDIR=/private/tmp make headless-e2e
TMPDIR=/private/tmp go test -race ./...
cd .cache/release-e2e
shasum -a 256 -c SHA256SUMS
```

No production publication, workflow dispatch, GUI, native Windows/Linux machine or real credential-store acceptance ran. The new CI step is implemented but has not run remotely. Existing npm channel transition and workflow authorization remain publication prerequisites.
