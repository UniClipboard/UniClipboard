# Receive item results and selective retry

Desktop adoption of Engine PR #230 (`UniClipboard/Engine` commit
`5cbca8fc24a278fa7b01ae611bf4b925914bef13`). The Engine owns attempts, retries,
cancellation, persistence and SDK shutdown. Desktop only carries the Engine's
answers to the UI and carries the user's selection back.

This page is in English at the request of the task that introduced it; the rest
of `docs/` is Chinese.

## Dependency pin

- `Cargo.toml` pins `uc-engine` to the full commit above; `Cargo.lock` follows.
- The Engine now needs `tokio >= 1.53.3` (locked: 1.54.1) and the iroh-blobs fork
  at `152c42a9217f1377a998b271e0052311e8ada789`. `crates/p2p-bench` names that
  fork directly, so its revision moves with the Engine's and the workspace still
  resolves one `iroh-blobs`.
- Not changed here: `RUSTSEC-2026-0387` (`crypto-bigint` 0.5.5 and 0.7.5),
  `RUSTSEC-2026-0385` (`ctutils`) and the three `zip` 1.1.4 advisories were already
  in the lock before the bump and are unchanged. The bump removes the `tokio`
  1.53.1 warning (`RUSTSEC-2026-0386`). No audit exception was added.
  Engine's own failing audit job and its MBX R2 jobs are separate findings that
  Desktop does not resolve.

## Daemon API

All routes sit under the existing `/clipboard` group and keep the `{ data, ts }`
envelope. Existing fields are unchanged; new fields are additive.

| Route | Change |
| --- | --- |
| `GET /clipboard/entries/{id}/receive`, `GET /clipboard/receives` | `EntryReceiveProgressResponse.items[]` added: `itemId`, `kind`, `state`, `failureReason` |
| `POST /clipboard/entries/{id}/receive/cancel` | new outcome `cancellation_incomplete` |
| `POST /clipboard/cancel-transfer/{transfer_id}` | new outcome `cancellation_incomplete` |
| `POST /clipboard/entries/{id}/receive/retry` | new: body `{ expectedAttemptId, itemIds }`, result `{ outcome, attemptId?, reason? }` |

`outcome` of the retry route is one of `accepted`, `already_receiving`,
`superseded`, `cancellation_incomplete`, `unavailable`. `reason` is present only
with `unavailable` (`source_unavailable` or `access_denied`); `attemptId` only
with `accepted`. An empty selection or malformed ids is HTTP 400; other Engine
errors keep the existing redacted mapping. `itemIds` must come from the latest
query; they are opaque, bound to the queried attempt, and never derived from file
names. A daemon that predates `items` yields an empty list in the client.

Events (`clipboard.receive_attempt_state_changed`, `file-transfer.*`) are
unchanged. They carry no item array; they only invalidate what the UI holds.

## Semantics the UI keeps

| Rule | Where it is enforced |
| --- | --- |
| `accepted` admits a new attempt; a file is saved only when a later query says `completed` | `retryOutcomeMessageKey` wording; the hook re-reads the snapshot after every retry |
| An aggregate `failed` or `cancelled` attempt can still hold saved files | `summarizeReceiveItems`; rows are rendered per item, nothing is removed on failure |
| Saved items are never retried or deleted | `retryableItemIds` selects only `failed`/`cancelled` items of a `failed`/`cancelled` attempt |
| A directory is one `directory_unit` row, retried whole | the Engine groups it; the UI never expands members |
| `cancellation_incomplete` is not "cancelled" | `markReceiveCancellationIncomplete`; cleared only when that attempt settles |
| A late event or reply of an old attempt cannot overwrite the new attempt | `supersededAttemptsByEntry` in `fileTransferSlice` ignores retired attempts; a settled attempt is never reopened by an older snapshot |
| Events are invalidation, the snapshot is the truth | `useEntryReceive` re-reads when events disagree with the stored snapshot, and only the latest reply is applied |
| `source_unavailable` after a restart is shown as is | retry outcome `unavailable` has its own message; results stay unchanged |
| Sender resend is not receiver retry | resend keeps its own route and hook; retry only exists for received entries |
| No host scheduler, retry loop or database access | none was added; Go talks to the daemon over HTTP/WS and no Go FFI layer exists |

## Where the code is

- DTOs: `crates/uc-daemon-contract/src/api/dto/clipboard_command.rs`
- Route and mapping: `crates/uc-webserver/src/api/clipboard.rs`; OpenAPI
  registration in `crates/uc-webserver/src/api/openapi.rs`; the generated
  `schema/openapi.json` and `apps/gui-go/frontend/src/api/generated/`
- Rust client: `retry_entry_receive` in `crates/uc-daemon-client/src/http/clipboard.rs`
- Frontend logic (shared by macOS, Windows and Linux): `src/lib/receive-items.ts`,
  `src/hooks/useEntryReceive.ts`, `src/hooks/useCancelEntryTransfer.ts`,
  `src/store/slices/fileTransferSlice.ts`, `src/api/file_transfer.ts`
- Frontend views: `src/pages/history-kit/HistoryKitReceive.tsx` (the kit used by all
  three platforms) and `src/components/clipboard/ReceiveItemsPanel.tsx` (the legacy
  history detail reached only by builds without a platform). Both are views over
  `useEntryReceive`.
- Strings: `clipboard.receive.*` in all six locales.

## Failure model and checks

Written before the code; each row failed on the first run and passes now.

| Failure | Check |
| --- | --- |
| Retry route missing, empty or malformed selection accepted | `tests/e2e/tests/entry_receive.rs` against a real `uniclipd` |
| Retry without a receive reported as `accepted` | same file; the Engine answers `unavailable` / `source_unavailable` |
| OpenAPI lacks retry or item schema | same file, reads the daemon's `/api-docs/openapi.json` |
| Incomplete cancellation reported as cancelled, retry outcomes mis-mapped, `items` dropped, wire keys not camelCase | unit tests in `uc-webserver`, `uc-daemon-contract`, `uc-daemon-client` |
| Saved file hidden by a failed sibling, retry sends wrong ids, stale attempt revives, incomplete cancel shown as cancelled | Vitest: `receive-items`, `fileTransferSlice.receiveItems`, `file_transfer.receiveItems`, `ReceiveItemsPanel`, `useCancelEntryTransfer` |

## Not run

A partially delivered receive needs two paired devices; one isolated daemon cannot
produce one. Not run: cross-device delivery and cancellation, per-file retry
against a real failing sender, directory publication and rollback, restart with
original retry material, mixed-version pairs, physical devices, and every visual
check of the new rows (the task owner does the visual acceptance).
