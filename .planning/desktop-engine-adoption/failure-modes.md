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
