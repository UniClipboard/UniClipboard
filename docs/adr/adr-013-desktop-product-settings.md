# ADR-013: The daemon owns Desktop product settings

- **Status**: Accepted. Implemented: the store, its daemon wiring and `general.trayIcon` (task 1),
  the `settings` WS topic and the Go host subscription (task 2), the tray executor and settings UI
  (task 3), field set 1 with the legacy import and owner-split writes (task 4, section 8).
  Acceptance B3 is blocked until the Engine drops the product fields (tasks 7 and 10; section
  8.4).
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
- It depends only on `blake3` (import fingerprint, task 4), `serde_json`, `tempfile`, `tokio`
  (`sync`, `rt`) and `tracing`. It does not
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

### 6. `PUT /settings` order (P0, replaced by section 8.3 in task 4)

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
- **Page-originated `settings://changed`.** Removed in task 5 together with the host's listener
  (section 9). The host follows only the daemon's `settings.changed`.

### 8. Field set 1: moved fields, legacy import and owner-split writes (task 4, P1)

P1 moves every field below from the Engine `settings.json` to this store. The JSON paths on
`/settings` do not change. Three fields are not in field set 1:
- `general.debugMode` stays in the Engine until P3.
- `general.telemetryEnabled` keeps its own file (D9).
- `general.trayIcon` was added in P0.

#### 8.1 Fields and grammars

Defaults are the Engine defaults of the pinned Engine (`uc-core/src/settings/defaults.rs`). A
fresh install and a missing legacy field therefore give the same effective value as before P1.
Product file values use the `/settings` wire spelling. Every enum spelling equals the Engine
spelling.

| Product path | Legacy path (`settings.json`) | Default | Grammar |
|---|---|---|---|
| `general.autoStart` | `general.auto_start` | `false` | bool |
| `general.startupMode` | `general.startup_mode` (legacy rule, 8.2) | `normal` | `normal` \| `silent` \| `lightweight` |
| `general.restoreLastEntryOnStartup` | `general.restore_last_entry_on_startup` | `false` | bool |
| `general.autoCheckUpdate` | `general.auto_check_update` | `true` | bool |
| `general.autoDownloadUpdate` | `general.auto_download_update` | `true` | bool |
| `general.updateChannel` | `general.update_channel` | `null` | `null` \| `stable` \| `alpha` \| `beta` \| `rc` |
| `general.theme` | `general.theme` | `system` | `light` \| `dark` \| `system` |
| `general.themeColorLight` | `general.theme_color_light` (fallback, 8.2) | `null` | `null` or a preset name `[a-z0-9-]{1,32}` (D4) |
| `general.themeColorDark` | `general.theme_color_dark` (fallback, 8.2) | `null` | same |
| `general.themeOverridesLight` | `general.theme_overrides_light` | `{}` | object with keys from `primary`, `background`, `foreground`, `border`; each value is `oklch(L C H)` with decimal numbers, L in [0, 1], C in [0, 1], H in [0, 360] (the `hexToOklch` output), or `#rrggbb` |
| `general.themeOverridesDark` | `general.theme_overrides_dark` | `{}` | same |
| `general.language` | `general.language` | `null` | `null` or a language tag: 1–8 letters, then subtags of 1–8 letters or digits, separated by `-` or `_` (the separators that the frontend `normalizeLanguage` accepts); at most 35 characters |
| `general.usageAnalyticsEnabled` | `general.usage_analytics_enabled` | `true` | bool |
| `keyboardShortcuts.<id>` (one field per id) | `keyboard_shortcuts.<id>` | absent (the built-in binding applies) | see below |
| `quickPanel.enabled` | `quick_panel.enabled` | `true` | bool |
| `quickPanel.position` | `quick_panel.position` | `center` | `center` \| `follow_cursor` |
| `quickPanel.doubleTapModifier` | `quick_panel.double_tap_modifier` | `disabled` | `disabled` \| `alt` \| `control` \| `meta` |

Shortcut grammar:
- id: `[a-z][A-Za-z0-9]*(\.[A-Za-z0-9]+)*`, at most 64 characters; at most 64 ids;
- value: one binding string, or an array of 1–4 binding strings;
- binding: 1–2 strokes, separated by one space;
- stroke: 1–8 tokens joined by `+`, each token from the fixed token set in 8.1.1.

##### 8.1.1 Shortcut token set

Tokens compare case-insensitively. The set is fixed in code. It is not the union of every
consumer.

- Modifiers that the three normalizers accept (`apps/gui-go/global_shortcuts.go`,
  `crates/quick-panel-core/src/shortcuts.rs`, `apps/gui-go/frontend/src/shortcuts/normalize.ts`):
  `ctrl control alt option shift meta super cmd command mod`.
- `a`–`z`, `0`–`9`, `f1`–`f24`. The design bounds function keys at `f24`. Wails also accepts
  `f25`–`f35`; they stay outside the set.
- Named keys, from three sources:
  - frontend aliases (`normalize.ts`, `definitions.ts`): `esc equal minus add subtract`;
  - single-word Wails named keys (`wails/v3 pkg/application/keys.go`): `backspace tab return enter
    escape left right up down space delete home end numlock plus`;
  - `global-hotkey` 0.8 `parse_key` names: `backquote backslash bracketleft bracketright pause
    pausebreak comma period quote semicolon slash capslock insert pagedown pageup printscreen
    scrolllock arrowdown arrowleft arrowright arrowup`, `numpad0`–`numpad9`, `num0`–`num9`,
    `numpadadd numadd numpadplus numplus numpaddecimal numdecimal numpaddivide numdivide
    numpadenter numenter numpadequal numequal numpadmultiply nummultiply numpadsubtract
    numsubtract audiovolumedown volumedown audiovolumeup volumeup audiovolumemute volumemute
    mediaplay mediapause mediaplaypause mediastop mediatracknext mediatrackprev
    mediatrackprevious`.
- One printable ASCII character that is not a letter, digit, space or `+` (for example `,` `;`
  `/` `[`). Both Wails and `global-hotkey` accept single printable ASCII characters.

Two-word names (`page up`, `forward delete`) are not in the set, because a space separates
strokes. `pageup` is in the set.

Some stored values are outside the set:
- values recorded before #1986, for example `alt+meta+√`;
- values recorded on layouts whose key is not a `KeyX` or `DigitN` code, for example `ctrl+ä`.

Today no consumer acts on these values, but they hide the built-in binding. After P1 such an
override is not effective, so the built-in binding applies. This is a behaviour change for those
users. The raw value stays in the product file (8.2) until that shortcut is written.

#### 8.2 Legacy import (bootstrap)

`uc-bootstrap` runs the import at the start of `prepare_desktop_engine_host`. This function is the
only path to `Engine::start`, for both the daemon and `uc-dev-cli`. The import runs after tracing
is installed and before any Engine settings load. The daemon loads the store after the import.

1. Read the product file.
   - Missing: use an empty document.
   - Unreadable, larger than 1 MiB, malformed JSON or a root that is not an object: skip the
     import. The file stays as it is until an explicit write replaces it (section 3); the import
     runs on the first start after that.
2. Read `stored = import.fieldSet` (missing = 0). The binary's field set is `N = 1`.
3. Read `settings.json` as raw JSON, with the reader rules of section 3. Log the BLAKE3 hash of the
   bytes read. Never log values.
   - Missing file: if `stored < N`, write `fieldSet = N` with the fingerprint of the empty
     legacy set (every path at its default) and stop. Otherwise stop. When the Engine later writes
     its defaults, the normalized set is the same, so no import follows.
   - Unreadable, too large, invalid JSON, or root not an object: log the reason category and
     stop. The marker does not advance, so the next start tries again.
4. Derive the legacy value of each field-set-1 path:
   - `startupMode`: `startup_mode` if the key exists; else `lightweight` if `lightweight_start`
     is `true`; else `silent` if `silent_start` is `true`; else absent.
   - `themeColorLight` and `themeColorDark`: the split value if it is not null; else
     `theme_color` if it is not null; else the split value as stored (null or absent).
   - every other path: the raw value at the legacy path, or absent.
5. Normalize. Each derived value that matches its grammar is kept; any other value becomes the
   default. Each shortcut id whose value matches is kept; any other id is dropped. The
   fingerprint is the BLAKE3 hash of this normalized set, serialized with sorted keys.
   - The raw JSON is never hashed. The Engine rewrites the file after it loads it (v2→v3 startup
     migration, default insertion, formatting). The normalized set does not change with that
     rewrite.
6. Decide:
   - `stored < N`: import.
   - `stored >= N` and the fingerprint differs from `import.sourceFingerprint`: import again
     (D8). This covers edits that an older binary made after a downgrade, and a config import.
   - otherwise: do nothing and do not write.
7. An import copies the **derived raw value** of every path that is not in `import.userWritten`.
   An absent value removes the product key.
   - An invalid value is therefore kept in the product file, and the effective value is the
     default (section 3). The log names the path, never the value.
   - Paths in `userWritten` keep the product value. If a repeated import finds a different legacy
     value for such a path, a warning names the path.
8. Write the product file atomically, once, with the values, `fieldSet = N` and
   `sourceFingerprint`. A crash before this write leaves both files unchanged, so the next start
   repeats the same import.
9. The import never writes `settings.json`.

After the import, bootstrap sets the analytics gate from the effective
`general.usageAnalyticsEnabled`. The gate no longer reads the Engine file.

`uc-dev-cli` does not hold the profile instance lock. Running it while a daemon serves the same
profile is already unsupported. The import is idempotent, so two concurrent imports write the
same content.

#### 8.3 Owner-split `PUT /settings`

This section replaces section 6.

1. Decode the body. A body that does not decode returns `400 bad_request`; the extractor
   rejection is mapped as in `history_tags`. This applies to every field, not only to product
   fields. It replaces the earlier `422`, so the endpoint follows the approved contract
   ("validation error: 400").
2. Split the patch by owner:
   - product fields (8.1 and `trayIcon`) go to the store;
   - `telemetryEnabled` goes to the telemetry file;
   - all other fields (`deviceName`, `debugMode`, `sync`, `retentionPolicy`, `security`,
     `pairing`, `fileSync`, `network`) go to the Engine.
   The Engine patch never carries a product field, so there is no dual write.
3. Validate the product part with the grammars of 8.1. An invalid value returns
   `400 bad_request` with the field path; nothing is written.
4. Send the Engine part, only if it is not empty. An Engine rejection returns `400`. Other Engine
   errors keep their current mapping. In both cases nothing else is written.
5. Write the product part, only if it is not empty. Then update the analytics gate, if the patch
   has `usageAnalyticsEnabled`.
6. Write the telemetry part, only if it is present.
7. The first failure in step 5 or 6 stops the sequence:
   - If no part was written before the failure, the error is `500 internal_error`.
   - Otherwise the error is `500 settings_partially_applied`, with
     `details = { applied: [...], failed: [...], skipped: [...] }`. The owners are `engine`,
     `desktop` and `telemetry`. `applied` lists the owners whose part was written. `failed` lists
     the owner whose write failed. `skipped` lists the owners in the patch that were not tried.
     An owner with an empty part is in no list.

The details type is `SettingsPartiallyAppliedDetailsDto` in `uc-daemon-contract`. It is in the
OpenAPI document and the generated TS client. Task 5 shows the error in the UI.

`PUT /settings/relay` uses the same patch DTO for its Engine save. A product or telemetry field in
that patch returns `400 bad_request` and never reaches the Engine.

Not changed in task 4 (task 5 handles them):
- `null` still means "no change" for `language`, `updateChannel` and the theme presets (the #606
  wire rule).
- `restartRequired` is still `true` when the patch names `network` or `debugMode`. The frontend
  saves the whole `general` section, so it still receives `true`.

`general.themeColor`:
- `GET` returns `null`, because the import folds the value into the split presets.
- A non-null `themeColor` in a `PUT` is folded the same way. It sets `themeColorLight` and
  `themeColorDark` if the patch does not name them and their effective value is `null`. Only a
  raw client sends a non-null value; the frontend sends `null`.

#### 8.4 P1 limitations

- **Downgrade.** A pre-P1 binary reads the frozen Engine values, so it does not show edits made
  under P1. Edits made in the older binary are imported on the next P1 start, for paths not in
  `userWritten` (8.2 step 6).
- **Config export.** The bundle still carries the Engine `settings.json` with frozen values, and
  does not carry the product file (tasks 8 and 10).
  - Fresh machine: the import uses the frozen values from the bundle.
  - Existing installation: a config import changes the fingerprint, so the next start imports
    the paths not in `userWritten`. Written paths keep the local value. The result is a partial
    restore, not the full restore that pre-P1 versions give.
- **Factory reset.** The Engine deletes `settings.json`. The product file stays until task 10
  (D5). The Engine loads defaults without saving them when the file is missing, so the
  preferences survive until the first Engine settings save. On the start after that save the
  fingerprint differs, so paths not in `userWritten` return to their defaults; written paths
  keep their values. Not verified by a run.
- **Whole-section saves defeated the `userWritten` rule before task 5.** The frontend saved the
  whole `general` section on every General change, so one GUI save put every general path into
  `userWritten`. Task 5 sends diff patches (section 9). Task 4 must not ship without task 5.
- **Recorder output outside the token set is rejected on write.** The recorder can still produce
  tokens outside 8.1.1 (non-ASCII characters, `dead`, `contextmenu`, `altgraph`). Saving such a
  value now returns 400, so the user sees a save error instead of an inert binding (task 5/6).
- **An unusable product file serves defaults.** When the product file is malformed, the import
  is deferred and every moved field is effective at its default, including
  `usageAnalyticsEnabled` (the gate is set to `true`) and `autoStart` (the Go host's start-up
  reconcile then removes an existing login item). This is the section 3 rule applied to more
  fields; it needs a decision before release.
- **The Engine still parses the frozen fields** until P2 removes them. The import keeps an
  invalid raw value and uses the default, but it cannot change what the Engine does with the
  same value in `settings.json` (acceptance B3 records the actual outcome).
- **Release order.** The Engine removal (tasks 7–9) requires that this importer has shipped (P1
  before P2 and P3). The importer must stay in every release until the oldest supported upgrade
  source already has field set 1.

### 9. Frontend authority and diff patches (task 5, P1)

**Diff patches.** Every GUI save sends only the leaves the user changed, at the server's merge
granularity:

| Path | Leaf |
|---|---|
| scalar fields (`general.*`, `sync.*`, `security.*`, `fileSync.*`, `network.*`, `quickPanel.*`) | the field |
| `sync.contentTypes` | each boolean |
| `general.themeOverridesLight` / `themeOverridesDark` | the whole map (the server replaces it) |
| `retentionPolicy.rules` | the whole list (the server replaces it) |
| `keyboardShortcuts` | each id; `null` removes the override |

- The leaves come from the caller's intended change, not from comparing a whole section with a
  cached copy. A control that holds a whole object (the retention policy) sends the leaves that
  differ from the value it displayed.
- `false`, `0` and `""` are values. `fileSync.autoSaveDir: ""` still clears the directory.
- `null` on `language`, `updateChannel`, `deviceName` and `themeColor*` stays a wire no-op (the
  #606 two-state rule). The frontend sends what the control produces and does not reinterpret it.
- Go host writers (tray sync, updater `autoDownloadUpdate`, `autoStart`, `deviceName`, quick panel,
  shortcuts) already send single leaves.
- `restartRequired` is true only when the patch carries `network` or `general.debugMode`. A tray,
  theme or other General change no longer carries them. The GUI changes `debugMode` through
  `PUT /diagnostics/debug`, not through this patch.

**One authoritative copy per window.** `src/lib/settings-sync.ts` holds each document's settings:
- It subscribes to the `settings` and `system` topics. Every `settings.snapshot` starts a new
  epoch and causes a read; so do `settings.changed`, `system.refresh_required` and a reconnect.
  Revisions are not compared across snapshots.
- Reads and the window's own writes run one at a time in one queue, so a read result never
  replaces a newer one. Requests that arrive during a read coalesce into one more read.
- The main window's `SettingContext`, the theme of the other windows (`useThemeSync`) and the
  quick panel's shortcut overrides read from it. First-paint caches (the stored UI language and
  the telemetry mirror) are written from the authoritative copy only.
- The page-originated Wails event `settings://changed` and the host's listener are removed, and
  `settings://sync-changed` with them: the tray's sync write reaches every window as an Engine
  `settings.changed`. `scripts/architecture/check-host-events.mjs` rejects both names.

**Save errors.** After any failed save (daemon error, 400, a host command error, a network
failure) the window reads `GET /settings` before the error reaches the control, so the control
shows the stored state. `500 settings_partially_applied` becomes a typed error with the
`applied`, `failed` and `skipped` owners; the control shows them in a toast. The window's load
error state is not used for save errors.

**Verification.** `apps/gui-go/frontend/e2e/settings-sync-api.mjs` runs these modules in Node against an
isolated real daemon (cases C1–C11 map to F64–F77). It records every HTTP request and WS frame.

**Tray language (B9).** The Go host applies `general.language` from the daemon in `applySettings`, so
the tray follows the store without a WebView. It uses the rule the main window and the quick panel
already use: the configured language when it is not empty, otherwise the system language (macOS: the
first of the preferred languages; Windows: the first preferred UI language; other systems: `LC_ALL`,
`LC_MESSAGES`, `LANG`), then the subtag mapping of `normalizeTrayLanguage`. The page no longer sets the
tray language; the host command `SetTrayLanguage` is removed.

**Host daemon stops.** Every window's daemon socket and the host watcher hold a control lease, so every
host path that stops the daemon announces the stop first (`announceDaemonStop`: watcher paused,
`app://shutting-down`, 300 ms grace). A path after which the process keeps running ends with
`resumeAfterStop`: when a compatible daemon answers, the host stores its client and emits
`app://daemon-connection-changed`, then resumes the watcher.

| Path (`apps/gui-go`) | Reached from | Before the stop | When this process keeps running |
|---|---|---|---|
| `shutdown` → `stopDaemonOnExit` (`lifecycle.go`) | tray Quit, Cmd-Q | announce (skipped when the exit keeps the daemon) | none: the process exits |
| `RestartDaemon` (`host_commands_native.go`) | network, diagnostics, factory reset, config import, startup recovery | announce | `resumeAfterStop`, success or failure |
| `fullRestart` (`lifecycle.go`) | tray Restart, `RestartApp` | announce | `resumeAfterStop` when the GUI relaunch fails |
| `installUpdate` → `installPayload` (`host_updater.go`) | updater window | announce | install failed: `resumeAfterStop` that also starts a daemon when none runs; relaunch failed: `resumeAfterStop` without a start (the installed daemon is newer than this GUI) |
| `PromoteOneshot` (bootstrap, `main.go`) | GUI start while a CLI one-shot daemon runs | none needed: no window has connected yet | bootstrap continues |

**Not changed by task 5.**
- (Superseded below: the tray language moved to the Go host.)
- Shortcut apply status for external writes is task 6.

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
| F23 | An invalid value (422 until task 4, 400 after section 8.3) emits an event | no event, then a positive control | daemon E2E |
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

Field set 1, legacy import and owner-split writes (task 4):

| # | Failure mode | Expected behaviour | Verification |
|---|---|---|---|
| F38 | The import runs after an Engine settings load, so the Engine has already rewritten or dropped values | the import runs at the start of `prepare_desktop_engine_host`; the hash of the bytes it read equals the hash of the profile's `settings.json` taken before start, and its log line comes before the Engine start | daemon E2E (hash in log against hash before start; log order) |
| F39 | `uc-dev-cli` loads Engine settings without the import | it uses the same `prepare_desktop_engine_host` | code review only (one call path; not run) |
| F40 | The import writes `settings.json` | never; the importer has no write path to it | code review; daemon E2E (hash read by the importer equals the hash before start) |
| F41 | File existence is used as the marker, so a P0 file (`trayIcon`) skips the import | the marker is `import.fieldSet`; `trayIcon` stays, field set 1 is imported | daemon E2E (B1a) |
| F42 | The marker advances on unreadable or invalid legacy JSON, so values are lost for good | no write, marker unchanged; the next start with a valid file imports | daemon E2E (malformed `settings.json`, then repaired) |
| F43 | A missing legacy file blocks the marker forever | `fieldSet = 1` is written with no values | daemon E2E |
| F44 | The marker and the values are written separately, so a crash leaves the marker without values | one atomic write holds both; a failed write leaves both files unchanged and the next start imports | daemon E2E: an unusable product path defers the import and the next start imports; a kill right after the product file appears keeps one consistent result. Not covered by E2E: the persist-failure branch itself (any path that fails the rename fails the read first) and a deterministic kill between the temporary file and the rename (code review of the single atomic write) |
| F45 | Engine's rewrite on the second start (v2→v3 migration, default insertion, formatting) changes the fingerprint and triggers a false re-import | the fingerprint covers normalized values only; second and third starts do not write the product file | daemon E2E (B1b: product file hash unchanged, no import log) |
| F46 | A re-import overwrites values the user wrote under P1 | paths in `userWritten` keep the product value, with a warning that names them | daemon E2E (B4, B1c) |
| F47 | Edits made in an older binary after a downgrade are ignored | the fingerprint differs; paths not in `userWritten` are imported again; the log names them | daemon E2E (B4 with a v1.2.0 binary) |
| F48 | An invalid legacy value is lost, or it stops startup, or it reaches the effective value | the raw value is copied into the product file; the effective value is the default; the warning names the path, never the value | daemon E2E for values the Engine does not type (shortcuts, presets, overrides, language). For an Engine-typed enum (`theme: "neon"`) the import behaves as stated, but the pinned Engine session does not start: B3 fails until tasks 7 and 10 (a valid-enum control with otherwise identical input starts) |
| F49 | Legacy rules differ from the design: startup precedence or `theme_color` fallback | `startup_mode`, else `lightweight_start`, else `silent_start`; `theme_color` fills a split preset that is null or absent | daemon E2E (v2-shaped input) |
| F50 | Product defaults differ from Engine defaults, so a fresh install changes behaviour | defaults equal `uc-core` defaults | daemon E2E (missing legacy file: GET equals the pre-P1 defaults) |
| F51 | A product field still reaches the Engine (dual write), through `PUT /settings` or `PUT /settings/relay` | the Engine patch never carries a product field; `/settings/relay` rejects them with 400 | daemon E2E (product-only `PUT` of every moved field leaves `settings.json` byte-identical; relay with a product field is 400) |
| F52 | `GET /settings` shows the frozen Engine value instead of the product value | every moved path is read from the store | daemon E2E (write product value, compare GET with the frozen Engine value) |
| F53 | An invalid product value is written to the Engine before validation fails | validation runs before any write; 400; no file changes | daemon E2E (mixed patch with an invalid shortcut token: `settings.json` and product file unchanged) |
| F54 | A malformed body returns 422, unlike the approved 400 | 400 `bad_request` | daemon E2E |
| F55 | Engine succeeds, product write fails, and the client sees a generic error or a wrong owner list | `500 settings_partially_applied`, `applied: [engine]`, `failed: [desktop]`, `skipped: [telemetry]` when telemetry was in the patch | daemon E2E (B8: product file replaced by a directory) |
| F56 | Engine and product succeed, telemetry fails, and the response hides the product write | `applied: [engine, desktop]`, `failed: [telemetry]` | daemon E2E (telemetry file replaced by a directory) |
| F57 | A failure with nothing applied is reported as partial | `500 internal_error` | daemon E2E (product-only patch with the product file replaced by a directory) |
| F58 | An empty Engine part still calls the Engine | no Engine call; no Engine event; `settings.json` unchanged | daemon E2E |
| F59 | The analytics gate reads the frozen Engine value at the next start and reverts consent | the gate is set from the product value after the import | daemon E2E (set `usageAnalyticsEnabled=false`, restart, GET; bootstrap log names the source) |
| F60 | Concurrent partial writes to different owners lose a field | the store merges per path; the Engine merges per field | daemon E2E (B7 backend: concurrent sync, `autoDownloadUpdate` and theme writes) |
| F61 | An out-of-set shortcut token (`f25`, `ctrl+ä`, `alt+meta+√`, `page up`) is accepted on write, or its raw legacy value is lost on import | write: 400; import: raw kept, effective entry absent (built-in binding), warning names the id | daemon E2E |
| F62 | A new product section is not reported, or is reported under the wrong key | `SettingsSection::ALL` has `general`, `keyboardShortcuts`, `quickPanel`; the mapping to `SettingsSectionDto` has no wildcard | compiler; daemon E2E (one event per write with the right sections) |
| F63 | Logs leak a setting value | the import and the handler log paths, ids, reason categories and hashes only | daemon E2E (a unique non-default value is absent from the log) |
| F64 | A tray, theme or other General save carries `debugMode` or `network` and reports a restart | the body holds only the changed leaf; `restartRequired: false` | frontend-module E2E (wire body and response) |
| F65 | A real `debugMode` or network change loses the restart indication | `restartRequired: true` | frontend-module E2E |
| F66 | A save from a stale cached section overwrites a concurrent edit of another field | only changed leaves are sent; every field survives | frontend-module E2E (B7: concurrent saves from two windows and the Go writers' single-leaf bodies) |
| F67 | `false`, `0` or `""` is dropped as "no change" | the leaf is sent | frontend-module E2E (`autoSaveDir: ""`, `syncEnabled: false`) |
| F68 | A read issued before a write returns after it and replaces the newer state | reads and writes run in one queue; a later read wins | frontend-module E2E (a held GET) |
| F69 | Revisions are compared across daemon lifetimes, so a restarted daemon's lower revision is ignored | every snapshot starts a new epoch and reads | frontend-module E2E (restart the daemon after an offline file edit) |
| F70 | A change from another client never reaches an open window | `settings.changed` causes a read | frontend-module E2E (external PUT) |
| F71 | A missed event while the socket was down leaves the window stale | the reconnect snapshot causes a read | frontend-module E2E |
| F72 | A failed save leaves the control showing the unsaved value or a value the server rolled forward | the window reads before the error is reported | frontend-module E2E (400 and partial failure, GET after the PUT in the wire log) |
| F73 | A partial failure is shown as "nothing saved" | typed error with `applied`/`failed`/`skipped` | frontend-module E2E (B8, product file replaced by a directory) |
| F74 | A save error replaces a settings section with the load-error view | save errors are toasts; the load error is set only by a failed read | code review |
| F75 | A shortcut save resubmits another id's raw invalid value | the patch holds the changed id only; the raw value stays in the file | frontend-module E2E |
| F76 | A recorder value outside the token set fails silently | the 400 reaches the caller and a read follows | frontend-module E2E |
| F77 | The removed page event comes back | the host-event check fails on `settings://changed` and `settings://sync-changed` | `bun run check:host-structure` |
| F78 | A first-paint cache (language, telemetry mirror) wins over the daemon | written from the authoritative copy only | code review |
| F79 | The tray keeps the language of the last page run, or English, in a silent start | the host applies the stored language without a WebView | Go watcher E2E decodes `general.language`; native check pending |
| F80 | A null or empty language shows English instead of the system language | the system language through the same subtag mapping | code review against `apps/quick-panel/src/ui/mod.rs` `apply_language` |
| F81 | Every settings read rebuilds the tray menu | an unchanged language does nothing | code review |
| F82 | A window socket delays the daemon stop of an update install | the stop is announced first; windows close their sockets | code review; native check pending |
| F83 | A failed install leaves the windows and the host without a daemon | the daemon is started again and every client reconnects | code review; native check pending |
| F85 | A restart path stops the daemon while window sockets are open, or leaves the windows disconnected after a failure | every stop path in the table above announces; every surviving path ends with `resumeAfterStop` | code review; native check pending |
| F84 | The page still sets the tray language and races the host | the page call and the host command are removed | `bun run check:host-structure`, bindings `--check` |

## Consequences

- An invalid product value no longer affects Engine session startup.
- Later product fields go into the same file. The task 4 importer uses `import.fieldSet` and
  `import.userWritten` to decide what to import.
- `tag_layout` is already product-side and stays in `uc-webserver` for now. Moving it into this
  crate is a separate decision; its file and format would not change.
- Deleting this file on factory reset (D5) and including it in config export (D6) are task 10.
