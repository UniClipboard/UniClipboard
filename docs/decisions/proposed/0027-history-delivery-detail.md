# 0027: History detail delivery list and send progress

- Status: Implemented. Engine/store targets, desktop device rows with single-device resend, and live byte progress pass process E2E (`TestHistoryTargets`, `TestHistoryTargetProgress`). Native window acceptance passed for sent, live progress with file popover, failed row, resend confirmation and single-device resend (cases `history-delivery`, `history-delivery-progress`, `history-delivery-resend`). Contract landed in the [history contract](../../contracts/history-contract.md).
- Date: 2026-11-16
- Scope: Per-device delivery state and byte progress of a history entry, from Engine to the desktop detail pane.
- Related decisions: [0024 directory copy and send](0024-directory-copy-and-send.md), [0017 history cleanup](0017-history-auto-cleanup.md).
- Landing targets: [history contract](../../contracts/history-contract.md), [Engine contract](../../contracts/engine-contract.md), [overview](../../architecture/overview.md#live-desktop-composition).

## Context

The v1 detail pane (tag v1.6.2, `apps/gui-go/frontend`) shows one row per paired device. Each row has a status, a progress ring, and a hover popover with per-file progress. A failed device has its own retry action.

The v2 detail pane shows one line: `同步状态：已发送`. This is the aggregate `HistoryItem.SyncState`. The per-device list exists only in the demo and uses fake data.

The Engine already tracks `Operation.Targets[]` per device. This data lives only while the operation lives. It has no bytes and it is not stored per history entry. The ui-kit already has `TransferStatus`, `TransferIndicator`, `TransferItem` and `InsetList`.

## Decision

Keep the v1 layout. Change the data source. Do not add a receipt layer.

### 1. Per-device state

A history entry stores the last send result for each target device:

| Field | Meaning |
|---|---|
| `backend_id`, `target_id` | The target. For a device, `target_id` is the device ID. |
| `state` | `queued`, `sending`, `sent`, `failed`, `superseded`. Same values as `TargetState`. |
| `error_code` | Stable failure class. Empty unless `failed`. |
| `items` | One item per sent part, in part order. See section 2. |

`sent` means every byte was written to the connection. It does not mean the peer received the content. The UI must say 「已发送」. The UI must not say 「已送达」.

`HistoryItem` gets `targets`. Persist the final state when the operation completes. Store it in a new table, `clip_item_target`, beside `clip_item_sync`. A restart turns open targets into `failed` with code `interrupted`, as `InterruptPending` does for the aggregate.

The aggregate `sync_state` stays. The list badge uses it. The detail pane does not use it.

### 2. Byte progress

The Engine hands each part to the backend as an `io.ReadCloser` (`peerSendSource.OpenPart`). Wrap that reader with a counter. This needs no plugin RPC change and works for every backend.

Each target item has `index`, `kind`, `state`, `completed_bytes` and `total_bytes`. `total_bytes` is the part size known at commit.

Item state:

| State | Rule |
|---|---|
| `waiting` | The part was not opened. |
| `transferring` | The part was opened and is not at EOF. |
| `completed` | The reader reached EOF. |
| `failed` | The target failed while this part was open. |

Reaching `total_bytes` never means the target is `sent`. The target state decides.

Progress lives in memory on the operation. It is not written to the database. Publish a change at most every 250 ms per target, and always on a state change. The event is `EventOperations`. The client reads the full snapshot after the event.

A running entry's `targets` merge the live operation over the stored rows. After completion, the stored rows are the only source.

### 3. Detail pane

Layout:

- Heading `送达`. Beside it, secondary text `N/M 台设备`. N counts `sent` targets.
- One `InsetList` row per paired device. Title is the device name. Trailing control is `TransferStatus`.
- The footnote `本地加密存储 · 设备之间端到端加密` stays.
- If the entry has no targets and no paired device exists, replace the list with `暂未配对任何设备`.
- Offline devices without a target show `设备离线`.

`TransferStatus` rules:

- The indicator shows a ring when `completed_bytes` and `total_bytes` are known. It shows a spinner when the total is unknown. It never shows a fake percent.
- The hover popover lists each file with its state and bytes. Use file names from the entry. Use `文件 N` if no name exists.
- A failed row shows the failure reason. A completed row stays quiet.
- A click on a failed or superseded row sends the entry again to that device only. Ask the user to confirm first. State that the device may get duplicate files.

### 4. Out of scope

- Peer receipts. A receipt needs a wire change. Do not add it here.
- Resume of an interrupted transfer. Nothing resumes. The user sends again.
- Aggregate progress across devices. Each device stays separate.

## Layers and order

Build each layer end to end before the next one.

1. Engine and store: persist `targets`, expose them in `HistoryItem`, API and DTO. E2E: CLI send to two devices, one offline, read `targets` from `unic history`.
2. Desktop: replace the aggregate line with the device list and states. Resend one device.
3. Byte progress: counting reader, throttled events, `TransferStatus` ring and popover. E2E: send a large file, observe `transferring` with rising bytes, then `sent`.

## Failure cases to cover before coding layer 3

- The reader is closed early: the item stays `transferring`, the target becomes `failed`.
- The same part is opened twice: the grant allows one open. A second open fails and counts nothing.
- A superseded target never opens a part: items stay `waiting`.
- Event flood on a fast local link: the 250 ms throttle bounds it.
- A restart during a send: open targets become `failed` with `interrupted`.
- A resend while the first send runs: the Engine refuses or supersedes. The row must show one state.
- A removed device: its stored row stays until the entry is deleted. The UI shows the stored name.
