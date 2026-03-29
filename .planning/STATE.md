---
gsd_state_version: 1.0
milestone: v0.4.0
milestone_name: Runtime Mode Separation
status: 'Phase 71 shipped — PR #323'
stopped_at: Completed 71-03-PLAN.md
last_updated: '2026-03-29T02:04:04.638Z'
progress:
  total_phases: 42
  completed_phases: 35
  total_plans: 93
  completed_plans: 90
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-03-09)

**Core value:** Seamless clipboard synchronization across devices -- copy on one, paste on another
**Current focus:** Phase 71 — dual-product-release-pipeline-for-cli-and-app

## Current Position

Phase: 71
Plan: Not started

## Performance Metrics

**Velocity:**

- Total plans completed: 2
- Average duration: 6.5min
- Total execution time: 0.22 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
| ----- | ----- | ----- | -------- |
| 19    | 2     | 13min | 6.5min   |

**Recent Trend:**

- Last 5 plans: 4min, 9min, 2min, 3min, 9min
- Trend: Stable
  | Phase 20 P01 | 2min | 2 tasks | 5 files |
  | Phase 20 P02 | 3min | 2 tasks | 2 files |
  | Phase 20 P03 | 2min | 1 tasks | 2 files |
  | Phase 21 P01 | 9min | 2 tasks | 6 files |
  | Phase 21 P02 | 8min | 2 tasks | 6 files |
  | Phase 22 P01 | 24min | 2 tasks | 8 files |
  | Phase 22 P02 | 5min | 2 tasks | 4 files |
  | Phase 24 P01 | 4min | 2 tasks | 10 files |
  | Phase 24 P02 | 6min | 2 tasks | 9 files |
  | Phase 24 P03 | 10min | 3 tasks | 5 files |
  | Phase 25 P01 | 8min | 2 tasks | 5 files |
  | Phase 25 P02 | 4min | 2 tasks | 4 files |
  | Phase 25 P01 | 8min | 2 tasks | 5 files |
  | Phase 26 P01 | 7min | 3 tasks | 4 files |
  | Phase 26 P02 | 2min | 3 tasks | 3 files |
  | Phase 27 P01 | 5min | 2 tasks | 13 files |
  | Phase 28 P01 | 3min | 2 tasks | 4 files |
  | Phase 28 P02 | 4min | 2 tasks | 4 files |
  | Phase 28 P03 | 7min | 2 tasks | 12 files |
  | Phase 28 P01 | 7min | 2 tasks | 6 files |
  | Phase 28 P02 | 3min | 2 tasks | 6 files |
  | Phase 29 P01 | 4min | 2 tasks | 5 files |
  | Phase 29 P02 | 8min | 2 tasks | 4 files |
  | Phase 30 P01 | -min | - tasks | - files |
  | Phase 30 P02 | 4min | 1 tasks | 6 files |
  | Phase 30 P01 | 3min | 2 tasks | 7 files |
  | Phase 30 P03 | 4min | 3 tasks | 4 files |
  | Phase 30 P04 | 3 | 2 tasks | 4 files |
  | Phase 31 P01 | 4min | 2 tasks | 8 files |
  | Phase 31 P02 | 3 | 2 tasks | 9 files |
  | Phase 31 P03 | 5min | 2 tasks | 12 files |
  | Phase 32.1 P01 | 10min | 2 tasks | 11 files |
  | Phase 32.1 P02 | 11min | 2 tasks | 8 files |
  | Phase 32.1 P03 | 1min | 1 tasks | 1 files |
  | Phase 32 P01 | 5min | 2 tasks | 8 files |
  | Phase 32 P02 | 5 | 2 tasks | 4 files |
  | Phase 32 P03 | 5min | 2 tasks | 3 files |
  | Phase 33 P01 | 13min | 2 tasks | 11 files |
  | Phase 33 P02 | 4min | 2 tasks | 7 files |
  | Phase 33 P03 | 20min | 2 tasks | 12 files |
  | Phase 33 P04 | 3min | 1 tasks | 5 files |
  | Phase 33 P05 | 5min | 2 tasks | 7 files |
  | Phase 33 P06 | 5 | 1 tasks | 3 files |
  | Phase 34 P01 | 19 | 3 tasks | 9 files |
  | Phase 35 P01 | 7min | 1 tasks | 4 files |
  | Phase 35 P02 | 10min | 2 tasks | 2 files |
  | Phase 65 P01 | 5min | 2 tasks | 19 files |
  | Phase 66 P01 | 5 | 1 tasks | 1 files |
  | Phase 66-daemon-dashboard P02 | 18 | 2 tasks | 4 files |
  | Phase 67-setup-filter P02 | 8 | 1 tasks | 3 files |
  | Phase 68-adopt-tauri-sidecar-for-daemon P01 | 5 | 2 tasks | 6 files |
  | Phase 68 P02 | 20 | 2 tasks | 6 files |
  | Phase 69 P01 | 4 | 3 tasks | 2 files |
  | Phase 70 P01 | 8 | 2 tasks | 6 files |
  | Phase 71 P02 | 52s | 1 tasks | 1 files |
  | Phase 71 P01 | 2 | 2 tasks | 4 files |
  | Phase 71 P03 | 2 | 2 tasks | 4 files |

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- 19-02: Used generic impl Layer<S> return types for builder functions to enable caller composition without Box<dyn> type issues.
- 19-02: Re-exported WorkerGuard from uc-observability to avoid adding tracing-appender as direct dependency.
- 19-01: Used JsonFields as field formatter so FlatJsonFormat can extract structured span data from extensions.
- 19-01: Sentry integration excluded from uc-observability to keep zero app-layer dependencies.
- Phase 19: Start observability work by refactoring the tracing subscriber into dual-output profile-driven logging.
- Phase 20: Capture observability uses `flow_id` and `stage` as the canonical clipboard pipeline correlation fields.
- Phase 21: Sync observability must reuse the same flow model as local capture rather than inventing a second tracing pattern.
- Phase 22: Seq remains local and configuration-driven for this milestone; full OTel and multi-backend support stay deferred.
- [Phase 20]: UUID v7 chosen for FlowId (time-ordered) over v4 (random)
- [Phase 20]: Stage constant values are lowercase snake_case matching const names for queryability
- 20-02: Replaced #[tracing::instrument] with manual span to support runtime-computed flow_id field
- 20-02: outbound_sync span carries flow_id but no stage field (Phase 21 adds publish stage)
- [Phase 20]: Split cache_representations into two sequential stage spans (cache_representations + spool_blobs) for distinct observability
- 21-01: origin_flow_id uses serde(default) + skip_serializing_if for zero-cost backward compatibility with older peers
- 22-01: SeqGuard drop uses std::thread::spawn for block_on to avoid runtime-in-runtime panic
- 22-01: SeqLayer implements Layer trait directly rather than using FormatEvent through fmt::layer()
- 22-01: CLEF format has no conflict resolution (simpler than FlatJsonFormat) since it targets Seq only
- 22-02: Seq layer uses Option<Layer> pattern for zero-overhead when disabled
- 22-02: hyper=info and hyper_util=info added to NOISE_FILTERS to suppress Seq HTTP client debug noise
- [Phase 24]: Upsert ON CONFLICT SET excludes sync_settings to avoid overwriting per-device overrides during pairing
- [Phase 24]: serde(default) on sync_settings for backward-compatible deserialization of existing PairedDevice data
- [Phase 24]: Settings loaded from storage each time (not cached) -- SQLite + WAL fast for 2-5 devices
- [Phase 24]: Peers not in paired_device table proceed with sync as safety fallback
- [Phase 24]: Per-device auto_sync filtering applied before ensure_business_path to avoid unnecessary connections
- 24-03: Removed permissions section from DeviceSettingsPanel per user feedback
- 24-03: Content type toggles made non-editable since sync engine filtering not yet implemented
- 25-02: Editable vs coming_soon status field on contentTypeEntries drives badge and interactivity
- 25-02: All-disabled warning uses Object.values().every() on content_types for computed state
- [Phase 25]: ContentTypes::default() fix from derive(Default) all-false to explicit all-true impl
- [Phase 25]: Classify snapshot once before peer loop for efficiency (not per-peer)
- [Phase 25]: Only Text and Image are filterable; unimplemented types always sync
- [Phase 26]: Exposed apply_sync_policy as pub for integration tests in tests/ to validate policy logic directly
- [Phase 26]: Global auto_sync guard executes before per-device evaluation and does not mutate per-device sync settings
- [Phase 26]: Global auto_sync off UX remains explicit-only (auto_sync === false) for banner visibility and disable cascade.
- [Phase 26]: Settings navigation category state is one-shot and cleared after consumption to prevent stale tab forcing.
- [Phase 27]: Used HashMap<String, serde_json::Value> for keyboard_shortcuts for flexible override storage
- [Phase 27]: Used mod prefix for all shortcut definitions for cross-platform compatibility (mod = Cmd on Mac, Ctrl on others)
- [Phase 28]: Used same binary codec pattern as clipboard_payload_v3.rs for FileTransferMessage consistency
- [Phase 28]: Extracted write_string_u16/read_string_u16 helpers for reuse across message variants
- [Phase 28]: Rejected filenames containing '..' anywhere (not just as path component) for extra safety
- [Phase 28]: First non-comment URI line determines file vs link classification per RFC 2483
- [Phase 28]: File category now filterable via ct.file toggle (was always-true)
- [Phase 28]: NoopFileTransportPort stub pattern used at NetworkPorts construction sites for pre-adapter compilation
- [Phase 28]: Manual schema.rs update for file_transfer table since diesel CLI not available
- [Phase 28]: url crate v2 for URL parsing validation instead of regex
- [Phase 28]: ClipboardItemDto.link changed from serde_json::Value to ClipboardLinkItemDto for type safety
- [Phase 28]: URL regex heuristic checks http/https/ftp/ftps/mailto with no-whitespace for text/plain link detection
- [Phase 29]: VerifyKeychainAccess use case takes only KeyScopePort + KeyMaterialPort (lighter than AutoUnlock's 5 ports)
- [Phase 29]: KeyringError mapped to Ok(false) to treat keyring issues as not-granted rather than hard failure
- 29-02: Used regular Button instead of AlertDialogAction for confirm to prevent auto-close on verification failure
- 29-02: Confirm button text changed to "I understand" per user feedback during verification
- [Phase 30]: Used libc::statvfs directly for disk space check instead of adding fs2 dependency
- [Phase 30]: Hash verification failure deletes temp file immediately with no retry policy
- [Phase 30]: Shared sync policy module extracted for reuse between clipboard and file sync
- [Phase 30]: Binary chunk frame format: 4-byte header-length prefix + JSON header + raw chunk data for efficient binary transfer
- [Phase 30]: Queue and retry modules co-created since queue.rs depends on retry.rs
- [Phase 30]: File cache directory derived from storage_paths.cache_dir.join('file-cache') rather than adding to AppConfig
- [Phase 30]: Clone FileTransferService (Arc<Inner>) out of Mutex before await to avoid holding lock across async boundary
- [Phase 30]: Per-peer send failures logged as warnings without aborting transfers to remaining peers
- [Phase 31]: FileContextMenu uses ContextMenuTrigger asChild for zero extra DOM wrappers
- [Phase 31]: Transfer tracking uses Set<string> in ClipboardContent state for transferringEntries
- [Phase 31]: TransferProgressBar uses two variants (compact/detailed) instead of separate components
- [Phase 31]: Transfer-to-entry mapping uses dual Record maps for O(1) lookup in both directions
- [Phase 31]: Notification batching uses 500ms window to coalesce multi-file sync notifications
- [Phase 31]: Error notifications fire immediately without batching for prompt user feedback
- [Phase 31]: Clipboard race handled by cancelClipboardWrite reducer dispatched on clipboard://new-content event
- [Phase 32.1]: CopyFileToClipboardUseCase takes entry_id only, looks up event_id via ClipboardEntryRepositoryPort
- [Phase 32.1]: Batch accumulator lives in event loop outside tokio::spawn for cross-event state coordination
- [Phase 32.1]: Entry persistence always via CaptureClipboardUseCase::execute_with_origin(RemotePush) regardless of clipboard race
- [Phase 32.1]: Added get_representations_for_event to ClipboardRepresentationRepositoryPort with default empty impl
- [Phase 32.1]: Extension-based file icon map uses constant Record lookup for ESLint static-components compliance
- [Phase 32.1]: Lazy stale detection -- staleness only discovered when copyFileToClipboard returns error, not on startup
- [Phase 32.1]: Delete cascade parses inline_data of text/uri-list representations to find and remove cache files
- [Phase 32.1]: ClearClipboardHistory updated with representation_repo for consistent delete cascade
- [Phase 32.1]: Pre-write race check uses consume_origin_or_default(LocalCapture) to detect clipboard activity during transfer
- [Phase 32]: Used separate updateFileSyncSetting context method matching existing FileSyncSettings type at Settings.file_sync
- [Phase 32]: Filesystem-based cleanup instead of DB repository: no FileEntryRepository port exists yet, file-cache directory is source of truth
- [Phase 32]: Cleanup module placed in file_sync/ (not file/) to match existing module naming
- [Phase 32]: Guards return Result errors (not events) since use cases have no event channel access; callers handle event emission
- [Phase 32]: transfer_errors module provides constants and formatters for consistent user-facing error messages
- [Phase 33]: Arc<TrackInboundTransfersUseCase> shared across spawned tasks for durable marking inside async spawns
- [Phase 33]: get_entry_id_for_transfer added to port for transfer_id-only context resolution in progress events
- [Phase 33]: file-transfer:// namespace prefix unifies all file transfer events
- [Phase 33]: tokio::watch for timeout sweep cancellation to keep it simpler than TaskRegistry token
- [Phase 33]: String-based entry_id in FileTransferRepositoryPort to avoid coupling to uc_ids across crate boundaries
- [Phase 33]: NoopFileTransferRepositoryPort stub for compilation before infra adapter lands
- [Phase 33]: Aggregate transfer status priority: failed > transferring > pending > completed
- [Phase 33]: PendingTransferLinkage returned from InboundApplyOutcome for platform layer status emission
- [Phase 33]: Durable entryStatusById separate from ephemeral activeTransfers to survive progress cleanup
- [Phase 33]: Old transfer://progress and transfer://error channels replaced with file-transfer:// namespace
- [Phase 33]: Durable entryStatusById takes priority over ephemeral activeTransfers for all UI state decisions
- [Phase 33]: Hydration dispatch placed inside thunk (not fulfilled reducer) because reducers cannot dispatch actions
- [Phase 33]: fetchClipboardItems filters file_transfer_status != null before building hydrateEntryTransferStatuses payload to avoid seeding null statuses into entryStatusById
- [Phase 34]: useDeviceDiscovery stores raw deviceName (string | null) from backend — no fallback mapping in hook
- [Phase 34]: onError callback stored in useRef synced via useEffect (not during render) to satisfy react-hooks/refs ESLint rule
- [Phase 34]: SetupPage migrated from 3s polling interval to event-driven useDeviceDiscovery hook
- [Phase 34]: Removed headerRight refresh button from JoinPickDeviceStep -- scanning is automatic, header stays clean
- [Phase 34]: AnimatePresence mode=wait for clean phase-to-phase transitions in JoinPickDeviceStep
- [Phase 35]: all_files_excluded guard scoped to file_sync_attempted flag to prevent false suppression when file_sync is disabled
- [Phase 35]: OutboundSyncPlanner plan() infallible: settings failure returns safe defaults (clipboard: Some, files: [])
- [Phase 35]: runtime.rs retains extract_file_paths_from_snapshot() + std::fs::metadata() calls (platform layer owns all fs I/O)
- [Phase 35]: extracted_paths_count captured from resolved_paths.len() BEFORE metadata filter; passed to plan() for all_files_excluded detection
- [Phase 65]: Inlined PlatformEvent (ClipboardChanged only) into clipboard/watcher.rs rather than keeping separate ipc module
- [Phase 66]: clipboard and file-transfer WS topics return Ok(None) from build_snapshot_event — no initial snapshot, matching PAIRING_VERIFICATION/SETUP pattern
- [Phase 66-daemon-dashboard]: bridge_state_monitor uses two boolean flags (has_been_ready, was_degraded) so startup path does not emit reconnect even if it briefly passes through Degraded
- [Phase 66-daemon-dashboard]: DaemonReconnected is ClipboardHostEvent variant (not HostEvent top-level) matching existing clipboard subsystem grouping
- [Phase 66-daemon-dashboard]: daemon://ws-reconnected is a dedicated Tauri channel separate from clipboard://event to avoid conflating reconnect signal with content events
- [Phase 67-setup-filter]: recover_encryption_session made pub so main.rs can call it before DaemonApp construction
- [Phase 67-setup-filter]: Removed recover_encryption_session from DaemonApp::run() — Phase 67 moved it to main.rs for deferred-start logic
- [Phase 67-setup-filter]: RuntimeState::update_service_health() added for single-entry Stopped→Healthy mutation when deferred worker starts
- [Phase 68]: Copy daemon binary before tauri_build::build() so externalBin path validation succeeds at check time
- [Phase 68]: tauri-plugin-shell in workspace.dependencies; build.rs in src-tauri/ (main crate) for TAURI_ENV_TARGET_TRIPLE access
- [Phase 68]: CommandChild from sidecar spawn maintains stdin tether (D-06): drop sends EOF to daemon's --gui-managed stdin monitor
- [Phase 68]: shutdown_owned_daemon uses terminate_local_daemon_pid + libc::kill(0) polling instead of Child::try_wait/kill/wait
- [Phase 68]: Sidecar rx Receiver drained in background task — must not be dropped immediately or pipe blocks
- [Phase 69]: run_new_space() uses build_cli_runtime() directly (no daemon) for first-time encryption init, matching space_status.rs pattern
- [Phase 69]: new_space_encryption_guard() extracted as pub fn for behavioral testability without async runtime
- [Phase 70]: Background start reuses ensure_local_daemon_running() for probe-spawn-poll pattern consistency
- [Phase 70]: SIGKILL not used -- user warned if daemon does not stop within 10s timeout
- [Phase 70]: libc added directly to uc-cli (not workspace) since no other crate needs it
- [Phase 71]: Mirror setup-matrix pattern from build.yml for consistent platform matrix across CLI and app builds
- [Phase 71]: Use cli-{target} artifact prefix to disambiguate from app artifacts in shared release workflows
- [Phase 71]: Cargo.lock refresh for workspace members delegated to cargo update -p uc-cli in CI (not fragile JS regex patching)
- [Phase 71]: cargo update steps in release.yml gated with if: github.event_name == 'workflow_dispatch' to avoid running on tag-push events
- [Phase 71]: CLI artifact collection uses separate find loop with no collision handling — CLI archives have unique names (version + target triple in filename)
- [Phase 71]: buildCliInstallerLines() detects artifacts by uniclipboard-cli- prefix + target triple patterns (aarch64-apple-darwin, x86_64-apple-darwin, linux-gnu, windows-msvc)

### Roadmap Evolution

- Phase 23 added: Distributed tracing with trace view visualization for cross-device observability
- Phase 24 added: Implement per-device sync settings for paired devices
- Phase 25 added: Implement per-device sync content type toggles
- Phase 26 added: Implement global sync master toggle and improve sync UX
- Phase 27 added: 支持快捷键设置在 settings page 中
- Phase 28 split: Original monolithic file sync phase split into 4 phases (28-31)
- Phase 28 updated: File sync foundation — message types, ports, classification fix, schema, settings
- Phase 28 added: Support link content type (MIME link and URL-detected plain text)
- Phase 29 added: Add macOS auto-unlock keychain Always Allow confirmation modal on UnlockPage
- Phase 30 added: File transfer service — chunked protocol, use cases, retry logic
- Phase 31 added: File sync UI — Dashboard file entries, context menu, progress, notifications
- Phase 32 added: File sync settings and polish — settings UI, quota enforcement, auto-cleanup
- Phase 32.1 inserted after Phase 32: Inbound file sync clipboard integration with persistent file URI list for cross-platform paste (URGENT)
- Phase 33 added: Fix file sync eventual consistency - ensure atomic sync with metadata and blob together
- Phase 34 added: Optimize JoinPickDevice page: event-driven discovery with scanning UX
- Phase 35 added: Extract OutboundSyncPlanner to consolidate scattered sync policy checks
- Phase 66 added: 修复 daemon 剪切板监听导致前端 dashboard 不会自动刷新剪切板历史的问题
- Phase 67 added: 设备在 setup 完成前不应被其他设备发现，需要在业务层进行过滤
- Phase 68 added: Adopt Tauri Sidecar for daemon binary management (dev build, bundling, and path resolution)
- Phase 69 added: CLI setup flow: first-time encryption init before daemon spawn
- Phase 70 added: CLI start/stop commands for daemon lifecycle management
- Phase 71 added: Dual-product release pipeline for CLI and App

### Pending Todos

None yet.

### Blockers/Concerns

- Phase 22 likely needs extra validation around CLEF field mapping and Seq waterfall/query behavior.
- Existing `log::*` and `tracing::*` coexistence may need an audit during Phase 19 to avoid mixed-output surprises.

### Quick Tasks Completed

| #   | Description                                                 | Date       | Commit   | Directory                                                                                         |
| --- | ----------------------------------------------------------- | ---------- | -------- | ------------------------------------------------------------------------------------------------- |
| 8   | Fix Vite chunk size warning by code-splitting large bundles | 2026-03-12 | 06d711af | [8-fix-vite-chunk-size-warning-by-code-spli](./quick/8-fix-vite-chunk-size-warning-by-code-spli/) |
| 9   | Optimize stale relative timestamps on clipboard items       | 2026-03-12 | 8a079cb7 | [9-optimize-stale-relative-timestamps-on-cl](./quick/9-optimize-stale-relative-timestamps-on-cl/) |

## Session Continuity

Last session: 2026-03-28T14:15:39.318Z
Stopped at: Completed 71-03-PLAN.md
Resume file: None
