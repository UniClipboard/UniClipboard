---
paths:
  - engine/directory.go
  - internal/directory
  - internal/localcontent/directory.go
  - app/desktop/internal/trayicon
  - app/desktop/internal/appicons
  - app/desktop/internal/appupdates
  - app/desktop/internal/lifecycle
  - app/desktop/internal/desktopprocess
  - app/desktop/internal/setupflow
  - app/desktop/internal/setupui
  - app/desktop/internal/windowchrome
  - app/desktop/e2e
  - app/desktop/internal/desktopui
  - app/desktop/internal/desktopkit
  - app/desktop/boardui
  - app/desktop/internal/i18n
  - app/desktop/internal/quickpanel
  - app/desktop/cmd/unic-desktop
  - internal/localconfig
  - internal/localapi
  - app/service/internal/server
  - app/service/internal/host
  - app/service/cmd/unic/cli.go
  - app/service/cmd/unic/history.go
  - app/service/cmd/unic/requirements.go
  - app/service/cmd/unic/session.go
  - app/service/cmd/unic/sendwait.go
  - internal/servicelaunch
  - internal/servicelifecycle
  - app/service/cmd/unicd/daemon.go
  - engine/history.go
  - engine/history_cleanup.go
  - engine/file_cache.go
  - engine/restore.go
  - engine/content.go
  - engine/content_types.go
  - engine/options.go
  - engine/types.go
  - engine/state.go
  - engine/peerproc.go
  - engine/receive.go
  - internal/store/file_cache.go
verified: 23d34c6d3f660309cca2fe0d7583c48e6d4fb6f9
---
# Desktop configuration and received files

The UI-free service loads `config.toml` and exposes the effective saved configuration over its local socket.
CLI changes and file edits use the same validation and Engine settings.

The independent GUI process uses the production `desktopui` renderer with live service data. While active, the tray process owns the sole clipboard adapter, global shortcut, notifications, login intent and service-lifetime pipe. The quick panel runs in a third Desktop OS process and remains hidden between requests. The separate `unicd` process owns Engine. Manual activation always opens the GUI. Normal GUI close keeps tray and service; lightweight GUI close retires all Desktop roles and leaves only unicd.
Fresh profiles show first-install setup and optional v1 migration before normal runtime startup. Completed setup and established v2 profiles open history directly. There is no recurring unlock flow. The marker, source detection and exclusive migration lifecycle are described in the [live desktop composition](overview.md#live-desktop-composition).
Settings > Storage > Migrate from v1 reopens the migration flow. The GUI requests a serialized tray pause. The tray closes its quick panel and unregisters its shortcut, cancels and waits for its clipboard adapter, and stops its owned service before permitting offline import. The pause and pending setup marker form the migration admission barrier before the GUI shows migration. A marker write failure prevents migration. Cancellation or process exit preserves this pending state; reopening Desktop returns to migration with its selected source. Import completion alone does not complete setup. Explicit entry starts the normal runtime again and persists completion after service readiness. An external target owner blocks import before credential access; it is not automatically stopped by migration.
General, Sync and Storage controls save through the revision-checked local configuration API.
Saved-but-not-applied failures remain visible. Native login status remains a separate launcher-owned receipt.
See the [live binding execution receipt](../exec/desktop-live-data.md) for supported operations and gaps.
`unic-desktop --version --json` reports build identity before configuration, service ownership or UI initialization. See the [release architecture](releases.md).

## Built-in plugin management

Settings > Plugins lists Tailcat sync, text tools and content classification. Plugin details use a compact back icon immediately before the toolbar title. The icon retains the translated “Back to plugins” accessible name and tooltip; the plugin list has no back icon. The detail pages edit the existing configuration through the revision-checked local API. Plugin maps and pointer values are copied before editing; failed writes retain the authoritative service snapshot. A successful save publishes the returned configuration on the view thread before controls accept another write. Saved-but-unapplied and invalid-file states remain visible. The list shows capability tags, the plugin ID, an inline enable switch (Tailcat is fixed on) and a name or ID filter. A detail page has an entity header, an overview with capability, event and run-state facts, and an Actions tab for the text tool.

Tailcat exposes its standard `text`, `images`, `files` and `auto_file_max_bytes` backend policies. They apply immediately. The file permission covers files and entire directory snapshots, including empty trees. The automatic threshold uses the selected files' total byte count; explicit sends bypass only that threshold. Global synchronization permissions and automatic-directory intent remain on Settings > Sync. Tailcat stays assembled and cannot be independently disabled or uninstalled.

Text tools and classification expose enablement and a processor failure policy. The dropdown offers inheritance from `plugins.on_error`, explicit continue and explicit block-sync choices. The inherited choice shows the current global policy; the accessible value shows the displayed choice. These settings apply after restarting the service. Quitting and reopening Desktop restarts a tray-owned service. An independently started service requires a separate restart by its owner. The Plugins page shows a pending-restart notice and does not claim the saved intent is already running. Both plugins' enablement changes participate in restart reporting; reverting to the startup settings clears the notice. Existing history is not reprocessed.

Content classification lets users add, edit and delete regular expression rules. Each rule pairs a pattern with a target tag. The form validates the draft before saving it through the same revision-checked configuration API. A valid save that changes the active rules appears in the service restart notice. See [classifier settings](../contracts/plugin-contract.md#built-in-content-classification).

The three built-in plugins have no plugin-specific Schema fields. See the [execution receipt](../exec/desktop-builtin-plugin-settings.md) for built-in management evidence.

## External plugin settings

Settings > Plugins lists external plugins discovered by the service. An external plugin detail page shows enablement, pending restart and any form defined by its manifest. The mygo renderer uses the same portable Schema as the service. Password fields show presence and allow replacement or clearing. Save checks the last revision. The service hot-replaces running Lua settings and reports a restart for Go settings. Install and uninstall are CLI operations.

The history toolbar (`history.toolbar`) and the history list/grid right-click context menu "动作" submenu (`history.item_menu`) expose matching plugin Actions filtered by both declared `Accepts` and `Placements`, displaying each Action's plugin provenance subtitle. When a single item has no matching Actions (or multiple items are selected), the context menu hides the "动作" submenu. Desktop honors each Action's declared `PostActions`: `copy` requests clipboard application through the active adapter, `notify` shows a window-level status toast (with running cancellation), `present` opens a BoardUI result dialog with copy, view-item and explicit `http(s)://` open-link controls, `select` focuses the new history item, and `none` commits silently to history. A clipboard failure leaves the committed result in history. See the [settings contract](../contracts/plugin-settings-contract.md) and [Action contract](../contracts/clipboard-action-contract.md).

## Main-window decoration

`desktop.system_window_frame` is a persisted boolean, defaulting to `false`.
Settings > General exposes the same switch on Windows, Linux and macOS. The host
captures its value when creating each GUI window. Saving a change leaves the
existing window intact. Close and reopen the main window to apply it. The GUI
compares saved intent with its own captured value for the pending notice. The
UI-free service stores the preference and never adds this host-only key to
`restart_required`; an older service generation does not keep a recreated GUI pending.

With the preference off, the host uses mygo's hidden native title bar, retaining
resize edges, rounded corners and shadows. macOS keeps its real system traffic
lights over the application-owned window top. History and Settings use one
52-DIP top band, following the shipped v1 SplitView structure. There is no separate
application-name row. The full-height sidebar reserves the native control band;
its persistent toggle remains outside the hidden/inert sidebar. The content
heading leads its toolbar, and view/search/actions trail. Collapsed/overlay
layouts continuously clear the native control area reported by mygo. Traffic
lights use a (14,18)-DIP native inset; no replacement buttons are drawn.
Setup and startup keep a separate draggable safe-area wrapper until normal
runtime content takes ownership of the window top. Linux
reserves GTK's reported caption-control area above the application content. With
the preference on, all three platforms use the normal native title bar and buttons.

Windows custom mode disables native caption buttons. The existing SplitView
provides a trailing toolbar slot. The host supplies Minimize, Maximize/Restore
and Close after the existing history commands. Search, history commands and
caption buttons share one 52-DIP band. Settings and plugin details use the same
slot. Ordinary pages have no separate `unic` caption row. Search shrinks to the
available width while caption and command hit targets retain their sizes.

Setup, offline migration, loading and startup-error pages have no SplitView;
these pages use a single minimal draggable fallback band with the same controls.
Only empty toolbar regions drag. Interactive children retain input. mygo owns
native hit testing, edge resize and double-click maximize. Maximize/Restore reads
the OS window state; the maximize element uses mygo's native maximize-button
marker. Windows 11 Snap behavior requires separate native acceptance. Close
invokes the existing GUI-close lifecycle and does not recreate a tray. System
mode draws no custom caption controls or fallback band. Quick Panel and its
satellite windows retain their independent decoration. See the
[acceptance receipt](../exec/desktop-window-chrome.md).

## Native application updates

Direct installations with a configured production update key use mygo's native updater window and the tray's check-updates action. `desktop.update_channel` accepts `stable` or `alpha`; it defaults to `stable` and applies on the next check. Updater preferences (`AutomaticChecks` and `AutomaticDownloads`) remain owned by the tray process in the same v2 user-data directory as the Desktop profile, and are exposed to the Settings → About view over Desktop role IPC (`status`, `check-updates`, and `set-update-prefs`). Switching the About channel selector to `alpha` requires explicit confirmation before persisting `desktop.update_channel` and triggering a check. Package-manager installations and portable archives retain external update ownership and route About's update check to package-manager or release-page instructions. AppImage self-update remains unsupported. The About view also lazily loads the public sponsor feed (`GET https://www.uniclipboard.app/api/v1/sponsors`) on demand and hides the sponsors section when empty or offline. The [release architecture](releases.md#native-application-updates) defines signed identity verification and deployment prerequisites.

## Incremental history browsing

Desktop loads history in 50-entry cursor pages as the list or grid approaches its loaded end. It retains a 500-entry sliding window, bounded cursor checkpoints and one selected detail outside that window. Scrolling toward an evicted newer page reloads it automatically. Stable item keys retain surviving scroll anchors.

Image preview admission occurs before `HistoryContent` can open or materialize a
response. Each UI process owns one slot through response close and decode completion.
The independent GUI and Quick Panel reuse this implementation but have separate
queues; this is not service-wide admission. Selected originals take priority over queued
neighbors. Cancelling a queued request removes it without opening a response; a
cancellation racing admission returns the transferred slot. Each view retains its
four-job bound and cancels one pending neighbor when selection needs that job.
Text and directory metadata do not take the image slot. Single-file candidates
also take it because their image MIME/name can be known only after response open.

Neighbor thumbnails use one 64 KiB input buffer over the shared byte limiter.
This separates codec read sizes from local-socket reads and permits short reads.
They stream the existing 32 MiB-bounded input through the existing
image codec and EXIF handling. Only inspected dimension-header bytes are replayed;
ordinary encoded images are not first copied into a whole-input buffer. The header
replay buffer uses the same encoded-input budget. Full decoding starts only after
the existing 16-million-pixel dimension check. A bounded remainder read detects
trailing input beyond the byte limit. Context checks cover reads and codec/resize
phase boundaries; a running codec/resize call is not preempted. Selected originals
retain their encoded reconstruction bytes and full dimensions. For a selected
original, a valid local response length up to 32 MiB reserves its encoded buffer
once, and a fixed 64 KiB copy buffer bounds individual read requests. Unknown or over-limit
lengths keep the bounded fallback. The actual byte limiter and raster check stay
authoritative when a hint is inaccurate. Neighbor thumbnails keep their streaming
path. See the [image implementation receipt](../exec/desktop-image-memory.md) and
[independent quick-panel receipt](../exec/quick-panel-memory.md).

Quick-panel reopening invalidates its cleared row projection even when the
service reuses an unchanged query/revision. It preserves the service cache and
reconstructs the real result list and satellite instead of an empty panel.

Service events trigger metadata revision checks. A 30-second bounded recovery probe detects missed notifications. An unchanged revision does not reload search results. Stream failure renews cursors, while query changes cancel stale generations. Selection, detail state and filters survive background reconciliation; selected deletion closes its detail and preview. See the [search contract](../contracts/history-search-contract.md#incremental-desktop-browsing) and [execution receipt](../exec/desktop-incremental-browsing.md).

## History source application

The history detail header shows the stored source application's icon and name immediately after device/time.
It uses `HistoryItem.SourceApplication`, including metadata received from another device.
Application identity stays separate from the clipboard preview image and source-device label.
Desktop uses the supplied display name, or the application identifier if the name is empty.
A missing or malformed PNG uses the existing generic package icon; unknown application metadata adds no header item.
Long names stay on one line within a bounded area. The tooltip retains the full name and identifier.
Application names are never translated. The accessible group label uses the desktop locale.
The [source application contract](../contracts/source-application-contract.md) defines capture and storage semantics.
The [Desktop source application receipt](../exec/desktop-source-application.md) records view acceptance and native evidence limits.

## Settings

Settings organizes active categories into three grouped sidebar sections:
General & Experience (`general`, `appearance`, `quickPanel`), Sync & Data
(`sync`, `storage`, `plugins`), and Diagnostics & System (`diagnostics`,
`about`), removing unimplemented placeholder categories from the navigation.

Settings > Appearance exposes `desktop.color_mode` as `system`, `light` or
`dark` (default `system`) via visual theme cards, alongside `desktop.tray_icon`
and `desktop.system_window_frame`. The revision-checked configuration path
persists these host-only preferences. Each native Desktop role sets mygo's
shared theme source before startup and watches subsequent configuration changes.
Main GUI, setup and Quick Panel therefore consume the same saved scheme. The
[appearance receipt](../exec/desktop-settings-appearance.md) proves configuration
and production binding behavior; native appearance remains separate acceptance.

Settings > Diagnostics & Privacy exposes persistent local/upload log levels,
per-signal telemetry switches with inline runtime/dropped status pills,
effective diagnostic state and bounded fifteen-minute debug overrides with live
countdown badges. Overrides use existing diagnostic operations without editing
the saved configuration. The view schedules status refreshes while this page is
open. Native Desktop callbacks select an archive destination and run the private
sibling CLI against the current profile. Export safety follows the offline log
archive contract. Opening the logs directory remains host-owned. See the
[diagnostic receipt](../exec/desktop-settings-diagnostics.md).

Settings > Storage & Backup shows aggregate default-cache stored/reserved bytes
and file/held counts through the existing service operation, paired with a
quota meter, cache quota control, receive directory indicator, history
retention tag-pill exclusions and local data directory/migration actions. Cache
values exclude the database, logs, user-managed receive directories and other
user files. An explicit refresh failure marks any retained sample as previous
data. The profile-directory opener remains host-owned. Full disk inventory,
manual cache maintenance, clear-all history and explicit search-index
maintenance are not supplied by these reads. See the
[storage receipt](../exec/desktop-settings-storage.md).

Storage also exports and imports a bounded portable-preference backup. It uses
an explicit typed allowlist rather than copying the profile or serializing the
full configuration. Import validates before preview, requires confirmation and
saves against the preview's original revision. Local identities, credentials,
plugin settings, paths, login registration, shortcut bindings and the separate
mygo updater preferences remain excluded. Exports refuse existing destinations.
See the [backup receipt](../exec/desktop-settings-backup.md) for the format,
allowlist and unexecuted native-file-panel boundaries.

Quick Panel settings passively display the current application's Accessibility
permission status and provide explicit refresh and System Settings actions.
macOS uses the existing mygo permissionflow library. Other platforms expose an
unsupported state for this specific permission switch. Reading never requests
permission; opening settings never counts as a grant or paste readiness. Reads
and actions use independent bounded workers with generation/cancellation checks.
The actual paste owner retains its existing permission checks. See the
[permission settings receipt](../exec/desktop-settings-shortcuts.md).

| Key | Default | Effect |
|---|---|---|
| `desktop.color_mode` | `"system"` | Select the shared system/light/dark scheme for native Desktop roles; apply saved changes without a restart. |
| `desktop.mode` | `"normal"` | Manual activation always opens the GUI. Normal GUI close keeps tray/service; lightweight GUI close leaves only unicd. Explicit background entry omits the GUI after setup. Lightweight login is service-only; normal login uses its background preference. |
| `desktop.launch_at_login` | `false` | The running tray applies login registration; actual OS status is displayed separately. |
| `desktop.tray_icon` | `"glyph"` | Immediately select `glyph` or `cat`; no restart required. |
| `desktop.start_in_background` | `false` | Normal login startup only: true omits the GUI, false opens it. Manual activation always opens the GUI; explicit `--background` always omits it after setup. |
| `sync.enabled` | `true` | Pause content sends and receives. Keep connections and local history. |
| `sync.automatic` | `true` | Send new observed copies. Local history, manual sends, receives, and Actions remain independent. |
| `sync.on_restore` | `false` | After a successful explicit history restore, dispatch the restored content once. |
| `sync.directories.automatic` | `false` | Prepare and send directory copies automatically; explicit send remains available. |
| `sync.files` | `true` | Permit file sends and receives, subject to each backend's policy. |
| `inbox` | Empty | Empty selects the default file cache. An absolute custom directory selects user-managed receives after restart. |
| `limits.max_file_bytes` | `"100MB"` | Bound each file transfer in both directions. The boundary is inclusive. |
| `file_cache.max_bytes` | `"500MB"` | Bound default-cache file bytes and pending file reservations. |
| `history.auto_cleanup` | `true` | Enable unified history and managed-file cleanup. |
| `history.retention_days` | `180` | Age from record creation; 0 disables the age limit. Applies to existing records. |
| `history.max_items` | `0` | Limit eligible records; 0 means unlimited. Excluded records do not count. |
| `history.excluded_tags` | `[]` | Skip any record with an exact matching tag, including its managed files. |
| `history.max_bytes` | `"4GiB"` | Startup history storage budget; a change requires restart. |

When automatic cleanup is off, Desktop retains the saved age, item-count and excluded-tag rules but disables their controls. The cache-quota control remains enabled. Reenabling cleanup makes the retained rules editable again.

The existing size parser uses powers of 1024: 100 MB is 104857600 bytes.
A file-size limit governs transfer, not local reference import or local history.
Protocol hard limits still govern content import.
No setting changes sync frequency. No setting creates metadata-only transfers or on-demand downloads.
The existing backend automatic-size filter remains a separate backend policy.

## Configuration writes

`GET /v1/config` returns the saved typed configuration, its raw-file hash revision, errors, and restart requirements.
`PUT /v1/config` accepts `{revision,toml}` and rejects stale or observed external edits with HTTP 409.
The daemon validates the whole document and replaces the file from a private temporary file in the same directory.
A failed parse preserves the last valid typed configuration.
The revision tracks the latest observed disk document, including an invalid document, so a CLI apply can repair the file.

The saved file and runtime Engine state do not share a transaction.
`apply_error` reports a saved configuration that the host could not apply.
`restart_required` reports fields that need a restart, including `inbox` for the service.
`desktop.mode` applies to the next GUI close. `desktop.start_in_background` applies
to normal-mode login startup. Neither field requires a restart flag or suppresses manual activation.
The service only validates and stores desktop settings. It never registers a login item or opens a window.
See [headless architecture](headless.md#desktop-startup-settings) for desktop ownership, actual login status and platform limits.
An external editor does not share the daemon write mutex; a later external save remains authoritative through the file watcher.

Desktop tag color overrides use the same configuration API and CAS behavior. `desktop.tag_colors` holds at most 4096 SHA-256 tag-ID keys and palette or `#rrggbb` values. The desktop resolves them into its shared tag registry on refresh and reopen. The service stores and validates presentation values without depending on the UI Kit. See [tag controls and colors](overview.md#tag-controls-and-colors).

## CLI service usage

Service commands acquire a generation-bound usage lease on their first request.
They reuse a running service or launch the sibling `unicd --cli-temporary` under the shared profile transition lock.
The final usage release retires a CLI-temporary service. CLI commands do not stop a borrowed standalone or desktop-owned service.
`unic config path` and `unic dev logs export` run without a service.
`unic history use` requires an available desktop clipboard adapter; starting a temporary service does not supply that adapter.
See [headless architecture](headless.md) for acquisition, ownership and adapter requirements.

## Restore

`unic history use` accepts a history ID or `@N`.
The command waits up to 60 seconds for its own send operation, when one exists.
JSON output contains separate `restore` and `send` objects. Text output includes the local result, network disposition, operation ID and send outcome.
An incomplete send reports exit code 4 after printing the result. A local clipboard write remains applied.
`unic history action` also waits for its own send operation; `--apply` adds the desktop clipboard adapter requirement.
The CLI can supply `--request-id` for retry deduplication and `--json` for machine-readable output.
The host keeps up to 1024 restore results for 30 minutes per daemon instance.
A full result cache rejects new requests instead of forgetting a result early.
A restart ends that deduplication window. The application does not promise durable exactly-once delivery.

Engine authorizes resend with the last successful history apply token.
`SyncRestoredHistory` dispatches existing content without a new commit or captured processor call.
The stored processor prohibition still blocks resend.
Applying an Action result uses the local-only apply path and does not trigger a second send.
Global sync, global file permission, backend policy, limits, and trust still apply.
A network failure cannot undo the local clipboard write.

## File ownership

The desktop host supplies `DataDir/inbox/file-cache` through `Options.CacheDir`.
Engine retains received payloads in the private cache that the host provides.
Temporary import and receive staging remains separate from retained cache ownership.
Other hosts can supply a different app-private directory without exposing a user-selected destination to Engine.
Its SQLite ledger records payload identity, size, completion time, and availability.
Engine journals ownership before creating each payload directory.
Default-cache history, Received, and content leases share the retained payload.
Unified cleanup removes eligible history records and releases their managed payloads.
Capacity reclamation uses the same deletion path and tag exclusions.
If automatic cleanup is off or eligible records cannot free enough bytes, default-cache admission fails.
`parts[].available=false` reports unavailable default-cache content.
Reading or restoring unavailable content returns `resource_expired`.

Custom receive files are user-managed.
The desktop host validates the configured destination and supplies `Options.StoreReceivedFile`.
Engine calls that callback with `ReceivedFile{Name, MIME, Size, Reader}` after processing the received content.
The callback consumes the stream and returns a readable `PersistentFileSource`.
The desktop host creates the file exclusively and owns its destination, permissions, and reference format.
Engine validates the source and stores its persistent reference without interpreting the reference.
A callback failure publishes no received item or history and does not fall back to the default cache.
The callback runs without Engine state, history, or content locks and must honor its context.
The default-cache quota does not count custom receives or reject them because the default cache is full.
Transfer limits, transient transfer resources, and disk-write errors still apply.
Custom finalization never silently overwrites an existing file.
The desktop host uses an opaque `desktop-received:` reference for custom received files.
`ResolveFileRef` resolves that reference after restart.
`FileReference` returns a checked host reference under a content lease.
`LocalFilePath` returns paths only for Engine-owned files and reports `unsupported` for host sources.
Only the desktop host decodes references for system clipboard writes.
Ordinary history responses, cache inventory, logs, and events contain no host references.
A partial custom destination from failed finalization remains for the user to inspect.

## Cleanup invariants

- Delete retained file content only when the default-cache ledger proves ownership and the open root confines the deletion.
- Use `os.Root` for cache publication and deletion. Refuse cleanup when the default-root identity changes.
- Reject a symlink as a managed root. Never follow a payload symlink into an external directory.
- Never scan a custom receive directory for old files, orphan files, or filename prefixes.
- Never unlink a custom received file on history deletion, history expiry, lease release, restart, or shutdown.
- Never enroll an earlier custom received file in cleanup after changing the active location.
- Keep payloads held by reads, sends, applies, or the clipboard.
- Service restart invalidates prior clipboard leases; persisted history and current content holders retain their own managed files.
- Admit headless file receives without waiting for a desktop baseline.
- Run bounded age/count cleanup after startup, policy changes, commits, and every 30 seconds while running. Each pass deletes at most 256 eligible records.
- Remove independent file-retention settings and payload-only eviction. One switch governs age/count cleanup and capacity-driven history deletion.
- Keep excluded records and their managed files. Manual deletion and explicit per-item expiry remain available independently.
- Delete logical records first. Reclaim physical payloads only after the final history reference and content/clipboard holder are gone.
- Keep failed-deletion bytes charged until physical deletion and ledger removal succeed.
- Do not run cache cleanup in offline migration mode.

The daemon no longer scans `clip-*` paths in the configured receive directory.
Temporary receive staging stays under the daemon's application storage.
After acquiring the profile lock, Engine removes plain `unic-content-*` and `unic-recv` directories from its InboxDir through `os.Root`.
This startup scan does not follow symbolic links, inspect the custom receive destination, or reclaim retained cache payloads.
Cleanup does not enumerate unknown default-cache directories merely because their names resemble generated IDs.

## Code map and validation

- `internal/localconfig/manager.go`: serialized saves, raw-file revision, watcher, and restart status.
- `app/service/cmd/unic/cli.go`: configuration commands and cache inventory.
- `app/desktop/internal/lifecycle/service.go`: tray acquisition policy (borrow a standalone service, start an owned service when absent, refuse another desktop owner or CLI-temporary service); process primitives are in `internal/servicelaunch`.
- `app/desktop/internal/desktopprocess`: profile-scoped kernel role locks, private activation IPC and OS peer executable verification.
- `app/service/cmd/unic/history.go`: explicit history restore, Action execution and retry key.
- `app/service/cmd/unic/requirements.go`, `session.go`, `sendwait.go`: command requirements, generation-bound service usage and own-operation completion.
- `engine/restore.go`: token-authorized resend without a new commit.
- `engine/file_cache.go`: private-cache ownership, quota reservation, pins, orphan reclamation, and host callback validation.
- `engine/history_cleanup.go`: unified age/count cleanup, tag exclusions, and capacity-driven record deletion.
- `app/service/internal/host/received_files.go`: service destination validation, custom file writing, and reference encoding/resolution.
- `engine/content.go`: separate access to Engine-owned file paths and opaque host references.
- `engine/history_import.go`: the existing directory-sync helper used by cache finalization.
- `internal/store/file_cache.go`: durable ownership and availability transitions.
- [Pipeline](pipeline.md), [Engine contract](../contracts/engine-contract.md), and [history contract](../contracts/history-contract.md).

CLI E2E uses real paired daemons and directory clipboards.
See the [execution receipt](../exec/0016-sync-and-file-settings.md) for exact verification and remaining limits.

## Gaps

The production Settings tab exposes startup, sync, history-cleanup and quick-panel shortcut controls. Other controls remain available through CLI/TOML.
Native clipboard and folder-picker interaction acceptance remains with the user.
Crash injection at every filesystem/database instruction and forced disk-full/unlink-failure acceptance are not complete.
Unknown retained-cache entries are not deleted through filename scans; only the ledger grants retained-payload deletion authority.

## Cleanup settings interface

The desktop Settings tab reads `GET /v1/config` and `GET /v1/history/tags`.
The tag endpoint returns the persisted tag-name list, including unassociated imported tags.
It contains no content or file references.
Settings save through the existing revision-checked `PUT /v1/config` path.
The page provides the unified switch, retention time, count limit, excluded-tag checkboxes, and per-device cache quota.
Configured values outside the preset lists remain selectable. The quota control remains enabled when cleanup is off.
Explicit clipboard writes release a replaced file lease after importing the new snapshot; otherwise rebasing would hide that release from polling.
Native visual acceptance remains separate from CLI acceptance.

## Client diagnostics

The host creates diagnostics before Engine construction and injects its logger and fixed-field callback.
Local logs are asynchronous and bounded under the profile directory. Remote logs, metrics and PostHog events default enabled. The `telemetry.enabled` master switch and independent `telemetry.logs.enabled`, `telemetry.metrics.enabled`, and `telemetry.analytics.enabled` switches can disable them through config.toml. Switch changes hot reload; endpoint changes require restart. A build without release deployment metadata reports degraded uploads and keeps the service usable.
`unic dev logs level` queries the independent local/remote thresholds. A temporary override uses `--for` and `--target`.
`unic dev diagnostics telemetry` reports signal state, losses, and pending endpoint restart without disclosing the endpoint or credentials.
See the [observability architecture](observability.md) and [contract](../contracts/observability-contract.md) for the execution points, strict socket DTOs, privacy, and cancellation barrier.

## Workspace verification identity

The `verified` front matter names the committed source checked against this page.
It does not establish native interaction acceptance.
The startup acceptance manifest binds the current source and executable hashes.
`docscheck -bump` requires committed source and rejects uncommitted changes under this page's code paths.

## Search reads

The normal history window uses explicit service search, not filtering its loaded page.
Selected full content remains a separate bounded read. Search counts and content reads have independent cancellation and never share the desktop mutation queue.
The real-data quick panel uses the same service query and catalogue with its existing phrase and tag-union semantics.
Unsupported OS paste and semantic classifications remain separate from retrieval. See the [search contract](../contracts/history-search-contract.md) and [execution receipt](../exec/desktop-search-implementation.md).

## Quick-panel system interaction

The tray captures the external foreground target before starting or activating the independent quick-panel process. It sends the ephemeral target through private Desktop IPC. The quick-panel process owns the target session in `app/desktop/internal/quickpanel`, its history window and its nonfocusable preview. Global shortcuts, tray activation and the Settings button use the same opening path. Repeated activation while visible dismisses the panel without replacing the target.

`desktop.quick_panel_shortcut` defaults to Command+Ctrl+V on macOS and Ctrl+Alt+V on Windows/Linux. Settings offers both platform bindings, Ctrl plus backtick, Command/Ctrl+Shift+Space and disabled. The existing revision-checked configuration persists the choice. The host registers a replacement before removing the working registration and exposes requested/effective disagreement with an explicit retry. Normal mode keeps the tray shortcut available after GUI close. Lightweight GUI close and lightweight background/login entry leave only unicd, so no shortcut or clipboard adapter remains.

Enter applies history through the existing IPC clipboard adapter, hides both windows, restores the captured target and sends one paste chord. Shift+Enter uses the existing plain-text Action and applies its resulting history item. Command/Ctrl+Enter waits 150 ms after input dispatch and reopens the panel only if the target still owns focus. Copy does not inject keyboard input. The panel delegates application and clipboard-match checks to the tray through private IPC. The tray's tracked clipboard wrapper verifies exactly one adapter write and compares content plus file paths before paste. The panel never acquires another adapter.

Generation cancellation prevents stale clipboard completions from sending input. Focus checks, held-key waits and input dispatch are bounded; errors never automatically replay input. External blur does not restore focus. Explicit Escape dismissal restores the original target only while unic still owns the foreground. Target names, IDs and clipboard fingerprints remain transient host state and never enter logs or analytics.

macOS uses Accessibility-gated CoreGraphics input and NSRunningApplication identity. Windows uses process/window identity and SendInput. Linux X11 uses checked XGB/XTest requests and EWMH activation. Wayland retains the mygo portal shortcut and Copy; automatic target capture/paste is explicitly unavailable. Windows ARM64 native hotkey delivery, panel foreground ownership and Escape target restoration passed in the [process receipt](../exec/desktop-process-lifecycle.md). Actual paste, permission flows, other-platform display-server policy and keyboard-layout acceptance remain unverified. See the [system contract](../contracts/desktop-quick-panel-contract.md) and [execution receipt](../exec/desktop-quick-panel-system.md).

## Desktop acceptance entry

`make desktop-e2e` is the canonical local and CI acceptance entry. Declarative testscript cases live under `app/desktop/e2e/cases`; the generated catalogue is checked during ordinary Go tests. The runner owns source snapshots, binary assembly, private profiles, per-case results and one HTML report. It retains screenshots and continuously sampled window-content recordings for native cases, with source/program/evidence hashes. All 25 former Desktop acceptance entries are migrated into package-local scenario registries. The generated catalogue identifies each case's mode and environment. PR CI runs the ordinary isolated cases in six disjoint duration-balanced shards on independent hosted runners. A timing report compares measured cases and shards and provides a candidate timing baseline after a successful run. Each job keeps the thirty-minute budget and normal evidence/cleanup requirements; full acceptance requires all shards to pass. View cases retain decodable software-rendered checkpoints. The runner verifies Go JSON events and rejects skipped subcases. Registry/catalogue mismatches fail preflight. Original test entries and standalone scripts are removed.

The `desktop_e2e` build tag imports mygo's opt-in automation plugin, assigns a unique application name and normally supplies the existing directory clipboard through the host Board seam. Untagged builds use the original system board and do not import automation. Ordinary native cases execute real desktop view actions against a production service and check directory-clipboard bytes. Those cases do not establish global key delivery, OS focus policy, system clipboard or cross-application paste.

Explicit `Environment: disposable-guest` cases retain the production system board only in a verified, authorized QEMU/KVM Wayland guest with its SPICE clipboard user agent stopped. They initialize private profiles through the production CLI, refuse locked sessions and hold only temporary idle inhibition. Ordinary runs and CI shards exclude these cases and retain NOT_RUN verdicts. The UTM adapter verifies VM/network identity and delivers each physical-input request at most once. Native clipboard read-back, actual target insertion, OS-window screenshots and mygo window-content recordings remain separate evidence. The [native integration receipt](../exec/desktop-native-integration.md) records exact scopes and unresolved failures. In the recorded Fedora KDE Wayland run, fresh capture failed while the main window was hidden; reopening and new capture passed. This is an unresolved acceptance failure, not an accepted background-capture guarantee.

The runner builds from a private frozen snapshot, so edits in the developer checkout do not change an active run. It rejects missing evidence, failed/skipped selected cases and source-snapshot mutation. It publishes the final verdict only after report/checksum generation succeeds. Native windows and services are stopped before private artifacts are removed. See the [E2E contract](../contracts/desktop-e2e-contract.md), [developer guide](../../app/desktop/e2e/README.md), and [execution receipt](../exec/desktop-e2e.md).

## Directory capture, send, and receive

The directory flow follows the [directory contract](../contracts/directory-contract.md). Default capture stores a cheap source reference. Desktop history distinguishes this reference from a frozen snapshot. Its send menu submits the selected history item and selected devices through the existing service. Scanning, preparation and sending have separate progress labels and an explicit cancel action. Linux settings disclose the identical-copy marker limitation. Native clipboard and visual acceptance are recorded separately in the [execution receipt](../exec/0024-directory-copy-and-send.md).

Default-cache directory application builds lease-owned views from retained members. Custom receives stage one complete batch under the configured destination, then publish it by rename. The host journals only its exact unpublished staging path in private profile storage before creation. Startup recovery uses those journals and validates destination identity; it never scans the custom destination or deletes published trees. Failed cleanup retains its journal. Existing custom destinations are never merged into or overwritten.

Before writing a prepared tree, the adapter rechecks the current clipboard against its observed baseline. A newer local copy defers the old application. Copying the same held paths again keeps their application lease while the new copy action is processed.

## Application and tray icons

Desktop reuses the original v1 application assets in `app/desktop/internal/appicons`. The asset README records their source commit and SHA-256 digests. The main window, macOS Dock and About view share the embedded application PNG.

Settings > General > Tray icon selects `glyph` (the v1 default, two offset rounded cards) or `cat` (the original cat alpha mask). The revision-checked configuration API persists `desktop.tray_icon`. The existing Desktop file watcher applies the saved preference immediately. `unic config set desktop.tray_icon '"cat"'` and direct TOML edits use the same validation and update path. Invalid values retain the previous valid configuration. The tray applies this preference while it runs. Normal GUI close retains the tray; lightweight GUI close removes it.

`app/desktop/internal/trayicon` owns geometry, platform palettes and image publication. It uses mygo's existing SVG renderer for the v1 glyph paths and x/image for cat scaling. macOS uses a 44-pixel template with a centered 36-pixel glyph grid or 34-pixel cat source. Windows uses the system small-icon metric and mygo template contrast. Linux uses 22 pixels, explicit light/dark foreground colors and the v1 KDE/gsettings palette selection. Theme changes repaint the currently selected style; Linux also checks KDE configuration changes once per second until Desktop shutdown.

Unchanged style preferences do not publish an image. Failed native delivery retains the shown style so the next update can retry. Style changes call only the native image API. Only language changes rebuild translated menus. No state badges or animation are implemented.

The [style execution receipt](../exec/desktop-tray-icon-styles.md) records nonvisual acceptance and remaining native tray appearance, DPI, theme-event and open-menu acceptance boundaries.


### Shared migration admission

Desktop and CLI imports publish the same profile-scoped migration receipt under
the service transition lock. A partial or cancelled Desktop import remains
pending for CLI initialization, including custom sources outside default
discovery. Only a verified complete import publishes completion. Desktop
welcome/skip and history navigation do not publish CLI completion. See the
[initialization contract](../contracts/local-service-contract.md#explicit-cli-migration-initialization).

## External sync instances

Plugins separates installed program identity from configured runtime instances. Create an instance with its own ID, edit its Schema values, enable it and restart. A new instance starts disabled and never copies another instance's credentials. The detail shows local runtime state; standard content policies save through the existing revision-checked configuration route. Go settings changes take effect after restart and invalidate that instance's old authorizations before its new process starts.

Device management uses one unified BoardUI presentation for live and preview data with user-facing "sync channel" terminology. The left canvas opens with a runtime sync-channel status strip that surfaces running, restarting, unavailable, and disabled channels plus pending-restart notices and jumps directly to Settings > Plugins (or a specific plugin detail). A global-sync-off warning banner offers one-click re-enablement. Pending banners distinguish brand-new device join requests from new-path requests on already-trusted devices, and external channels (`kind == "external"`) appear in their own section without device fingerprints.

The right 380-DIP inspector manages the selected item: the local device displays inline rename (`config.DeviceName`), the local `DeviceID` (never a fabricated pairing fingerprint), and a clickable channel status overview; a trusted device supports revision-checked inline rename (`PATCH /v1/peers/{id}`, clearing to revert to `reported_name`), a top inline pending-path approval card whose rejection removes only the pending binding via `DELETE /v1/bindings/{id}` without unpairing the device, per-path sync switches (`PATCH /v1/bindings/{id}`) and overflow actions (channel policy jump and confirmed path removal), an 8-cell fingerprint grid, a batch pause/resume button shown only when two or more authorized paths exist, and confirmed device removal (`DELETE /v1/peers/{id}?revision=N`); external channels expose name/enable/auto-apply controls (`PATCH /v1/channels/{id}`) and revocation-aware confirmed removal (`DELETE /v1/channels/{id}?revision=N`).

Device invitation and connection tasks select a running peer-sessions channel (hiding the selector when only one channel is running, and replacing the form with plugin-settings guidance when zero are running). External-channel and unsupported object-store types do not enter device pairing. Opening the pairing sheet snapshots existing pending revisions so inline approval switches only for matching new or updated requests. History detail shows each backend's disposition/state/error, so one successful path does not hide another failure.

`make desktop-e2e CASE=external-sync-workflow` drives the real mygo composition, production service and real external Go fixture processes. It seeds only the verified fixture installation; installation itself is covered by the real CLI GitHub-fixture workflow. It creates/enables a runtime instance through UI, restarts, changes policy, approves a cross-instance pair and checks exact bidirectional text plus per-instance results. Its recording is native application content, not OS clipboard, cross-application paste or user visual approval. See the [workflow receipt](../exec/sync-plugin-workflow.md).

## Unified content presentation and component acceptance

Main GUI, first-install setup, Quick Panel and demos use the same native BoardUI content resources through the public `boardui` controls and application-only `desktopkit` compositions. The former skin engine, platform token snapshots and selector are removed. System/light/dark remain scheme choices. OS frame policy, modifiers, tray/system menus, file and credential prompts, updater, clipboard ownership and focus/paste dispatch remain native host integration.

The independent `boardui-demo` uses synthetic local form state and creates no product service/GUI/panel/tray role. Production pages use their existing live binding and runtime configuration. Canonical `boardui-settings`/`boardui-states` cases and the real-React replay cover the named component scenes. Production composition/migration/menu/surface cases cover their own shared-renderer scenarios. These results do not imply native OS keyboard/IME, platform accessibility, compositor or Windows memory acceptance. See the [native BoardUI contract](../contracts/boardui-native-contract.md) and [production migration receipt](../exec/boardui-production-migration.md).

## External share Actions

History Action menus filter declarations by the selected item type. Go plugins
may declare text, image and file inputs. Their result remains a new text history
item and uses the existing explicit apply path. Desktop opts into cancelling
admitted Action work with the caller context. CLI allows the five-minute Action
budget plus transport margin. Provider upload behavior remains outside the
Desktop composition. See the [Action contract](../contracts/clipboard-action-contract.md).

Desktop Action jobs use a five-minute plus thirty-second worker budget for
execution, response and clipboard application. Other queued jobs and mutations
retain their fifteen-second budget. The Engine independently enforces each
Action declaration's execution deadline.

## Online plugin marketplace

Settings > Plugins opens with an Obsidian-style Plugin Marketplace card above
the installed third-party plugins and built-in features. `GET /v1/plugins/marketplace`
reads a static JSON registry (`UNIC_PLUGIN_REGISTRY_URL`, defaulting to the
official `registry.json` and accepting `off` for offline bundled mode), caches
validated entries locally under `$UNIC_HOME/plugins/marketplace-cache.json`,
and falls back to the cached or bundled official catalog (including zero-config
`unic.wenshushu`) when offline.

One-click install and update call `POST /v1/plugins/install`, reusing the
existing GitHub commit-locked, SHA-256-verified installer (`luascripts.Install`)
with a three-minute Desktop worker budget. Uninstall (`DELETE /v1/plugins/{id}`)
and rollback (`POST /v1/plugins/{id}/rollback`) update the immutable
installation lock and mark restart required while preserving history and local
configuration. Zero-config plugins omit empty settings sections and display
their declared Actions, usage steps and privacy boundary. Execution order
(`plugins.order`), global `plugins.on_error` policy and manual URL install stay
under the overflow menu.
