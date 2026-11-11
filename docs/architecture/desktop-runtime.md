---
paths:
  - app/desktop/internal/lifecycle
  - app/desktop/e2e
  - app/desktop/internal/desktopdemo
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
verified: 59972253ea527ab77cb340a50ee4f5d5be92c55b
---
# Desktop configuration and received files

The UI-free service loads `config.toml` and exposes the effective saved configuration over its local socket.
CLI changes and file edits use the same validation and Engine settings.

The normal desktop uses the shared mygo demo composition with live service data.
It opens history directly, with no setup or unlock flow.
General, Sync and Storage controls save through the revision-checked local configuration API.
Saved-but-not-applied failures remain visible. Native login status remains a separate launcher-owned receipt.
See the [live binding execution receipt](../exec/desktop-live-data.md) for supported operations and gaps.
`unic-desktop --version --json` reports build identity before configuration, service ownership or UI initialization. See the [release architecture](releases.md).

## Settings

| Key | Default | Effect |
|---|---|---|
| `desktop.mode` | `"normal"` | Next launch: normal desktop owns/restarts its service; lightweight leaves only a standalone service. |
| `desktop.launch_at_login` | `false` | Running normal desktop applies login registration; actual OS status is displayed separately. |
| `desktop.start_in_background` | `false` | Next normal launch: create the main window hidden while tray and clipboard integration run. |
| `sync.enabled` | `true` | Pause content sends and receives. Keep connections and local history. |
| `sync.automatic` | `true` | Send new observed copies. Local history, manual sends, receives, and Actions remain independent. |
| `sync.on_restore` | `false` | After a successful explicit history restore, dispatch the restored content once. |
| `sync.files` | `true` | Permit file sends and receives, subject to each backend's policy. |
| `inbox` | Empty | Empty selects the default file cache. An absolute custom directory selects user-managed receives after restart. |
| `limits.max_file_bytes` | `"100MB"` | Bound each file transfer in both directions. The boundary is inclusive. |
| `file_cache.max_bytes` | `"500MB"` | Bound default-cache file bytes and pending file reservations. |
| `history.auto_cleanup` | `true` | Enable unified history and managed-file cleanup. |
| `history.retention_days` | `180` | Age from record creation; 0 disables the age limit. Applies to existing records. |
| `history.max_items` | `0` | Limit eligible records; 0 means unlimited. Excluded records do not count. |
| `history.excluded_tags` | `[]` | Skip any record with an exact matching tag, including its managed files. |
| `history.max_bytes` | `"4GiB"` | Startup history storage budget; a change requires restart. |

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
`restart_required` reports fields that need a restart, including `inbox` for the service
and `desktop.mode` / `desktop.start_in_background` for the next desktop launch.
The service only validates and stores desktop settings. It never registers a login item or opens a window.
See [headless architecture](headless.md#desktop-startup-settings) for desktop ownership, actual login status and platform limits.
An external editor does not share the daemon write mutex; a later external save remains authoritative through the file watcher.

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
- `app/desktop/internal/lifecycle/service.go`: desktop launch policy (normal replaces a service, lightweight reuses a standalone one and refuses a desktop-owned or CLI-temporary service); process primitives are in `internal/servicelaunch`.
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

Desktop owns a target session in `app/desktop/internal/quickpanel`. The host captures the external foreground target before showing the existing history window and nonfocusable preview. Global shortcuts, tray activation and the Settings button use the same opening path. Repeated activation while visible dismisses the panel without replacing the target.

`desktop.quick_panel_shortcut` defaults to Command+Ctrl+V on macOS and Ctrl+Alt+V on Windows/Linux. Settings offers both platform bindings, Ctrl plus backtick, Command/Ctrl+Shift+Space and disabled. The existing revision-checked configuration persists the choice. The host registers a replacement before removing the working registration and exposes requested/effective disagreement with an explicit retry. Lightweight mode has no graphical shortcut host.

Enter applies history through the existing IPC clipboard adapter, hides both windows, restores the captured target and sends one paste chord. Shift+Enter uses the existing plain-text Action and applies its resulting history item. Command/Ctrl+Enter waits 150 ms after input dispatch and reopens the panel only if the target still owns focus. Copy does not inject keyboard input. A tracked clipboard wrapper verifies exactly one adapter write and compares content plus file paths before paste.

Generation cancellation prevents stale clipboard completions from sending input. Focus checks, held-key waits and input dispatch are bounded; errors never automatically replay input. External blur does not restore focus. Explicit Escape dismissal restores the original target only while unic still owns the foreground. Target names, IDs and clipboard fingerprints remain transient host state and never enter logs or analytics.

macOS uses Accessibility-gated CoreGraphics input and NSRunningApplication identity. Windows uses process/window identity and SendInput. Linux X11 uses checked XGB/XTest requests and EWMH activation. Wayland retains the mygo portal shortcut and Copy; automatic target capture/paste is explicitly unavailable. Native focus/input, permission flows, display-server policy and keyboard-layout acceptance remain unverified. See the [system contract](../contracts/desktop-quick-panel-contract.md) and [execution receipt](../exec/desktop-quick-panel-system.md).

## Desktop acceptance entry

`make desktop-e2e` is the canonical local and CI acceptance entry. Declarative testscript cases live under `app/desktop/e2e/cases`; the generated catalogue is checked during ordinary Go tests. The runner owns source snapshots, binary assembly, private profiles, per-case results and one HTML report. It retains screenshots and continuously sampled window-content recordings for native cases, with source/program/evidence hashes. All 25 former Desktop acceptance entries are migrated into package-local scenario registries. The 26-case catalogue includes 21 software-view cases, four process cases and one native-window case. View cases retain decodable software-rendered checkpoints. The runner verifies Go JSON events and rejects skipped subcases. Registry/catalogue mismatches fail preflight. Original test entries and standalone scripts are removed.

The `desktop_e2e` build tag imports mygo's opt-in automation plugin, assigns a unique application name and supplies the existing directory clipboard through the host Board seam. Untagged builds use the original system board and do not import automation. The native case executes real desktop view actions against a production service and checks actual directory-clipboard bytes. It does not establish global key delivery, OS focus policy, system clipboard or cross-application paste.

The runner builds from a private frozen snapshot, so edits in the developer checkout do not change an active run. It rejects missing evidence, failed/skipped selected cases and source-snapshot mutation. It publishes the final verdict only after report/checksum generation succeeds. Native windows and services are stopped before private artifacts are removed. See the [E2E contract](../contracts/desktop-e2e-contract.md), [developer guide](../../app/desktop/e2e/README.md), and [execution receipt](../exec/desktop-e2e.md).
