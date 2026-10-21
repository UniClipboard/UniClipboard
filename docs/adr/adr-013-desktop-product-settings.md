# ADR-013: The daemon owns Desktop product settings

- **Status**: Accepted. In progress: the store, its daemon wiring, the `general.trayIcon`
  field on `/settings`, the `settings` WS topic and the Go host subscription (task 2) are
  implemented; the tray executor and settings UI (task 3) are not.
- **Date**: 2026-10-20
- **Related decisions**:
  - [ADR-008](../architecture/adr-008-uniclipd-split-gui-as-client.md)
  - [ADR-011 (daemon connection file)](../architecture/adr-011-daemon-connection-discovery-conn-file.md)
  - The configuration rule in `VISION.md`, section "安全与隐私底线"
- **Engine baseline**: `f0627a8b0bcc274707035a86ab2df47154d1e536`

## Context

The Engine `settings.json` holds both Engine behaviour settings and Desktop shell preferences:
shortcuts, the quick panel, theme, startup, update and language. The Engine stores these shell
fields but never reads them. This causes three problems:

1. One invalid enum value in a product field stops the Engine session from starting.
2. The Engine drops unknown fields when it saves, so a newer Desktop cannot add product fields
   there.
3. The meaning of these fields belongs to Desktop, but the portable Engine defines their types.

The tray icon preference (`general.trayIcon`) is the first new product field. It must not go
into the Engine.

## Decision

### 1. Ownership

- A new crate, `crates/uc-desktop-settings`, owns the product settings model, defaults,
  per-field validation, lenient loading, atomic storage, the in-process single writer, the
  revision and change notification.
- It depends only on `serde_json`, `tempfile`, `tokio` (`sync`, `rt`) and `tracing`. It does not
  depend on `uc-engine`, `uc-webserver` or any HTTP crate, so `uc-bootstrap` can call it before
  the Engine starts.
- The daemon constructs one `DesktopSettingsStore` after it holds the profile instance lock and
  before `Engine::start`, then injects it into `DaemonApiState`. The instance lock allows one
  daemon process per profile, so the file has one writer. Inside the process, one async mutex
  in the store serializes writes.
- `uc-webserver` only maps the protocol: `GET /settings` composes the store's effective value
  into `general.trayIcon`; `PUT /settings` hands `general.trayIcon` to the store. All other JSON
  paths are unchanged.
- File location: `<app_data_root>/desktop-settings.json`, in the same directory as the Engine
  `settings.json` and `desktop-telemetry.json`. The telemetry consent keeps its own file (D9).

### 2. File format

```json
{
  "version": 1,
  "import": { "userWritten": ["general.trayIcon"] },
  "general": { "trayIcon": "cat" }
}
```

- The file is plain JSON. Under the configuration rule in `VISION.md` it needs no encryption
  and no per-item approval. It must not hold clipboard content, data derived from clipboard
  content, or credentials. The per-field validation exists so that an invalid value can never
  stop startup and can never reach an OS API.
- `version`: the file format version. An existing value is kept as it is, including a higher
  version.
- `import.fieldSet`: the highest field set imported from the Engine `settings.json`. A missing
  value means 0. P0 imports nothing and does not write this key; an existing value is kept. The
  task 4 importer uses this key as its marker. File existence is not the marker.
- `import.userWritten`: the field paths that an explicit write has set since the file was
  created, in write order, without duplicates. Every explicit write adds its paths. The
  importer imports only fields that are not in this list.
- `import.sourceFingerprint`: used by task 4. P0 keeps an existing value as it is.

### 3. Load rules

| Input | Effective value | File rewritten on load |
|---|---|---|
| File missing | defaults | no; the first explicit write creates it |
| Not valid JSON, root not an object, larger than 1 MiB, or unreadable | defaults; a warning gives the reason category | no; the next explicit write replaces the whole file |
| Field missing | the field's default | no |
| Field invalid (unknown enum value, wrong type) | the field's default; the warning names the field path, never the value | no; the raw value stays until the user writes that field |
| Unknown key | ignored | no; later writes keep it |
| Higher `version` | read leniently as usual | no |

Warnings are logged once, at load.

### 4. Write rules

1. The patch is validated before it reaches the store. An invalid value is rejected and nothing
   is written.
2. The store copies the current raw document and replaces only the field paths the patch names.
   It adds those paths to `import.userWritten`. All other fields, including invalid values and
   unknown keys, stay as they are.
   - If there is no usable document, the write starts from an empty object.
   - If `general` or `import` is not an object, it is replaced by an object.
   - If `version` is missing, it is set to `1`.
3. If the new document equals the current raw document (a same-value write), the store does not
   write the file, does not change the revision and sends no notification. It returns success
   with `changed: false`. If the effective value is the same but the raw document differs (for
   example the stored value was invalid, or the path was not yet in `userWritten`), the write
   counts as a change.
4. Atomic write: temporary file in the same directory, `sync_all`, `persist`.
5. Only after the write succeeds does the store replace its memory state, add one to the
   revision and send the notification, while it still holds the lock.
6. If step 4 fails, the memory state, the revision and the notifications are unchanged, and the
   caller receives an error.
7. Steps 2–5 run on their own task. A caller that is dropped mid-write cannot leave the file
   changed while the memory state, the revision and the notification stay behind.

### 5. Revision and notification

- The revision is an in-memory `u64`. It starts at 0 with each daemon run and grows by one per
  persisted change. It is monotonic only within one daemon run and is never written to the file.
- Notifications go through `tokio::sync::broadcast`. The payload is `{ revision, sections }` and
  carries no values; subscribers read a new snapshot. Notifications arrive in revision order. A
  subscriber that lags (`Lagged`) must read a new snapshot.
- The daemon maps this notification to the WS event `settings.changed` (section 7).

### 6. `PUT /settings` order (P0)

1. `general.trayIcon` is validated while the request is decoded. An invalid value is rejected as
   a client error and nothing is written.
2. Without `trayIcon`, the Engine patch is sent to Engine `UpdateSettings` only if it is not
   empty. A request that holds only `trayIcon` does not reach the Engine and does not change
   `settings.json`.
3. The product settings are written after the Engine part succeeds. If the Engine part was
   written and the product write fails, the handler returns an internal error and logs it. Task 4
   adds the typed `settings_partially_applied` error with `applied` and `failed` lists. Until
   task 3, the frontend does not send `trayIcon`, so the GUI never sends a mixed patch in P0.

### 7. Settings change events (WS topic `settings`)

Clients learn about changes from the daemon WebSocket, never from a page-originated event. The
events carry no setting values: clients read `GET /settings` again.

| Event | When | Payload |
|---|---|---|
| `settings.snapshot` | once, right after a client subscribes to `settings` | `{ "revisions": { "desktop": n, "engine": m } }` |
| `settings.changed` | after a persisted change by one owner | `{ "owner": "desktop" \| "engine", "sections": [...], "revision": n }` |

- **Owners.** `desktop` is the product store of this crate. `engine` is the Engine settings,
  reported by Engine `SettingsChanged`. A mixed `PUT /settings` that changes both produces one
  event per owner.
- **Sections** use the top-level JSON keys of `GET /settings` (`general`, `sync`,
  `retentionPolicy`, `security`, `pairing`, `keyboardShortcuts`, `fileSync`, `network`,
  `quickPanel`). Engine `MobileSync` maps to `mobileSync`, which has no path in `/settings`; a
  client treats it like any other section and reads again. The Engine section mapping is an
  exhaustive match, so a new Engine section fails to compile instead of being dropped.
- **Revisions** are per owner, in daemon memory, and start at 0 with each daemon run. The
  `desktop` revision is the store revision (section 5). The Engine event has no revision, so the
  daemon assigns one: it adds one per mapped Engine `SettingsChanged`. Revisions of different
  owners, or of different daemon runs, are never compared.
- **Sources.** The daemon subscribes to the store before it starts the HTTP server and forwards
  each notification. The Engine event forwarder maps Engine `SettingsChanged`.
- **Lag.**
  - If the store-to-WS forwarder lags, it reads a new snapshot and sends one `settings.changed`
    for `desktop` with every product section and the current revision.
  - If one WS client lags, the existing per-connection rule sends `system.refresh_required`.
  - If the Engine event consumer lags, the Engine sends `RefreshRequired`, which becomes
    `system.refresh_required`.
- **Client protocol.**
  1. Connect and subscribe to `settings` and `system`.
  2. Wait for `settings.snapshot`. Only then is the subscription active. Then read
     `GET /settings` and apply it. The snapshot revisions are the baseline for this connection.
  3. On `settings.changed` with a revision above the baseline of its owner, or on
     `system.refresh_required`, read `GET /settings` again. Events that arrive during a read
     are coalesced into one more read.
  4. On disconnect, reconnect with a bounded backoff and start again at step 1. A new
     connection has no baseline; a daemon restart therefore needs no special case.
- **Apply is idempotent.** An executor compares the new value with the state it already holds
  and does nothing when they are equal. The host's own writes therefore are not applied twice;
  the event is not suppressed by revision, because `PUT /settings` returns no revision and a
  mixed write produces two events.
- **Go host.** The shared Go settings view (`settings.View`, `settings.Read`) and the watcher
  (`settings.Watcher`) live in `packages/desktop-host-go/settings`, which does not import Wails.
  The GUI host reads `GET /settings` only through this view and starts the watcher during
  bootstrap, also when no window exists (silent start). An open WebSocket holds a control lease,
  so the host pauses the watcher (normal close frame, no reconnect) before every daemon stop it
  starts: quit, `RestartDaemon`, the full restart and the update install. It resumes the watcher
  whenever the process keeps running afterwards: after `RestartDaemon` (also when the restart
  failed), when the GUI relaunch of a full restart fails, and when the install or its relaunch
  fails. On those paths the host may not have stored a client for the daemon that now runs (a
  restart reported as failed can still bring a daemon up; a full restart replaces the daemon
  before the GUI relaunch fails). The watcher therefore gets its client through
  `daemonclient.Follow`: it keeps the stored client while `daemon.conn` names the same endpoint
  and token, and otherwise swaps in a fresh client for every caller of the host.
- **Page-originated `settings://changed`.** Until the frontend follows `settings.changed`, the
  page still emits it after its own saves. The host does not use the page's settings copy: it
  reads `GET /settings` and runs the same idempotent apply. The listener and the page's emit are
  removed together when the frontend moves to `settings.changed`.

## Failure modes (listed before implementation)

| # | Failure mode | Expected behaviour | Verification |
|---|---|---|---|
| F1 | File missing | default `glyph`; load does not create the file | library test, daemon E2E |
| F2 | File is not valid JSON | defaults; load does not rewrite the file; the daemon starts | library test, daemon E2E |
| F3 | Root is an array or a scalar | same as F2 | library test |
| F4 | File larger than 1 MiB | same as F2; at most 1 MiB + 1 byte is read | library test |
| F5 | `trayIcon` has an unknown enum value or a wrong type | effective default; the raw value stays on disk | library test, daemon E2E |
| F6 | `general` is not an object | effective default; writing `trayIcon` replaces it with an object | library test |
| F7 | Unknown keys (top level, in `general`, in `import`) | still present after every write | library test, daemon E2E |
| F8 | A write of one field resets other invalid raw values to defaults | not allowed; only the named path changes | library test (P0 has one field, so unknown and invalid values elsewhere in the document prove it) |
| F9 | The write fails (read-only directory) | error; memory, revision and notifications unchanged; old file unchanged | library test |
| F10 | Crash during a write | the disk holds the old file or the new file | same-directory temporary file and `persist` |
| F11 | Same-value write | no file write; revision unchanged; no notification | library test, daemon E2E |
| F12 | Concurrent writes | writes are serialized; the revision grows by the number of real changes; notification revisions strictly increase; the final file value equals the value of the last change; unknown keys are kept | library test |
| F13 | Restart | the written value is kept; the revision starts at 0 | library test, daemon E2E |
| F14 | Higher `version`, existing `import.fieldSet` | kept as they are, never downgraded | library test, daemon E2E |
| F15 | Invalid write (`trayIcon: "bogus"`) | rejected; file and memory unchanged | daemon E2E |
| F16 | A `trayIcon`-only `PUT` changes the Engine `settings.json` | not allowed | daemon E2E (hash before and after) |
| F17 | Logs leak a setting value | warnings carry only the field path and a reason category | daemon E2E (`neon` absent from the log) |
| F18 | The caller is dropped while the write is in flight (cancelled HTTP request) | the write runs on its own task: the file, memory, revision and notification still move together | library test |

Settings change events (task 2):

| # | Failure mode | Expected behaviour | Verification |
|---|---|---|---|
| F19 | A product write emits no event, or more than one | exactly one `settings.changed` with owner `desktop`, sections `["general"]` and revision + 1 | daemon E2E |
| F20 | An Engine write is not reported, or is reported as `desktop` | exactly one `settings.changed` with owner `engine` and the Engine sections; no `desktop` event | daemon E2E |
| F21 | A mixed write merges both owners into one event | one event per owner, each with its own revision | daemon E2E |
| F22 | A same-value write emits an event | no event; a following real write emits one (positive control) | daemon E2E |
| F23 | An invalid value (422) emits an event | no event, then a positive control | daemon E2E |
| F24 | An Engine rejection (400) emits an event | no event, then a positive control | daemon E2E |
| F25 | A failed product file write emits a success event | error response and no `desktop` event; after the directory is writable again a write emits one | daemon E2E |
| F26 | A change between the client's read and its subscription is lost | the client reads only after `settings.snapshot`, which is sent after the subscription is active | daemon E2E (snapshot arrives; a write right after it is delivered), Go E2E |
| F27 | The store-to-WS forwarder lags and a change is lost | it reads a new snapshot and sends one `desktop` event with every section | code review only (forcing 64 missed store notifications needs a stalled forwarder) |
| F28 | A WS client lags, or the Engine consumer lags, and a change is lost | `system.refresh_required`; the client subscribes to `system` and reads again | code review only (the existing per-connection rule and the watcher's handling were not forced) |
| F29 | After a daemon restart the client compares the new revision 0 with the old revision and ignores changes, or keeps stale settings | the client reconnects, waits for the snapshot and reads again; baselines are per connection | daemon E2E (revision restarts at 0), Go E2E (value written while the daemon was down is seen after reconnect) |
| F30 | The payload leaks setting values | the payload has only `owner`, `sections` and `revision` (or `revisions`) | daemon E2E (exact key set; written value absent from every frame) |
| F31 | The host applies its own write twice | executors compare with the state they hold (the tray sync label does nothing when unchanged) | code review |
| F32 | The host watcher keeps the daemon waiting at stop, or stays paused for good | the watcher closes with a normal close frame and stays paused before every host-initiated daemon stop; it resumes whenever the process keeps running | Go E2E (the daemon logs the close right after `Pause`; reconnect after a failed restart and after `Resume`); the host call sites are code review only |
| F33 | Settings reach the host only through a WebView | the watcher is in a package without Wails; an external write is seen with no frontend running | Go E2E (build without Wails; external `PUT`) |
| F34 | After a daemon restart the host did not record, the watcher keeps using the old client | the watcher asks for its client through `daemonclient.Follow` on every attempt | Go E2E (holder never updated by the test; a watcher with the stale client is the negative control and does not recover) |
| F35 | A future Engine section is silently dropped | exhaustive match without a wildcard | compiler |
| F36 | A wrong JSON tag in the shared Go type yields a silent zero value | a real `GET /settings` is decoded into the type and checked against non-default values | Go E2E |
| F37 | The watcher retries without pause while the daemon is gone | bounded exponential backoff | Go E2E (reconnect after restart) and code review |

## Consequences

- An invalid product value no longer affects Engine session startup.
- Later product fields go into the same file. The task 4 importer uses `import.fieldSet` and
  `import.userWritten` to decide what to import.
- `tag_layout` is already product-side and stays in `uc-webserver` for now. Moving it into this
  crate is a separate decision; its file and format would not change.
- Deleting this file on factory reset (D5) and including it in config export (D6) are task 10.
