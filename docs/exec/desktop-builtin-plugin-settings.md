# Built-in plugin settings

## Scope and behavior

Settings > Plugins manages the three shipped plugins through the existing revision-checked configuration API. Tailcat exposes text, image, file/directory policy and the automatic file-byte threshold. These policies apply immediately. The global Sync page retains global permissions and automatic-directory intent. Tailcat cannot be independently disabled or removed.

Text tools and content classification expose enablement and processor error policy. Policies inherit the global default unless explicitly overridden. Both settings apply on the next Desktop startup. The UI must distinguish saved settings from running settings and direct the user to quit and reopen Desktop. None of these plugins has plugin-specific Schema fields. External installation and arbitrary Schema forms remain out of scope.

## Failure inventory written before implementation

- The Plugins navigation entry is missing, or opens fixture controls in production.
- Editing a shallow-copied instances map mutates authoritative state before save.
- A stale revision overwrites concurrent configuration, or a save error appears successful.
- Controls become enabled before the saved snapshot reaches the UI, so a second toggle reuses the old value or revision.
- Saving one plugin discards other plugin settings or unrelated configuration.
- Omitted policy booleans appear off instead of their default on state.
- Tailcat exposes unsupported enablement or processor policy.
- Files and directories receive different backend permissions.
- Automatic threshold applies per member instead of to the selection total.
- Manual send incorrectly obeys the automatic-only threshold.
- An invalid, fractional or out-of-range threshold is saved or silently rounded.
- Processor changes appear active before restart, or fail to apply after restart.
- Classification enablement fails to report restart required.
- Reverting a saved processor edit leaves a false restart notice.
- An inherited failure policy is flattened into an explicit override.
- Saved-but-unapplied service errors disappear on reload.
- Reopening Desktop loses settings, incorrectly resets the test profile or leaves an old service running.

## Acceptance

Native cases `builtin-plugin-policy` and `builtin-plugin-processors` drive Settings > Plugins in the production Desktop with the runner-owned profile, directory clipboard and real service. IPC assertions verify persisted configuration and observable Engine policy decisions. Threshold acceptance pairs a second production service using exact fingerprint/revision approvals. Oversized automatic copies must produce new local history while leaving the receiver unchanged for 1.5 seconds; a subsequently delivered text barrier verifies the live connection. Explicit sends and an immediate threshold reset must deliver all file payloads with matching receiver-side SHA-256 values. Directory fixtures contain two 600 KiB files, each below the 1 MiB threshold but above it in total. Processor cases verify history tags and available Actions before and after Desktop restart. No personal profile or system clipboard is used.

Evidence includes source/binary identities, JSON assertions, screenshots and continuously sampled window recordings. Processor error settings are verified for persistence and restart semantics; built-in processor failures are not artificially injected. OS clipboard integration remains outside this change.

## Execution receipt

- Host: macOS arm64, Go 1.27.3, race detector enabled.
- `builtin-plugin-processors`: passed in `.build/desktop-e2e/20261114T162933Z-51e86fce/`. Saved/reverted settings, old runtime before restart, disabled/enabled runtime after two real restarts, inherited policy reset, unchanged telemetry/global sync, screenshots and recording passed.
- `builtin-plugin-policy`: passed in `.build/desktop-e2e/20261114T164520Z-b3bcfae3/`. All content switches, rejected negative threshold, persistence/reopen, real paired-service threshold checks and receiver payload SHA-256 assertions passed. The case recorded 52 assertions and 29 frames over 36.28 seconds.
- `TMPDIR=/private/tmp go test -race ./...`: 782 tests passed across 75 packages after the final production changes.
- Repository-pinned golangci-lint v2.15.0: 0 issues in Desktop renderer, Desktop E2E and local configuration packages.
- Full `make desktop-e2e` catalogue: running in `.build/desktop-e2e/20261114T164644Z-3cce45f8/`; final result will be recorded below.
- The host's default temporary-directory alias triggers the existing cache-root symlink guard; the canonical temporary directory resolves that environment issue. No cache-root validation was weakened.
- Recording pauses across actual Desktop quit/reopen gaps. The test-only capture lock protects the window identity before capture. The profile and service remain real, isolated processes.
- Native OS clipboard delivery, OS focus restoration and cross-application paste: NOT_RUN.

Replay:

```sh
make desktop-e2e CASE=builtin-plugin-policy
make desktop-e2e CASE=builtin-plugin-processors
TMPDIR=/private/tmp go test -race ./...
make desktop-e2e
```

The final report, `manifest.json`, `SHA256SUMS`, per-case JSON assertions, screenshots and recordings are the reproducible evidence. The runner verifies its frozen source and removes private source, binaries, profiles and logs after process cleanup.
