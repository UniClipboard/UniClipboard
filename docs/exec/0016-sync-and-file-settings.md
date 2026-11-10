# Execution plan: Sync and file settings

> Scope update: [0017](0017-history-auto-cleanup.md) removes the separate file-retention and cleanup settings and replaces payload-only eviction with unified history/file deletion. The per-device cache quota remains. Earlier implementation and acceptance notes below describe the previous behavior.

- Status: CLI and TOML implemented; final acceptance in progress. Settings GUI deferred.
- Date: 2026-11-09
- Requirements: [User stories and decision draft](../decisions/proposed/0016-sync-and-file-settings.md).
- Source: `github.com/uniclipboard/unic`, branch `v2`.
- Inspected HEAD: `2d4e856b9044bd4a9eb4d4d24a4c1de5bf86521a`.
- Scope: Nine settings; no sync frequency or immediate-transfer threshold; default maximum file size 100 MB.
- Confirmed boundary: File quota, retention, and cleanup cover only the default managed file directory. Custom receive directories grant no deletion authority.

## 1. Evidence and limits

The following findings come from source inspection. They are not runtime acceptance results.

| Area | Current implementation | Gap and implementation consequence |
|---|---|---|
| Global sync | `engine/types.go: SyncSettings`; `engine/state.go: UpdateSettings`; `engine/backend.go: dispatchProcessed` | The switch exists. Add GUI editing and prove that pause preserves local history. |
| Automatic sends | `engine/clipboard.go: ObserveClipboard`; `engine/submission.go`; `engine/backend.go` | No independent automatic-send switch. Suppress dispatch after local commit, without suppressing capture or explicit sends. |
| History restore | `engine/history.go: BeginApplyHistory`; `engine/clipboard.go: ReportApplyResult`; `app/desktop/internal/host/clipboard.go: UseHistory` | Restore deliberately does not send. Add explicit restore intent and a send path that creates no new history. |
| File policy | `engine/types.go: BackendSyncPolicy`; `engine/send.go: syncAllowsPart`; `engine/receive.go: allowsAny` | Each backend has its own file policy. Add one global restriction and combine it with backend policy. |
| File size | `engine/options.go: DefaultSettings`; `engine/content.go: ImportContent`; `engine/peerrecv.go: checkClipLimits` | Default is 2 GiB. The same configured limit also restricts import. Separate the transfer limit from local-history import. |
| Backend threshold | `engine/backend.go: fileTooLarge`; `app/desktop/internal/config/config.go: BackendSync` | The existing threshold skips an automatic commit. It does not deliver metadata. Keep it outside this feature. |
| Storage location | `config.Config.Inbox`; `host.Open`; `engine.Options.InboxDir` | A directory change already requires restart. History file blobs still live under `DataDir/history`. Changing inbox alone does not relocate persistent file content. |
| Temporary cleanup | `engine/content.go: cleanContentDirs`; `engine/receive.go: recvDir`; `app/desktop/cmd/unicd/daemon.go` | Engine removes temporary receive data at startup. The daemon also deletes legacy `clip-*` directories before Engine takes its lock. Neither operation implements retention. |
| Persistent files | `engine/history.go: persistHistory`, `loadHistoryContent`, `blobTracker`; `internal/store/history.go` | Engine copies received files into history blobs. Count history payloads only when they belong to the default file cache. Custom receives need external references, not owned history copies. |
| History expiry | `engine/history.go: sweep`; [history contract](../contracts/history-contract.md) | Expiry removes entire records. Add separate payload availability and retention; do not map S8 to history TTL. |
| Resource budget | `engine/options.go: ResourcePolicy`; `engine/history.go: reserveStored` | Shared budget defaults to 8 GiB; history budget defaults to 4 GiB. Neither is a file-only 500 MB quota. Preserve both existing budgets. |
| Settings persistence | `config.Manager.Reload`, `Watch`, `State`; `api.Handler` | Only `GET /v1/config` exists. Add one daemon-owned write path, conflict detection, and saved/applied state. |
| Settings interface | `app/desktop/cmd/unic/gui.go`; `internal/uikit/settings.go` | No settings tab. Existing SettingsContent, SettingsGroup, and SettingsRow can build the page. |
| E2E harness | `e2e/cli/e2e_test.go`; [E2E guide](../../e2e/cli/README.md) | Real processes and directory clipboards already exist. The harness removes work directories and temporary binaries, so retained evidence needs explicit support. |

### Dependency check

The project already has TOML, fsnotify, SQLite, and a UI Kit. These features need no new package.
The pinned mygo revision provides `Dialog.Open(OpenDialogOptions{Directory: true})`.
The method returns no paths on cancellation. The local module cache did not contain this revision. Source inspection confirmed this API in the pinned upstream revision.
The agent did not launch a folder picker.

### Mature-product research

Sources were read on 2026-11-09. These observations inform the proposal; they do not prove unic behavior.

| Source | Observed pattern | Application to this plan |
|---|---|---|
| [KDE Connect clipboard source](https://raw.githubusercontent.com/KDE/kdeconnect-kde/master/plugins/clipboard/clipboardplugin.cpp) | The automatic-change callback checks `autoShare`. The manual send method is separate. The file send path checks a configurable size. | Separate automatic sending from explicit sending. Keep a size check at admission. Do not copy KDE's reconnect replay behavior. |
| [KDE Connect share source](https://raw.githubusercontent.com/KDE/kdeconnect-kde/master/plugins/share/shareplugin.cpp) | Destination selection is separate from transfer. The receive path checks each destination. Existing names are not silently overwritten. | Reuse a native picker, isolate managed paths, and avoid name collisions. Ordinary user downloads are not a cache design. |
| [Syncthing file versioning](https://docs.syncthing.net/users/versioning.html) | Archive policy is local to each device. Archive age and custom version location are explicit settings. | Keep quota and retention local. Store completion times. Separate storage ownership from the selected parent folder. |
| [Syncthing configuration](https://docs.syncthing.net/users/config.html) | Configuration distinguishes storage locations, cleanup parameters, and free-disk requirements. | Do not equate logical quota with available disk space. A disk-full failure still needs rollback. |
| [Pinned mygo dialog source](https://raw.githubusercontent.com/egoist/mygo/9233663736ca/dialog.go) | The existing dependency has a native directory picker and explicit cancellation. | Add no folder-picker dependency or platform shell script. |

KDE's website returned HTTP 403; primary source code supplied the evidence instead.
Two guessed upstream filenames returned HTTP 404; the existing clipboard, share, and dialog files above supplied the evidence.

## 2. Proposed architecture

Engine owns policy, receive admission, default-cache payloads, leases, and default-cache eviction.
Custom received files belong to the user. Engine stores opaque references and never deletes these files through lifecycle operations. The host owns custom destination selection and file writing.
The desktop host owns TOML, local API transport, the folder picker, and system clipboard calls.
The GUI reads and saves settings through the daemon. It never edits TOML directly.

### Proposed configuration mapping

Names in this table are implementation proposals, not existing API fields.
Do not add duplicate configuration for the existing global switch, inbox, or file-size setting.

| Story | TOML field | Typed destination | Application boundary |
|---|---|---|---|
| S1 | `sync.enabled` | Existing `Settings.Sync.Enabled` | Live; cancel content work when disabled |
| S2 | `sync.automatic` | `Settings.Sync.Automatic` | Live; future observed submissions |
| S3 | `sync.on_restore` | `Settings.Sync.OnRestore` | Snapshot at restore admission; recheck global permission before sending |
| S4 | `sync.files` | `Settings.Sync.Files` | Live; effective files = global permission AND backend permission |
| S5 | `inbox` | `StoreReceivedFile` implemented by the desktop host; `CacheDir` selects private storage | Restart; expose configured and active location and cleanup scope |
| S6 | `limits.max_file_bytes` | Existing `Settings.Limits.MaxFileBytes` | New transfers; default 100 * 1024 * 1024 bytes |
| S7 | `file_cache.max_bytes` | `Settings.FileCache.MaxBytes` | Live; default-cache files only; no custom-directory quota |
| S8 | `file_cache.retention_hours` | Typed Engine duration or validated integer hours | Default-cache files only; evaluate completion time at cleanup |
| S9 | `file_cache.auto_cleanup` | `Settings.FileCache.AutoCleanup` | Default managed directory only; never traverse a custom receive directory |

Keep protocol version 1. The 100 MB default requires no wire-size increase.
Keep `limits.max_total_bytes` as a separate internal and existing batch bound.
Do not add backward-compatibility readers, fallback fields, or migrations for an unreleased schema.
Do not change the migration tool's offline ownership semantics without an explicit contract update.
Imported historical file payloads remain archive data under the history budget; the new receive-cache policy must not silently expire imported archives.

### Settings writes

1. Add a revision to configuration state and the raw file identity used for the read.
2. Add `PUT /v1/config` with a complete typed value and an expected revision.
3. Serialize daemon writes through the configuration manager.
4. Validate all fields before replacement, including directory rules and integer overflow.
5. Reject stale revisions or a changed disk file with `conflict`.
6. Encode with the existing TOML library and replace the file from the same directory.
7. Keep the file private and preserve unrelated supported settings in the submitted value.
8. Apply live Engine settings and expose any pending restart or apply failure separately.
9. Avoid a second revision increment when fsnotify sees the daemon's own save.
10. Add a CLI writer that uses the same endpoint for repeatable black-box acceptance.

The file and Engine state do not share a transaction.
Define the API response as saved configuration plus effective application status.
A save must not report that a failed Engine update took effect.
An ordinary external editor does not honor the daemon's mutex.
File-identity comparison detects observed conflicts but cannot promise an atomic compare-and-swap against every external rename.
Document that limit and surface the next external save through the existing watcher.

### Restore sends

Use a restore-specific request identity and operation result.
Keep generic `BeginApplyHistory` local unless the caller marks an actual history restore.
Applying an Action result must not send it a second time through S3.

Extend the successful apply path to schedule a history resend exactly once.
Dispatch outside Engine locks. Acquire the content reference before releasing the apply reference.
Reuse backend dispatch, trust checks, size checks, and operation tracking.
Do not call `CommitContent` or `SubmitContent` to resend: those calls create another history item and rerun processors.
Return local apply status separately from network status.
Bound the request cache and state the idempotency window. Do not promise exactly-once delivery across a crash or beyond that window.

The current history schema does not preserve a processor's original sync prohibition as a resend permission.
Before implementation, decide whether an explicit restore can override that prohibition.
The recommended rule is to preserve a stored prohibition and report a blocked resend.
This needs a persisted policy field; a sent/not_requested summary is not a permission flag.

### File ownership and cleanup boundary

Implement two explicit storage classes. Do not infer deletion authority from a history reference or a filename prefix.

| Storage class | Accounting | Lifetime and deletion authority |
|---|---|---|
| Default cache | S7 counts owned file bytes and reservations once. Existing global resource limits also apply. | S8 and S9 apply. Delete only proven owned files inside the default managed file directory. |
| Custom receive | S7 does not count final files or reject receives because the default cache is full. Transfer limits and disk errors still apply. | The user owns final files. Quota, retention, history deletion, history expiry, lease release, restart, and shutdown must not delete them. |

Use the default path resolver and a persisted storage-class field to identify default-cache payloads.
A custom path string must not grant deletion authority, even if the custom path aliases the default path.
Reject ambiguous custom destinations that overlap or alias the managed cache, its parents, or its internal storage.
Never follow a symlink or a replaced directory outside the verified default-cache boundary during cleanup.
Use proven per-file ownership and a directory boundary check together; neither check replaces the other.
Do not scan a custom receive directory for expired, orphaned, or familiar-prefix files.
Do not keep custom receive roots in any automatic deletion registry.

### Default-cache payloads

Add the smallest Engine-owned payload component that covers default-cache receive and history references.
Reuse SQLite and the existing blob-holder pattern. Do not add a generic storage-provider framework.

- Store payload identity, default-cache storage class, received completion time, byte size, and availability in SQLite.
- Let Received, history, reads, sends, and clipboard leases share one default-cache payload identity.
- Count default-cache file payload bytes once. Keep pending reservations and failed-deletion bytes in the charge.
- Stage and promote default-cache payloads inside the verified default directory.
- Publish a complete batch only after file finalization, database commit, and the existing receive barrier.
- Keep text/image storage, imported archives, and local source references outside this file-cleanup scope.
- Add availability to history DTOs without exposing paths or filenames in ordinary list responses.
- When default-cache eviction removes a file, retain history metadata and return a stable unavailable-content result on use.
- Remove the daemon's unowned prefix-based legacy cleanup. Constrain startup reconciliation to proven owned files inside the default managed directory.
- Protect default-cache paths still present on the system clipboard at startup before age cleanup runs.
- If clipboard inspection fails, preserve previously recorded clipboard-held payloads until the host can reconcile them.
- Make deletion restartable: mark the payload unavailable, unlink it within the verified boundary, then clear the charge after successful deletion.
- Keep a failed unlink charged and visible. Do not revive logically evicted content after restart.

A retention deadline makes a default-cache payload eligible for startup cleanup; the deadline does not invalidate a still-present file by itself.
Quota eviction can remove an unexpired default-cache payload. Retention is not a minimum storage guarantee.

### Custom received files

Have the desktop host finalize custom receives and return persistent file references for history.
Reuse source version checks to detect a moved, deleted, or modified custom file.
Do not retain an owned history copy that silently brings the custom file under the default-cache quota.
History deletion and history expiry may discard references; they must not unlink the custom file.
Release of the last content lease must not unlink the custom file.
Use exclusive file creation and avoid overwriting an existing destination.
Keep incomplete transfer staging in verified application-managed temporary storage with separate bounded transfer resources.
Do not apply the default-cache quota to a custom receive merely because transfer staging uses managed storage.
Cross-filesystem finalization needs a copy and explicit completion checks; do not assume a rename can cross filesystems.
If finalization leaves a partial file in the custom directory, report the failure and leave removal to the user.
Do not add a custom-directory cleanup exception for incomplete files.

Changing the active location affects future receives only.
Earlier default-cache files remain eligible under their original default-cache rules.
Earlier custom received files remain user-managed after switching back to the default, after restart, and after history deletion.
The settings page must show that S7-S9 do not apply to the custom directory.
Keep saved S7-S9 values so that returning to the default restores the configured cache policy.

## 3. Failure inventory before implementation

Use these failures to write E2E scenarios before each code slice.
This plan adds no unit tests. Do not write unit tests after feature code.

| Failure family | Required scenarios |
|---|---|
| Settings persistence | Invalid TOML, unknown key, overflow, stale revision, concurrent GUI saves, external edit, write permission failure, failed runtime application, restart-required state, self-watch notification |
| Policy precedence | Global pause with manual/restore send; automatic off with receive/manual send; file global off with backend files on; backend files off with global files on |
| Observation | Capture while automatic sync is off; enabling does not replay; restore echo; startup baseline; new local copy during apply |
| Restore | Failed/deferred/conflicting apply; duplicate request; no target; source changed; global switch changes during apply; Action application; restart before send |
| Size limits | Below/exactly/above 100 MB; a multi-file batch with one oversized file; receive-side mismatch; declared length mismatch; in-flight policy update |
| Quota | Default-cache concurrent receives and shared payloads; active pins; small/lowered quota; custom receive while default cache is full; disk full; reservation rollback |
| Cache durability | SIGKILL before/after file promotion and database commit; restart retains valid payloads; history references remain valid; failed deletion stays charged |
| Retention | Default-cache deadline boundaries; clock movement; changed retention; cleanup off; active pins; retained metadata; custom received files and imported archives unaffected |
| Storage root | Unwritable/missing directory; picker cancellation; default -> custom -> default; custom aliases default; symlink/directory replacement; custom files and familiar-prefix files survive every cleanup path |
| User interface | Save failure, lost daemon, conflicting save, dependent disabled controls, keyboard focus, narrow window, three skins and two themes |

## 4. Implementation phases and exit gates

Each phase keeps prior behavior working. Do not replace the receive pipeline with an unfinished cache rewrite.
Write each phase's E2E scenario first. Add the retained artifact path before the first feature acceptance run.

| Phase | Work | Exit gate |
|---|---|---|
| P0: Review and contracts | Resolve proposal section 4 and restore permissions. Specify config writes, restore results, payload availability, ownership, and quota accounting in contracts. | Requirements and failure matrix have no unresolved behavior that blocks P1. No feature implementation yet. |
| P1: First complete path | Implement config write API and CLI writer. Add S2 to Engine. Capture locally with automatic sending disabled. Preserve manual sending and receiving. | Real paired processes prove save -> local capture -> no automatic send -> manual send -> receive -> restart persistence. Retain a reproducible artifact. |
| P2: Remaining sync policy | Expose S1, add S4, and set the S6 default. Separate import hard limits from transfer limits. Apply the global file restriction across in-process and external backends. | Prove policy precedence, boundary sizes, whole-batch rejection, local oversized-file history, and no replay. |
| P3: Restore | Add restore identity, successful-apply resend, structured local/network results, original policy rule, and Action distinction. | Prove default local-only, one successful resend, no duplicate history, no send after failure/conflict, retry deduplication, no Action double send. |
| P4: Persistent payload and location | Separate default-cache payloads from custom received references. Connect S5 and constrain startup reconciliation to the default directory. | Real file receive -> history -> local apply -> TERM/KILL -> restart -> read proves stable bytes and one retained payload. Custom received files and unrelated files survive history deletion, lease release, and restart. |
| P5: Quota and retention | Add S7-S9 for the default directory only, reservations, eviction, startup retention, byte accounting, and deletion recovery. | Default-cache matrices pass. Custom receives succeed with a full default cache, and custom files survive every automatic and history cleanup path. |
| P6: Settings page | Build two settings groups with existing UI Kit components. Add native folder selection, save/error state, restart status, and an explicit custom-directory cleanup exemption. | All nine controls round-trip through the daemon. User completes native interaction checklist. |
| P7: Closure | Update architecture, contracts, E2E catalog, and validation receipt. Re-run affected multi-process cases and existing full CLI E2E suite. | All stories map to evidence; platform limits and NOT RUN items are explicit; archive only after landing. |

P4 and P5 need the largest ownership change.
Do not present a settings form as completion of cache behavior.
If 0015 lands first, move host/API edits to its actual paths and preserve its single adapter session.
Do not make this feature depend on completing 0015.
Update 0015's unconditional local-only history-use description to state the default and the optional restore send before either proposal lands.

### Planned edit map

| Module | Planned change |
|---|---|
| `engine/types.go`, `options.go`, `state.go` | Typed policy, validation, defaults, update behavior |
| `engine/clipboard.go`, `submission.go`, `backend.go` | Automatic-send gate and restore dispatch without duplicate commit |
| `engine/history.go`, `history_types.go`, `history_sync.go` | Storage class, external custom-file references, availability, restore result and sync summary |
| `engine/content.go`, `receive.go`, `peerrecv.go`, `peerproc.go` | Default-cache-only reservations/deletion, custom finalization, receive admission, effective policies, lease accounting |
| `internal/store/history.go` and a focused cache store file | Durable payload rows and transaction helpers |
| `app/desktop/internal/config/` | New fields, atomic replacement, revision, effective/pending state |
| `app/desktop/internal/api/`, `internal/host/` | Configuration write transport and restore result transport |
| `app/desktop/cmd/unic/` | CLI configuration writer and settings page |
| `app/desktop/cmd/unicd/daemon.go` | Remove unsafe legacy cleanup and order startup reconciliation |
| `plugin/rpc/` and protocol schema, if required | Add only fields needed to enforce new global policy for external backends; retain version 1 |
| `e2e/cli/` | New cases, minimal black-box helpers, generated catalog, artifact export |

## 5. End-to-end acceptance and artifacts

Use real `unic` and `unicd` processes with isolated homes and directory clipboards.
The agent must not launch an application window, browser, or VM. The automated harness must not read or write the native system clipboard.
The user owns native visual and interaction acceptance.

Planned case families:

1. `sync-settings`: S1, S2, S4, persistence, conflict, and invalid-value paths.
2. `history-restore-sync`: S3, duplicate requests, apply conflict, Action distinction, and original sync prohibition.
3. `file-size-policy`: S6 and per-backend policy intersection.
4. `file-cache-location`: S5, default/custom storage classes, location changes, restart, crash, and custom-file preservation.
5. `file-cache-quota`: S7, default-cache concurrency, active pins, accounting, and custom receives while the default cache is full.
6. `file-cache-retention`: S8-S9, default-cache cleanup, metadata availability, and unchanged custom-file hashes after every cleanup trigger.

Set the public retention field to one hour for retention acceptance.
Use an offline fixture to stamp known completion times in a stopped disposable node.
Exercise the normal startup path after that fixture step.
Do not add a production clock setting or an undocumented seconds override.
Test the default 24-hour value by reading the effective configuration and its computed deadline.
Keep clock-transition testing bounded and confined to test-owned processes or fixtures; never change the host clock.

Retain an artifact directory for every E2E run:

- `manifest.json`: source SHA, dirty patch hash, binary SHA-256 hashes, toolchain, OS, case names, and results.
- `replay.sh`: build instructions, environment names, and exact case commands.
- `config/`: sanitized input TOML files and effective configuration responses.
- `assertions.json`: story IDs, step IDs, expected results, actual results, and timestamps.
- `payloads.sha256`: deterministic fixture hashes and received/exported content hashes.
- `cache-inventory.json`: payload IDs, storage classes, logical state, sizes, reservations, held flags, and physical presence.
- `custom-files-before-after.sha256`: unchanged custom-file hashes after quota pressure, expiry, history deletion, lease release, and restart.
- `cleanup-boundary.json`: default-root identity, cleanup trigger, candidate IDs, deletion results, and refusal reasons for out-of-scope paths.
- Sanitized command transcripts and node logs with no pairing ticket, address, source path, or clipboard body.
- A summary with passed, failed, skipped, and NOT RUN counts.

Export evidence before harness teardown on success and failure.
Use fixture-only content. Retain no production data, live credentials, or general database dump.
A byte/hash comparison proves delivered content. A send-operation state alone does not prove remote receipt.

Planned verification commands, after implementation:

```sh
go build -o .build/unic ./app/desktop/cmd/unic
go build -o .build/unicd ./app/desktop/cmd/unicd
UNIC_E2E=1 go test ./e2e/cli -run 'TestCLI/(sync-settings|history-restore-sync|file-size-policy|file-cache-)' -count=1 -v
UNIC_E2E=1 go test ./e2e/cli -count=1 -v
go test ./e2e/cli -run TestCatalog
go run ./tools/docscheck
```

The artifact export option does not exist yet. Add it to the harness in P1 and document the exact command there.
Do not copy the planned case commands into a completion receipt until the cases exist and pass.

Native acceptance checklist for the user:

1. Read the nine default values on the settings page.
2. Toggle each switch and reopen the page.
3. Save each numeric field, including an invalid value.
4. Select a directory, cancel selection, and restore the default.
5. Confirm the location change shows its restart requirement and the custom-directory exemption from quota, retention, and cleanup.
6. Restore text and a file with S3 off and on.
7. Check unavailable-file history after cache eviction.
8. Check keyboard navigation, narrow layout, and the active platform skin.

## 6. Pre-implementation verification snapshot

This snapshot records the initial research environment. Sections 7 and 8 contain later implementation and acceptance results.

- Feature implementation: NOT STARTED.
- New unit tests: none.
- CLI E2E, feature build, native UI, folder picker, and cross-platform acceptance: NOT RUN.
- Required module toolchain: Go 1.27.3, from `go.mod`.
- Installed local toolchain: Go 1.24.0.
- `go run ./tools/docscheck`: blocked before execution because the Go 1.27.3 download returned EOF.
- Documentation-only fallback: `GO111MODULE=off GOWORK=off GOTOOLCHAIN=local go run ./tools/docscheck/main.go` passed with zero warnings.
- Both new documents passed the technical-writing check with zero findings.
- `git diff --check` passed.

Do not lower `go.mod` or claim feature validation from a documentation-only check.

## 7. CLI and TOML implementation receipt

Implementation now covers S1-S9 through the CLI and configuration file.
P6, the settings GUI, remains deferred. This receipt does not claim native UI acceptance.
The current behavior is documented in [desktop runtime](../architecture/desktop-runtime.md).

- Configuration writes: `unic config set`, `unic config apply`, and `PUT /v1/config` with raw-file revision.
- Configuration status: `unic config --json` includes pending restart and runtime apply failure.
- Restore: `unic history use <ref> [--request-id KEY] [--json]` returns separate local and network results.
- Inventory: `unic dev cache --json` lists only default-cache IDs, bytes, reservations, availability, and held state.
- Default quota and retention: ledger-backed ownership, active pins, deferred startup clipboard reconciliation, and physical deletion accounting.
- Custom receives: exclusive creation, persistent external history references, and no lifecycle deletion authority.
- Crash recovery: ownership enters the SQLite ledger before payload creation. Incomplete entries remain charged until safe cleanup succeeds.
- Existing unrelated GUI edits remain intact. An initial UI compile failure required a temporary build overlay; final builds use current source without that overlay.

Validation artifacts use `.build/sync-settings/evidence-cli-v4/`.
Race-enabled CLI and daemon binaries use `.build/sync-settings/bin/`.
Final command:

```sh
UNIC_E2E=1 UNIC_E2E_BIN="$PWD/.build/sync-settings/bin" UNIC_E2E_ARTIFACTS="$PWD/.build/sync-settings/evidence-cli-v4" go test -race ./e2e/cli -run TestCLI -count=1 -parallel 4 -v
```

Full CLI result: PASS, 13 of 13 cases, 176.883 seconds, with race-enabled CLI and daemon binaries and test parallelism 4.
The final cache timestamp correction now records completion after file and directory synchronization.
A focused rerun of all four cache and file-size cases passed in 65.124 seconds against `.build/sync-settings/bin-final/`.
A final portability adjustment reuses the existing directory-sync helper, which already accounts for Windows directory-handle limitations.
The same four cases passed against `.build/sync-settings/bin-portable/` in 96.516 seconds; no other runtime behavior changed.
Final per-case artifacts are in `.build/sync-settings/evidence-portable-cache/`.
The aggregate local receipt is `.build/sync-settings/validation.json`.
The final source-file inventories match the current working tree.
Its artifacts use `.build/sync-settings/evidence-final-cache/`.
`go vet` passed for the affected Engine, store, configuration, API, host, CLI, daemon, and E2E packages.
`go test ./... -run '^$'` compiled all packages and their existing tests without executing unit tests.
`git diff --check` passed.
`go run ./tools/docscheck` passed with four stale architecture warnings.
The required `-bump` commands refused uncommitted code paths. Verified markers must be updated after the implementation receives a scoped commit.
No unrelated GUI changes were staged or committed to bypass that guard.
The retained manifest includes binary hashes, tracked patch hash, and source-file hashes, including untracked implementation files.
Each case also retains configuration snapshots, default-cache inventory, fixture hashes, and a replay command.
The test harness does not retain node databases, pairing tickets, or daemon logs.

The following acceptance remains NOT RUN: native clipboard/picker interaction, Windows/Linux native execution, forced disk-full and unlink failures, and crash injection at every file/database instruction.
The current CLI E2E includes restart/crash recovery and a default-cache SIGKILL recovery path.
No new unit tests were added.

## 8. Host-owned receive storage correction

The user confirmed that a custom save location belongs to the host.
The correction removes `Options.ReceiveDir`, custom-directory validation, and custom file creation from Engine.
The desktop host now owns those operations in `host/received_files.go`.
Engine accepts only `ReceivedFile` stream metadata and a callback that returns `PersistentFileSource`.
The host selects Engine-private storage through `Options.CacheDir`.
Quota, retention, pins, and deletion authority remain in the private-cache module.

The correction also removes a filesystem assumption from `LocalFilePath`.
A separate `FileReference` method returns opaque host references.
The desktop host encodes custom received references with `desktop-received:` and resolves them for clipboard use.
A future Android host can implement the same callback with document-provider streams and opaque references.
No Android native integration is claimed.

The CLI response keeps `custom_destination`, but the desktop host adds that field.
Engine cache inventory contains only private-cache state.

New acceptance: `host-received-files` checks missing destination failure, no fallback cache, opaque persisted references, restart/read/apply, history expiry, and user-file preservation.
The retention case now proves cleanup-off behavior with an unheld expired file before checking cleanup-on behavior with a separate pinned file.
The [CLI matrix](../../e2e/cli/MATRIX.md) distinguishes current assertions from remaining fault-injection and native acceptance gaps.

Current validation: five focused cases passed against the refactored race-enabled binaries before strengthening the retention case.
Full CLI regression with the strengthened retention case: PASS, all 14 cases, 194.56 seconds.
The CLI, daemon, and harness use race detection. The harness also checks child daemon race reports.
All packages compile with unit tests excluded from execution. Affected-package `go vet` and `git diff --check` pass.
Engine dependency inspection finds no application, mygo, TOML, or file-watcher dependency.
Docscheck passes with four stale architecture warnings. Its verified update still requires a scoped commit.
Aggregate receipt: `.build/receive-storage/validation.json`.
The receipt records source inventory changes since case completion.
Artifacts: `.build/receive-storage/full-evidence/`.
Binaries: `.build/receive-storage/bin/`.
No feature UI was launched. No unit tests were added.

## 9. Scoped commit and architecture verification

Implementation commit: `85dbf35a5643f15a4a54eef062f4888e62308eec`.
The commit contains sync settings, host-owned receive storage, contracts, and CLI E2E coverage.
Unrelated desktop UI changes remain outside the commit.
A clean export of the staged source built both CLI and daemon successfully before commit.
The previous 14-case race-enabled E2E result remains the runtime acceptance evidence.
The desktop-runtime and pipeline architecture pages now verify this implementation commit.
Docscheck passes with two remaining warnings for the identity-and-trust and overview pages.
Those pages cover broader paths and are not re-verified as part of this change.
