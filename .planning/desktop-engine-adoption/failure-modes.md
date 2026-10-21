# Failure modes: Desktop adoption of the Engine product-settings removal

Written before the code. Each mode names the E2E check that must prove it.
Engine pin: `4c363556f6deab7ce93fea679bcba33553db5571` (PR #227 head; contains #225/#226).

## Field set 2 import (`general.debug_mode`)

| # | Failure | Check |
|---|---|---|
| F1 | The new Engine loads and saves `settings.json` before the import; `debug_mode` is lost. | Start from a pre-P1 profile with `debug_mode=true`; value survives the first Engine save. |
| F2 | Direct pre-P1 -> this release runs only set 2 and skips set 1. | Same profile also carries shortcuts, theme, legacy `silent_start`; all imported. |
| F3 | A P1 profile (`fieldSet=1`) re-imports set 1 and overwrites user edits. | P1 profile with edited shortcut + `debug_mode=true`; edit stays, debug imported. |
| F4 | After the new Engine erased the product keys, the D8 re-import sees "absent" legacy values and wipes the product values (stored set == current, fingerprint differs). | Third start after Engine save: product values unchanged. |
| F5 | `debug_mode` is not a bool (`"yes"`) or null; the invalid value stops startup or reads as true. | Default false, warning names the field, session starts (B3). |
| F6 | Crash between import write and Engine load repeats or half-applies the import. | Kill after the product file write; next start identical. |
| F7 | Legacy file unreadable: marker advances anyway and the value is lost. | Corrupt `settings.json`; marker not advanced, retried. |

## Log profile

| # | Failure | Check |
|---|---|---|
| L1 | Stored preference is reported as the effective profile. | `debugMode=true` stored, env override set: `effectiveLogProfile` is the installed profile. |
| L2 | The profile is read before the import ran, so a pre-P1 `debug_mode=true` is ignored on the first start. | First start from pre-P1 profile installs the `debug` profile. |
| L3 | Engine is started without the label, or with the preference label instead of the installed one. | Diagnostics GET shows the installed label; undeclared is never produced by the daemon. |
| L4 | PUT calls the removed Engine operation, or reports `restartRequired=false` when the preference differs from the installed profile. | PUT then GET: stored changes, installed unchanged, `restartRequired=true`; restart applies. |
| L5 | `/settings` aggregate still reads `debugMode` from Engine. | PUT `/settings` debugMode, GET `/diagnostics/debug` agrees; Engine settings.json has no key. |

## Factory reset (D5)

| # | Failure | Check |
|---|---|---|
| R1 | Product file deleted although the Engine reset failed. | Inject Engine reset failure; file bytes unchanged. |
| R2 | Engine reset succeeds but the product file survives; later start restores old preferences. | After reset: file absent, GET returns defaults, restart keeps defaults. |
| R3 | File deletion fails and the API reports success. | Make the delete fail; API returns an explicit error, memory not reset. |
| R4 | Retry impossible because the second Engine reset is rejected. | Retry after R3; succeeds and deletes. |
| R5 | Memory state, revision and `settings.changed` are not updated after delete. | WS event received; GET shows defaults without restart. |
| R6 | A concurrent settings write recreates the file after delete. | Delete and apply are serialized by the store lock. |
| R7 | `desktop-telemetry.json` deleted. | File byte-identical after reset. |

## Export, import, upgrade backup (D6)

| # | Failure | Check |
|---|---|---|
| E1 | Files not declared: updater state files and the product file drop out of the bundle or the reset. | Declarations asserted from a real bundle's restored files. |
| E2 | Restored product file is overwritten by the daemon's stale in-memory copy. | Import into an existing profile, restart, change one field: restored values stay. |
| E3 | Cross-installation restore on a new profile loses the product file. | Export from profile A, import into fresh profile B: equal bytes. |
| E4 | Old incomplete bundle (task 8 rejection) surfaces as a generic failure. | Preview/stage returns 422 `INCOMPATIBLE_BUNDLE` with the re-export reason; profile untouched. |
| E5 | Telemetry consent travels in the bundle. | Bundle has no `desktop-telemetry.json`. |
| E6 | Content or credentials leak into the product file. | Scan of the product file after all runs. |
| E7 | Upgrade backup lacks the product file. | Backup archive from a real upgrade contains it. |

## Removal of dead Engine surface

| # | Failure | Check |
|---|---|---|
| D1 | Engine event `SettingsSection::{KeyboardShortcuts,QuickPanel}` mapping or `UpdateDebugMode` references remain. | Workspace builds; grep finds none. |
| D2 | OpenAPI/CLI/frontend contract drifts from the handlers. | `schema/openapi.json` regenerated and checked; CLI `debug` run against the daemon. |

## Incomplete import protection

F8: A failed product-file import is followed by a new Engine write that erases the only
legacy copy. Start a real daemon with the product path occupied by a directory and valid
legacy product values. Require startup failure before Engine I/O, byte-identical legacy
settings, and successful migration after correcting the file type. Current Engine settings
without product keys must still allow lenient product-file fallback.

## Review follow-up failures

- Initial import warnings can be lost before tracing is installed. Use a real daemon with
  invalid legacy startup mode and a user-written theme value; require the invalid-field
  and kept-user-value diagnostics after tracing starts, without changing initial log selection.
- Unlink success without directory synchronization can resurrect preferences after power loss.
  Synchronize the parent on Unix, including retry after an earlier unlink. Real reset/restart
  checks cover the normal path; power loss and Windows durability remain unverified.
- A daemon stop failure can abort cleanup of other profiles and omit receipts. Kill one owned
  real daemon, then clean up it and another owned real daemon; retain both exit codes and
  receipts, and preserve the original test exception when cleanup also fails.
- A stopped owned daemon cannot process TERM. Stop it with SIGSTOP inside the isolated
  runner, require timeout escalation to SIGKILL, and retain its failure receipt while
  still stopping a second running daemon normally. Only the spawned process handle is used.

## Persistent rejected-import feedback (user-confirmed policy)

Record these failures before implementing the Desktop consumer:

- Rejected old staging is deleted or silently applied. Require staged-data hashes to remain
  available and the original initialized space to start and remain queryable.
- A one-time event/toast disappears after reconnect or restart. Query Engine's actual durable
  rejection contract on startup and on settings entry/reconnect. Keep an inline settings notice;
  do not use localStorage or a parallel persisted client status as the authority.
- A rejection is mistaken for key/profile recovery or first-run setup. Keep the original-space
  gates unchanged. Present the rejection without stopping normal original-space use.
- A startup notice is suppressed forever. Dismiss only for the current client session; show it
  again after a new client start while the authoritative rejection remains.
- The recovery path requires deleting the preserved staging or resetting the original space.
  Reuse the existing config picker, preview, confirmation and stage/restart path after a new
  export. Never make deletion/reset the recovery action.
- The old source pin has no durable rejection contract. Do not invent a temporary Engine API
  or accept a log-only implementation. Wait for the corrected contract in remote task 9.
- Re-export guidance is generic or lost. The Simplified Chinese message must read:
  “导入未完成，需要使用新版本重新导出配置包”. Keep equivalent guidance in every locale.
- UI feedback is claimed visually accepted without a user check. Use real-daemon receipts and
  non-visual structural checks; retain native/visual acceptance as a separate user gate.

## Preservation contract follow-up (Engine task 8 at 7a9e639f)

- A missing rejection is reported as a successful import. Treat `None` only as no current
  rejected input. Never infer application success from that value.
- A diagnostics I/O failure is converted into an absent rejection. Propagate the error and
  show a separate status-read failure while leaving the original-space gates unchanged.
- A host reads private marker/staging files to reconstruct public status. Use only the actual
  `QueryDiagnostics.rejected_config_import` summary and retain its reason.
- A generic identity-move confirmation is reused as authorization to destroy rejected staging.
  Require a separate explicit warning: the preserved staging may be the only copy and its
  replacement cannot be undone. Cancel/back must leave it untouched.
- The host silently enables replacement. Default the Desktop request to false and forward that
  value unchanged through the Go host and daemon to `StageConfigImportInput`.
- A wrong password releases the only rejected copy. With explicit replacement authorization,
  send a wrong password first and compare the retained marker and complete staging inventory.
  Only a fully validated new package may replace the old input.
- Stale status permits an unconfirmed replacement after reconnect. Requery authoritative status;
  an unconfirmed stage must remain false even when the old status was absent or a read failed.
- Error feedback disappears into a toast. Retain a settings status-read failure with retry and
  retain the startup rejection across new client sessions. Do not store a permanent client ack.

The final adoption source must be the synchronized remote task-9 SHA, not the task-8 SHA above.
Do not claim these cases passed until the real Desktop daemon executes against that source.

## Existing CLI E2E alignment before implementation

- CI requests historical tests without providing historical inputs. Prepare the pinned v1.2.0
  release daemon and the hash-verified 0.19.4 Engine fixture before running the full suite.
  Fail preparation on download, platform, source or digest mismatch; never report a skip as pass.
- Field-set-1 expectations reject a correct field-set-2 migration. Keep exact file and unknown-key
  assertions but update the approved version and normalized empty fingerprint.
- Info-level migration assertions run with warn logging. Set the suite's explicit info override;
  keep privacy assertions and compare actual input/source hashes.
- An incomplete product migration is expected to reach API readiness. Instead require a real
  failed startup with unchanged legacy bytes, then repair only the synthetic blocker and prove
  successful, repeatable migration and restart.
- Engine-erased product fields are mistaken for a new import. Require product bytes and values
  to remain stable; do not require an import event for input that no longer has product fields.

## Review: status read recovery

- A failed pre-stage read and the settings query keep separate error state. A successful retry
  can leave stale failure feedback. Route pre-stage reads through the same RTK Query endpoint
  as startup/settings and remove the parallel error flag. Successful reads must clear its error
  before a replacement prompt; failed reads must keep failure feedback and never call Stage.

## Review: cache-driver protocol lifecycle

- Missing readiness, a request that never returns, or endless non-receipt output can leave
  stdout.readline blocked forever. Require one bounded deadline per protocol receipt.
- The child can exit early or close stdin between poll and close. Preserve early-exit failure
  and collect its owned exit code without skipping daemon cleanup.
- A pipe read can fail. Surface that failure rather than waiting without a result.
- A platform-specific pipe selector can break Windows. Use a standard reader thread and queue.
- A timeout can leave the child alive. Terminate only the Popen-owned child; retain the timeout
  and cleanup receipts, and allow the existing bounded escalation to finish cleanup.

## Merged Engine source and CI completion

- A merged PR can have a rewritten history: verify the actual pin is an ancestor of remote main.
- Compare production, binding and lock sources before retaining any previous acceptance scope.
- Rebuild every artifact that embeds the Engine or Desktop source revision. Preserve old receipts.
- Cargo update can change unrelated dependency edges: retain the prior graph and verify --locked.
- Cold builds and the full CLI suite can exceed 45 minutes: retain cancellation annotations and
  rerun the entire suite with a 90-minute job budget, without adding skips or removing assertions.
- Keep the existing Swagger/zip audit failure visible; do not upgrade or add exemptions here.
