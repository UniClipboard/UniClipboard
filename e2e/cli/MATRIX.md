# CLI acceptance matrix: Sync settings and receive storage

The cases below use real CLI and daemon processes with directory clipboards.
The matrix describes assertions in the scripts, not native Android or desktop UI acceptance.
The latest run receipt is in [execution plan 0016](../../docs/exec/0016-sync-and-file-settings.md).

| Requirement | Case | Assertions |
|---|---|---|
| S1: Global sync | `sync-settings`, `history-restore-sync` | Outgoing pause, local history, no replay after resume, restore cannot bypass pause. |
| S2: Automatic sync | `sync-settings` | Automatic sends stop; local capture, manual sends, and remote receives continue. |
| S3: Restore sync | `history-restore-sync`, `plugin-sensitive-blocks-sync` | Default local-only behavior, optional send, request deduplication, no duplicate local history, stored processor prohibition. |
| S4: File sync | `sync-settings` | Global file-send restriction blocks automatic delivery. |
| S5: Host-owned destination | `host-received-files`, `file-cache-scope` | Custom path selection and restart, host write failure without cache fallback, custom file larger than cache quota, return to default. |
| S6: Transfer limit | `file-size-policy`, `sync-settings` | Exactly 100 MB accepted; one byte over rejected; sender and receiver limits apply; rejected local files remain in history. |
| S7: Private cache quota | `file-cache-scope` | Clipboard pin blocks reclamation; release permits unified record/file deletion; SIGKILL preserves retained bytes. |
| Unified cleanup | `history-auto-cleanup`, `file-cache-retention` | Live count/age rules, exact OR tag exclusions, unlimited count, restart, coupled record/file deletion, and removed legacy configuration keys. |
| Configuration persistence | `sync-settings` | CLI set/apply, stale revision conflict, invalid value leaves the file unchanged, invalid external TOML preserves typed state, CLI repairs the file, restart persistence. |
| Host reference opacity | `host-received-files` | SQL ownership assertion finds an opaque desktop reference and no owned blob; CLI reads and restores the reference after restart. |
| Host-file deletion authority | `host-received-files`, `file-cache-scope` | Custom bytes survive history expiry/deletion, lease release, restart, and location changes. |
| Cleanup scope | `file-cache-boundary` | Payload symlink targets, moved original data, and unrelated familiar-prefix files survive cleanup. |
| Existing content paths | `content-formats`, `plugin-tag-and-action` | Text, HTML, PNG, files, plugin tags, and Actions still work. |
| Identity and process recovery | `pair-and-sync-text`, `restart-keeps-state`, `data-dir-single-instance`, `crash-and-suspend-recovery` | Approval gates, persistent identity/history, exclusive profile use, reconnect after process loss. |

## Remaining acceptance

The matrix does not claim the following checks:

- Android document-provider integration or native clipboard permissions.
- Desktop settings GUI, native folder picker, Windows native execution, or Linux native execution.
- Forced disk-full and unlink failures, or crash injection at each file/database instruction.
- A callback implementation that returns a wrong size, an empty reference, a panic, or an unreadable source.
- Every policy permutation, concurrent quota reservation, exact wall-clock deadline, or source-mutation race.

Engine validates callback results in code. A successful ordinary host save does not prove all callback failure branches.

## Reproduction

Build matching race-enabled `unic` and `unicd` binaries, then run:

```sh
UNIC_E2E=1 UNIC_E2E_BIN="$PWD/.build/receive-storage/bin" UNIC_E2E_ARTIFACTS="$PWD/.build/receive-storage/full-evidence" go test -race ./e2e/cli -run TestCLI -count=1 -parallel 4 -v
```

Each artifact records binary hashes, source hashes, fixture hashes, configuration, cache inventory, and a replay command.
The harness checks child daemon logs for race reports before removing test-owned state.
