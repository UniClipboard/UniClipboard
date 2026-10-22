# ADR-013: The daemon owns Desktop product settings

- **Status**: Accepted. Implemented: the store, its daemon wiring and `general.trayIcon` (task 1),
  the `settings` WS topic and the Go host subscription (task 2), the tray executor and settings UI
  (task 3), field set 1 with the legacy import and owner-split writes (task 4, section 8),
  frontend diff patches and daemon-followed settings (task 5, section 9), and the global shortcut
  apply status (task 6, section 10; native OS registration checks pending).
  P3 adoption removes the remaining Engine product projection and imports field set 2
  (task 10; section 11). Native OS registration checks remain separate.
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

### 主窗口尺寸偏好

`general.mainWindowSize` 是三端共用的 Desktop 偏好，由本 ADR 的单写入者、逐字段校验、原子存储和设置事件机制管理，不进入 Engine。其值为 `{ "width": 1100, "height": 720 }`，单位为逻辑像素；默认宽高分别为 1100、720，两个维度必须同时存在，且均为 `1..=i32::MAX` 的整数。没有旧 Engine 字段，不增加导入字段集。无效存量值只对本字段回退到默认值，不阻塞其他设置加载。

Go 宿主通过 `GET /settings` 读取偏好，运行时采集普通窗口尺寸，在 daemon 停止前或 GUI 进入轻量模式时通过 `PUT /settings` 一并写入宽高。最小化、最大化和全屏尺寸不覆盖普通尺寸，未创建主窗口的静默启动不写该字段。始终自动记住，不增加开关；macOS、Windows 和 Linux 执行相同流程。GUI 不打开 `desktop-settings.json`，不创建独立的窗口设置文件。

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
- Shortcut apply status for external writes is task 6 (section 10).

### 10. Global shortcut apply status (task 6, P1)

Scope: the quick panel's global shortcut (`keyboardShortcuts["global.toggleQuickPanel"]`), the
`quickPanel.enabled` switch and the modifier double tap. In-app shortcuts are matched in the page
and have no OS side effect.

**Diagnosis of the v1.2 Windows report** ("changing the trigger shortcut has no effect; Ctrl+Alt+V
still works"). The executor code is the same in v1.2.0 (`4438ff72b`) and at the task 5 baseline.
On Windows the native helper is the executor (`nativePanelDefault`, and `build.yml` ships
`uniclip-quick-panel.exe`). The GUI save runs:

1. `ShortcutRow.handleConfirm` closes the recorder and calls `useShortcutSettings.persist`.
2. The host command `UpdateKeyboardShortcuts` runs `probeHelperShortcuts` (Windows only). The probe
   registers the new binding through Wails `GlobalShortcut` in the host process and releases it.
3. Only when the probe succeeds: `PUT /settings`, then a helper restart.

Evidence boundary. Proven and not proven are kept apart:

- **Proven (real parsers, source reading):** the probe parses with Wails `parseAccelerator`; the
  helper parses with `global-hotkey`. Running both real parsers shows that Wails rejects
  `arrowup`, `pageup`, `insert`, `esc`, `equal`, `minus`, `comma`, `quote`, `semicolon`,
  `period`, `slash`, `backquote`, `bracket*`, `backslash`, `numpad*`, `capslock`, `printscreen`
  and the media keys, all of which `global-hotkey` accepts. The recorder turns the Space key into
  `ctrl+alt` (the space is the stroke separator), which both parsers reject.
- **Proven (source reading):** `useShortcutSettings.persist` catches the command error and only
  logs it. The recorder is already closed, and the page reloads the stored value. A refused save
  shows no error, and the old binding stays.
- **Not proven:** which value the reporting user recorded, whether their save was refused or
  saved, which executor ran on their machine (the helper, or the WebView panel when the helper is
  missing), and whether `RegisterHotKey` failed (for example `Win+V`, which Windows reserves for
  clipboard history). No log, settings file or Windows run from that user is available.

Candidate chains that end with "the default still works", none confirmed for the user:

1. The probe refuses the value (Wails parser or `RegisterHotKey`); the UI hides the error;
   nothing is saved.
2. The value is saved, the helper restarts, registers the platform default at start, and then
   fails to register the configured value (its only signal is a message inside the hidden panel).
3. The helper's first settings read fails; it keeps the default until the panel is shown.

The parser difference alone does not prove chain 1 for this user. macOS has no probe, so the same
values are saved there; this is consistent with a Windows-only report but is not proof.
Confirming the root cause needs the user's stored `global.toggleQuickPanel` value and the GUI and
helper logs from the failing save.

Storage is not on any of these chains. The task 4 store does not change any of these steps.

**Ownership.** The Go host owns the desired binding and the active binding of the running
executor, in memory only. `packages/desktop-host-go/globalshortcut` holds this state without a GUI
dependency. The executors:

| Executor | Platforms | Applies |
|---|---|---|
| Native helper (`uniclip-quick-panel`) | macOS, Windows (default) | `set_shortcuts` over the helper's standard input; the helper answers with the bindings it holds |
| WebView panel (Wails `GlobalShortcut`) | Linux, or when the helper is missing | `updateShortcuts` in `global_shortcuts.go` |
| none | lightweight mode (the GUI exits) | nothing; no status |

Both executors are transactional: on a failure they keep the bindings they held before. Every
apply returns the bindings the executor reports it holds afterwards, or "unknown" when it cannot
say (no answer). The host records that report as the active binding; it never sets active to the
desired binding without one.

**GUI save** (`UpdateKeyboardShortcuts`, `SetQuickPanelEnabled`):
1. Compute the target (the effective binding while the panel is enabled, none otherwise).
2. If it differs from the desired binding, apply it in the executor. On failure return `Conflict`;
   nothing is saved.
3. Persist. On failure apply the previous binding again and return the error. Desired stays the
   previous (still stored) binding; active is what the rollback reports. When the rollback is
   refused or unanswered, the status says so (`not_active` or `unknown`); it does not claim the
   old binding is back.
4. Record desired = target and active = the executor's report before the lock is released.
5. An unknown result in step 2 is a failure: nothing is saved, the status is `unknown`.

The Windows probe is removed: the executor that will hold the binding is the check.

**External write** (CLI, another client, a silent start without a WebView): the settings watcher
calls `applySettings` after every read. The reconcile step:
- does nothing when the target equals the desired binding (the host's own write, a reconnect, a
  snapshot, an unrelated event);
- otherwise reads `GET /settings` again under the lock, so a read that started before the host's
  own write cannot reapply an old value;
- applies the target. On success: active. On failure: the target stays saved and desired, the
  previous binding stays active, and the status is `not_active` with the OS error. The saved
  value is never reverted.

**Retry.** A `not_active` binding is retried only when the desired binding changes or the helper
starts again. Unrelated events do not retry it, so a reconnect cannot produce a false conflict
against a binding the host already holds.

**Helper protocol.** One JSON object per line, in both directions, next to the existing requests.
- Host to helper (standard input): `{"command":"set_shortcuts","seq":N,"shortcuts":["ctrl+alt+b"]}`
  and `{"command":"reload_settings"}`. The end of input still means exit.
- Helper to host (standard output): `{"shortcuts":{"seq":N,"active":[...],"error":"..."}}`.
- A supervised helper registers nothing until the first `set_shortcuts` and does not apply the
  shortcut from its own settings reads. An unsupervised helper (development) keeps the old
  behaviour.
- After every launch (first start, crash restart, enable) the host sends the desired binding.
  When it fails, the host sends the last binding an executor confirmed holding, so a crash
  does not lose a working binding.
- A shortcut or double-tap change no longer restarts the helper: `set_shortcuts` and
  `reload_settings` apply it in the running process. Own saves cause zero helper restarts.
- Every reply carries `active`, the bindings the helper holds after the command, also after a
  refusal. The host takes `active` from the reply, not from the request.
- A reply that does not arrive within 3 s makes the result **unknown**: the helper may have
  applied the command, refused it, or not read it. The host records active as unknown and does
  not report a rollback. A reply whose `seq` is not the one awaited (a late answer to a timed-out
  command) is dropped; it cannot be matched to the current request.
- Convergence: while active is unknown, the next settings read, GUI save or helper launch sends
  `set_shortcuts` with the desired binding again. Replacing a binding with the same binding
  registers nothing, so the retry is safe; its reply gives the actual state.

**Double tap and the enabled switch.** The reconcile step also follows `quickPanel.enabled` and
`quickPanel.doubleTapModifier`. Each is compared with the value the host last applied, so the
host's own write does nothing a second time. The helper reloads its settings; the WebView panel
sets its modifier monitor.

**Executor key names.** The approved token set (section 8.1.1) is unchanged. The WebView executor
maps the set's named keys to Wails names (`arrowup` → `up`, `pageup` → `page up`, `esc` →
`escape`, `equal` → `=`, `comma` → `,` and the other punctuation names). A token that the
executor or the OS still refuses (`insert`, `numpad*`, the media keys on the WebView panel) is a
visible failure, not a silent one.

**Recorder.** The recorder stores physical key names from `KeyboardEvent.code` (`comma`, `quote`,
`semicolon`, `space`, `arrowup`), as decided after task 4. A key with no name in the set (a
non-Latin character without a physical name, a dead key, `F25` and above) is shown as unsupported
and cannot be saved. The recorder stays open after a failed save and shows the error.

**Status in the UI.** The host command `GetQuickPanelShortcutStatus` returns
`{shortcutId, state, desired, active, error}` with state `active`, `not_active`, `unknown`
(the executor's state could not be read; `active` is empty) or `inactive` (no executor, or the
panel is disabled). The host emits `quick-panel://shortcut-status` with the
same payload on every change. The shortcut row shows "saved, not active" with the active binding
and the error.

**Verification.**
- `packages/desktop-host-go/globalshortcut/applier_e2e_test.go`: the applier and the settings
  watcher against an isolated real daemon, with a counting executor.
- `packages/desktop-host-go/quickpanelhelper/protocol_e2e_test.go`: the real supervisor and
  process launcher against a stand-in helper process (`testdata/standin`).
- `apps/gui-go/frontend/e2e/shortcut-recorder-api.mjs`: the production key mapping
  (`src/shortcuts/record.ts`) against the daemon's token set.

The executors in these checks are test doubles: they prove the sequence of requests, events and
executor calls, not an OS registration. The real helper binary was not run. OS registration on
macOS, Windows (helper and WebView) and Linux X11 is a native user check.

**Open question (not changed).** The section 8.1.1 grammar accepts a stroke made only of
modifiers (`ctrl+alt`, which the v1.2 recorder produced for Ctrl+Alt+Space). Both executors refuse
it, so it shows as a refused GUI save or "saved, not active". Rejecting it in the grammar would
narrow the approved set and needs a decision.

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

Global shortcut apply status (task 6):

| # | Failure mode | Expected behaviour | Verification |
|---|---|---|---|
| F86 | The host's own save registers again when its own `settings.changed` arrives (Windows: a false "not active") | the reconcile step compares with the recorded desired binding and does nothing | globalshortcut E2E (one executor call per own save) |
| F87 | A watcher read that started before the host's own write is applied after it and reapplies the old binding | a differing view is read again under the lock before anything is applied | globalshortcut E2E (stale view injected after the own save) |
| F88 | A GUI save with a conflicting binding is persisted | the executor applies first; on failure nothing is written | globalshortcut E2E (file hash unchanged); native check |
| F89 | The save fails after the executor applied the new binding | the previous binding is applied again; the error is returned | globalshortcut E2E (daemon rejects with 400) |
| F90 | An external conflicting write is reverted, or the old binding is dropped | the value stays saved; status `not_active`; the previous binding stays active | globalshortcut E2E; native check |
| F91 | An external write is not applied without a WebView | the host watcher applies it | globalshortcut E2E without any page; native silent-start check |
| F92 | A reconnect or snapshot read registers the active binding again | equal target: nothing happens | globalshortcut E2E (daemon restart, no executor call) |
| F93 | Every unrelated event retries a failed binding | retried only when the desired binding changes or the helper starts | globalshortcut E2E (tray write after a failure: no executor call) |
| F94 | A helper restart drops the old binding or activates the platform default | no restart for shortcut changes; the helper replaces bindings in its process, add before remove | code review; native check |
| F95 | A helper crash loses the binding or leaves a stale status | after every launch the host sends the desired binding, then the last active one on failure | helper protocol E2E (stand-in helper relaunched) |
| F96 | A hung or silent helper blocks the save | 3 s timeout; the GUI save fails without writing; an external write is `not_active` | helper protocol E2E (stand-in helper that never answers) |
| F97 | The helper's own settings read and the host both change the shortcut | a supervised helper takes the shortcut only from the host | code review |
| F98 | A pre-save check parses differently from the executor | the probe is removed; the executor itself is the check | parser matrix (real Wails and `global-hotkey` parsers) |
| F99 | The WebView executor rejects named keys of the approved set | mapped to Wails names; remaining refusals are visible failures | parser matrix |
| F100 | The recorder stores an out-of-set value (Unicode, dead key, `f25`, Space as `ctrl+alt`) | code-based names; unsupported keys are shown and cannot be saved | code review; native check |
| F101 | A failed save is silent and the recorder closes | the recorder stays open and shows the error | code review; native check |
| F102 | An external change of `quickPanel.enabled` or `doubleTapModifier` is never applied | the reconcile step applies it once | globalshortcut E2E |
| F103 | A double-tap change restarts the helper, or restarts it twice | `reload_settings`, compared with the last applied value | code review; helper protocol E2E |
| F104 | Concurrent GUI save and external write leave the executor and the store on different values | both run under one lock; the later write wins in both | globalshortcut E2E |
| F105 | The helper is not running (disabled, gave up) when the GUI saves | GUI save fails without writing; an external write is `not_active` until the helper starts | helper protocol E2E |
| F106 | Lightweight mode expects a status | no executor, state `inactive` | code review |
| F107 | The helper applied `set_shortcuts` but its reply is lost or later than 3 s; the host reports the old binding as active, or a rollback as done | state `unknown`, active unknown; a late reply with an old `seq` is dropped; the next read, save or launch sends the desired binding again and takes active from that reply | helper protocol E2E (stand-in helper that applies and answers late; then a retry) |
| F109 | The startup read fails (a slow cold start), the helper starts with the default binding, and the first watcher read only records the saved binding: the running helper is never told it, and later reads find nothing to do | the enabled switch is compared with the value actually applied at launch, not with "unknown"; when it is unchanged the binding goes through the normal reconcile step and is applied once | code review (`reconcilePanel` is in `package main`); native cold-start check |
| F108 | The save fails and putting the old binding back also fails; the host records desired = active anyway | desired stays the stored binding; active is the executor's report or unknown; state `not_active` or `unknown` with the error | globalshortcut E2E (executor refuses the rollback) |

Native acceptance of the global shortcut (real helper, real OS registration):

| # | Failure mode | Expected behaviour | Verification |
|---|---|---|---|
| F110 | A second change keeps the first new binding held, or fails because the helper still holds it | the same helper process holds only the second binding; the first one and the default no longer open the panel | Windows native script, check 5c |
| F111 | A key name that `global-hotkey` accepts but the Wails parser does not (`equal`; the v1.2 Windows probe refused it as a conflict) is refused on the native helper | saved; status `active` with that binding | Windows native script, check 5c |
| F112 | A value the executor cannot register (the modifier-only `ctrl+alt`) is saved, or drops the binding that was active | `Conflict`; the stored value and the helper process are unchanged; status `active` with the previous binding | Windows native script, check 5b |
| F113 | After a GUI restart the helper holds the platform default instead of the saved binding, or both | status `active` with the saved binding; the default does not open the panel; the saved one does | Windows native script, check 9 |
| F114 | The macOS native script requires a helper restart for a shortcut or double-tap change, so it fails on the contract above and would pass a regression back to restarting | the same helper process before and after both changes; status `active` with the saved binding | macOS native script |
| F115 | Logs do not say which binding the executor holds, so a report of "the old shortcut still works" cannot be diagnosed | the host logs the bindings the executor reports after every apply (binding names only). F17 and F63 cover the daemon import and handler; the host already logs shortcut names when it binds them (`apps/gui-go/global_shortcuts.go`) | code review; host log in the native script output |

## Consequences

- An invalid product value no longer affects Engine session startup.
- Later product fields go into the same file. The task 4 importer uses `import.fieldSet` and
  `import.userWritten` to decide what to import.
- `tag_layout` is already product-side and stays in `uc-webserver` for now. Moving it into this
  crate is a separate decision; its file and format would not change.
- Deleting this file on factory reset (D5) and including it in config export (D6) are task 10.


### 11. P3: Engine adoption, log profile and configuration lifecycle

This section supersedes the P1 transitional ownership and export limitations in section 8.
The immutable Engine source is `a8e1b2c912cebbbb835049d3bc343cfe9a036bb7`, the merged
commit of Engine PR #227 on Engine main, including merged #225 and #226. Production code,
binding sources and Engine lockfiles match the earlier e4ff36c9 acceptance source. The old
revision is not a main ancestor, so new embedded source identifiers require rebuilt artifacts.
The merged source does not imply a published Engine release or product release approval.

- The importer runs before host log selection and before any Engine settings read.
  Its current field set is 2. Set 2 imports `general.debug_mode`. Direct pre-P1 upgrades
  import sets 1 and 2. Advancing from set 1 preserves all existing set-1 values.
- `userWritten` still protects explicit product writes. The fingerprint still hashes
  normalized values. A new Engine file without product keys is not a downgrade edit.
  The importer stays available for supported older upgrade sources. If an import cannot
  complete while the source still contains product fields, host preparation stops before
  Engine can erase them. Correct the product file permissions or type and restart.
  Without legacy product fields, an unusable product file still falls back leniently.
- The product store owns `general.debugMode`. Both `/settings` and `/diagnostics/debug`
  use that preference. Engine no longer receives product fields or debug operations.
- Bootstrap selects and installs the host profile. Existing `UC_LOG_PROFILE` and
  `RUST_LOG` precedence stays in force. `EngineConfig.with_host_log_profile` declares
  that installed label. Diagnostics report that label, including environment overrides.
  `restartRequired` compares the requested preference with the installed profile;
  writing a preference does not change the installed profile.
- The host declares `desktop-settings.json` as `ByHost`. The three updater files
  `skipped_version.json`, `last_notified_update.json` and `update_prompt_throttle.json`
  are declared as `ByEngine`. Engine includes them in exports, imports and upgrade backups.
- Factory reset deletes the product file only after Engine reset succeeds. The store
  serializes deletion with writes, resets memory, advances its revision and emits
  `settings.changed` only after deletion succeeds. `PRODUCT_SETTINGS_RESET_FAILED`
  means Engine reset completed but host deletion failed. Retrying factory reset retries
  host deletion. `desktop-telemetry.json` is retained.
- Engine applies staged configuration imports during startup. The daemon loads the
  product store afterwards, so a restored file becomes authoritative in memory.
  The current process keeps its already installed log profile until restart.
- Configuration exports exclude `desktop-telemetry.json`. Old incomplete bundles
  return `422 INCOMPATIBLE_BUNDLE` with a request to re-export from the source installation.

The reproducible real-daemon runner is `scripts/e2e/desktop-settings/adoption.py`.
Its receipts distinguish migration, CLI, effective logging, cross-installation restore,
upgrade backup, reset failures and retry. It uses synthetic isolated profiles and a no-op
system clipboard. It does not establish native GUI, Windows, Linux or physical-device acceptance.


#### Review follow-up boundaries

The initial import retains field-path diagnostics and replays them only after host tracing is
installed. Import still precedes profile selection. Invalid values and protected user writes
therefore remain visible without logging their values.

Product reset flushes the parent directory on Unix before publishing defaults. A retry also
flushes when the previous unlink succeeded but its directory flush failed. Filesystem operations
run on a blocking task. This does not establish power-loss acceptance or a Windows durable-unlink
guarantee; those remain explicit platform gates.

The E2E runner records receipts even when a daemon exits unsuccessfully. Cleanup continues for
other owned profiles, escalates an owned daemon that cannot honor TERM, and keeps the original
failure when cleanup also fails. `review_regressions.py` exercises real daemon failures.

The original 11 adoption cases do not cover the later Engine task-8 findings about old pending
schema-1 imports, aggregate export limits, blocking file reads or conflicting schema documentation.
Those require the corrected task-8 source integrated into task 9 and a new immutable Desktop pin.


#### Preserved rejected imports

The daemon projects the actual Engine `QueryDiagnostics.rejected_config_import` through
`GET /config/import/status`. The result preserves the three public reasons. `rejectedImport: null`
only means there is no rejected input; it is not proof of a completed import. I/O and journal
errors propagate as status-read failures. The Desktop host does not parse private import files.

Engine retains the original pending marker and entire staging tree and starts the original
space. Desktop queries at authenticated startup, settings entry and reconnect.
It shows a startup warning and a persistent settings notice. A client-session dismissal does not
acknowledge Engine state or suppress the next client startup. A failed query has separate visible
feedback and a retry action. Pre-stage reads use that same RTK Query endpoint, so a successful
retry clears stale query errors before any replacement prompt. There is no parallel error flag. Existing content-lock and key-recovery gates remain authoritative.

The existing picker, password validation and identity-move confirmation remain the import path.
`replaceRejectedImport` defaults to false in the HTTP request. The Go host forwards the explicit
boolean. If Engine reports rejected input, Desktop requires a separate destructive confirmation
that the preserved input may be the only copy and replacing it cannot be undone. Only that
confirmation calls Stage with true. Engine validates the new package and password before
replacing the preserved input. Cancel and wrong passwords retain it. No automatic deletion,
localStorage acknowledgement, new credential copy or weaker encryption is introduced.

Configuration export writes to a same-directory temporary file and atomically replaces the
requested output only after Engine succeeds. An aggregate-limit rejection therefore does not
truncate an existing output. This is not a claim of power-loss acceptance.

The new `rejected_import.py` real-daemon E2E covers repeated rejected-input queries and starts,
original-space history and identity, all three rejection reasons, I/O failure propagation,
default/false replacement refusal, wrong-password preservation, explicit replacement, actual
adoption, re-export and aggregate export-limit propagation. `prepare-history.py` verifies the
v1.2.0 release archive and the Engine 0.19.4 fixture hashes. `run-history.py` runs both existing
migration suites with those inputs and info logging in an isolated HOME. Failed attempts remain
separate evidence. Native visual acceptance stays with the user.
