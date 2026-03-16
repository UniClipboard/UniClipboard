# Roadmap: UniClipboard Desktop

## Milestones

- ✅ **v0.1.0 Daily Driver** - Phases 1-9 (shipped 2026-03-06)
- ✅ **v0.2.0 Architecture Remediation** - Phases 10-18 (shipped 2026-03-09)
- 📋 **v0.3.0 Log Observability** - Phases 19-23 (in progress)

## Phases

<details>
<summary>✅ v0.1.0 Daily Driver (Phases 1-9) - SHIPPED 2026-03-06</summary>

See: `.planning/milestones/v0.1.0-ROADMAP.md`

</details>

<details>
<summary>✅ v0.2.0 Architecture Remediation (Phases 10-18) - SHIPPED 2026-03-09</summary>

See: `.planning/milestones/v0.2.0-ROADMAP.md`

- [x] Phase 10: Boundary Repair Baseline (3/3 plans)
- [x] Phase 11: Command Contract Hardening (2/2 plans)
- [x] Phase 12: Lifecycle Governance Baseline (2/2 plans)
- [x] Phase 13: Responsibility Decomposition & Testability (3/3 plans)
- [x] Phase 14: Lifecycle DTO Frontend Integration (2/2 plans)
- [x] Phase 15: Clipboard Management Command Wiring (3/3 plans)
- [x] Phase 16: Dashboard Refresh Optimization (2/2 plans)
- [x] Phase 17: Dynamic Theme Switching (2/2 plans)
- [x] Phase 18: Chunked Transfer and Resume (3/3 plans — partial, known gaps)

</details>

### 📋 v0.3.0 Log Observability (In Progress)

**Milestone Goal:** Make the clipboard capture pipeline fully observable with structured logging, dual output, Seq-based local visualization, and cross-device tracing.

- [x] **Phase 19: Dual Output Logging Foundation** - Establish structured dual-output logging, profiles, and configuration-controlled activation. (completed 2026-03-10)
- [x] **Phase 20: Clipboard Capture Flow Correlation** - Correlate a single clipboard capture across spans, stages, layers, and spawned work. (gap closure in progress) (completed 2026-03-10)
- [x] **Phase 21: Sync Flow Correlation** - Extend the same flow model to inbound and outbound sync activity on a device. (completed 2026-03-11)
- [x] **Phase 22: Seq Local Visualization** - Deliver configurable Seq ingestion and searchable flow visualization for local developer debugging. (completed 2026-03-11)
- [ ] **Phase 23: Distributed Tracing** - Enable cross-device tracing with device_id injection and Seq saved searches. (in progress)

## Phase Details

### Phase 19: Dual Output Logging Foundation

**Goal**: Developers can run the app with one tracing setup that emits human-readable console logs and machine-readable JSON logs using selectable profiles.
**Depends on**: Phase 18
**Requirements**: LOG-01, LOG-02, LOG-03, LOG-04
**Plans:** 2/2 plans complete

Plans:

- [ ] 19-01-PLAN.md — Create uc-observability crate with LogProfile, FlatJsonFormat, and dual-layer init
- [ ] 19-02-PLAN.md — Integrate into app, Sentry wiring, legacy cleanup, documentation update

**Success Criteria** (what must be TRUE):

1. Developers can start the app and simultaneously see pretty console logs and structured JSON log records generated from the same tracing pipeline.
2. Developers can choose `dev`, `prod`, or `debug_clipboard` logging behavior via configuration without changing code.
3. JSON log records include active span data and inherited parent span fields so correlated identifiers remain visible on each event.
4. Developers can discover how to select log profiles and outputs from milestone documentation or configuration guidance.

### Phase 20: Clipboard Capture Flow Correlation

**Goal**: Developers can trace one clipboard capture from detection through persistence and publish using a single correlated flow record.
**Depends on**: Phase 19
**Requirements**: FLOW-01, FLOW-02, FLOW-03, FLOW-04
**Plans:** 3/3 plans complete

Plans:

- [x] 20-01-PLAN.md — FlowId newtype, stage constants, and dependency wiring in uc-observability
- [x] 20-02-PLAN.md — Instrument runtime and capture use case with flow_id and stage spans
- [ ] 20-03-PLAN.md — Gap closure: add spool_blobs stage span (FLOW-03 partial fix)

**Success Criteria** (what must be TRUE):

1. Each clipboard capture starts with a unique `flow_id` at the platform entry point and that identifier remains attached to the root capture span.
2. Developers can inspect logs for one clipboard capture and see the same `flow_id` across detect, normalize, persist_event, select_policy, persist_entry, spool_blobs, and publish stages.
3. Each major capture step appears as a named span with a `stage` field, making pipeline progress readable in structured logs.
4. Work that crosses platform, app, and infra boundaries — including spawned async tasks — preserves the same flow context instead of breaking correlation.

### Phase 21: Sync Flow Correlation

**Goal**: Developers can follow inbound and outbound sync operations with the same flow conventions used by local clipboard capture.
**Depends on**: Phase 20
**Requirements**: FLOW-05
**Plans:** 2/2 plans complete

Plans:

- [ ] 21-01-PLAN.md — Add sync stage constants and origin_flow_id to ClipboardMessage
- [ ] 21-02-PLAN.md — Instrument outbound and inbound sync spans with flow_id and stage fields

**Success Criteria** (what must be TRUE):

1. Outbound sync activity emits spans and events that use the same `flow_id` and `stage` structure as local capture flows.
2. Inbound sync activity emits spans and events that use the same `flow_id` and `stage` structure as local capture flows.
3. Developers can review logs on a single device and follow sync-specific stages without learning a second observability model.

### Phase 22: Seq Local Visualization

**Goal**: Developers can stream structured events into a local Seq instance and query a single flow as an ordered sequence of stages.
**Depends on**: Phase 21
**Requirements**: SEQ-01, SEQ-02, SEQ-03, SEQ-04, SEQ-05, SEQ-06
**Plans:** 2/2 plans complete

Plans:

- [ ] 22-01-PLAN.md — CLEFFormat formatter, shared span-field extraction, Seq sender/layer/builder in uc-observability
- [ ] 22-02-PLAN.md — Wire Seq layer into bootstrap, docker-compose, documentation, end-to-end verification

**Success Criteria** (what must be TRUE):

1. Developers can enable or disable Seq ingestion through configuration and run the app against a local Seq endpoint without code changes.
2. Structured events arrive in Seq in CLEF-compatible form with `flow_id` and `stage` fields preserved.
3. Seq ingestion happens asynchronously with batching so normal application activity does not pause while logs are shipped.
4. Developers can query a single `flow_id` in Seq and see the related capture or sync stages in time order.
5. Local Seq defaults are sensible enough that developers can get observability working with minimal setup, while still supporting explicit endpoint and API key overrides.

### Phase 23: Distributed Tracing with Trace View Visualization

**Goal:** Enable cross-device tracing by injecting device_id into every Seq event and providing Seq saved searches for flow correlation across devices.
**Depends on:** Phase 22
**Plans:** 2/2 plans complete

Plans:

- [x] 23-01-PLAN.md — Inject device_id into SeqLayer, early resolution from device_id.txt, update docker-compose for LAN access
- [x] 23-02-PLAN.md — Create Seq signal configs, add graceful degradation warning, extend documentation

**Success Criteria** (what must be TRUE):

1. Every CLEF event sent to Seq includes device_id field from the sending device.
2. Developers can query Seq for all events from a specific device using device_id field.
3. Developers can query Seq for cross-device flows by filtering on flow_id OR origin_flow_id.
4. Seq is accessible from LAN devices for cross-device testing (docker-compose binds to 0.0.0.0).
5. Older peer messages without origin_flow_id are handled gracefully with warning logs.

## Progress

| Phase                                  | Milestone | Plans Complete | Status     | Completed  |
| -------------------------------------- | --------- | -------------- | ---------- | ---------- |
| 1-9                                    | v0.1.0    | 17/17          | Complete   | 2026-03-06 |
| 10-18                                  | v0.2.0    | 22/22          | Complete   | 2026-03-09 |
| 19. Dual Output Logging Foundation     | v0.3.0    | 2/2            | Complete   | 2026-03-10 |
| 20. Clipboard Capture Flow Correlation | v0.3.0    | 3/3            | Complete   | 2026-03-10 |
| 21. Sync Flow Correlation              | v0.3.0    | 2/2            | Complete   | 2026-03-11 |
| 22. Seq Local Visualization            | v0.3.0    | 2/2            | Complete   | 2026-03-11 |
| 23. Distributed Tracing                | v0.3.0    | 2/2            | Complete   | 2026-03-11 |
| 24. Per-device Sync Settings           | -         | Complete       | 2026-03-11 | 2026-03-11 |

### Phase 24: Implement per-device sync settings for paired devices

**Goal:** Users can configure sync settings on a per-device basis for each paired device, with per-device overrides and global fallback, affecting actual sync behavior.
**Requirements**: DEVSYNC-01, DEVSYNC-02, DEVSYNC-03, DEVSYNC-04, DEVSYNC-05
**Depends on:** Phase 23
**Plans:** 3/3 plans complete

Plans:

- [x] 24-01-PLAN.md — Domain model extension, DB migration, repository update for per-device sync settings
- [x] 24-02-PLAN.md — Use cases, Tauri commands, and sync engine integration
- [x] 24-03-PLAN.md — Frontend API, Redux thunks, and DeviceSettingsPanel wiring

**Success Criteria** (what must be TRUE):

1. Each paired device can store its own sync settings or inherit global defaults.
2. The outbound sync engine checks per-device auto_sync before sending clipboard data.
3. Users can view, modify, and reset per-device sync settings through the UI.
4. Settings changes take effect immediately without app restart.
5. New devices default to global settings when first paired.

### Phase 25: Implement per-device sync content type toggles

**Goal:** Users can control which content types (text, image) sync to each paired device, with the sync engine filtering outbound content by type and the UI providing interactive toggles for implemented types.
**Requirements**: CT-01, CT-02, CT-03, CT-04, CT-05, CT-06, CT-07
**Depends on:** Phase 24
**Plans:** 2/2 plans complete

Plans:

- [ ] 25-01-PLAN.md — Backend content type classification and sync policy filtering
- [ ] 25-02-PLAN.md — Frontend content type toggle interactivity and visual states

**Success Criteria** (what must be TRUE):

1. Clipboard snapshots are classified by primary content type from MIME data.
2. Outbound sync filters peers by both auto_sync and content type toggles in a single pass.
3. Unknown/unimplemented content types always sync regardless of toggle state.
4. Text and image toggles are interactive in the UI; other types show "Coming Soon".
5. All-disabled warning appears when auto_sync is on but all content types are off.
6. ContentTypes defaults to all-true so new devices sync everything by default.

### Phase 26: Implement global sync master toggle and improve sync UX

**Goal:** The global auto_sync toggle acts as a true master switch that overrides all per-device sync settings. When off, all outbound sync stops. Per-device settings are preserved and resume when re-enabled. The Devices page shows a warning banner with navigation to Settings, and all device controls cascade-disable.
**Requirements**: GSYNC-01, GSYNC-02, GSYNC-03, GSYNC-04, GSYNC-05
**Depends on:** Phase 25
**Plans:** 2/2 plans complete

Plans:

- [x] 26-01-PLAN.md — Backend global auto_sync guard in sync engine + i18n keys and description copy
- [x] 26-02-PLAN.md — Frontend warning banner, cascade disable, and Settings navigation

### Phase 27: Keyboard Shortcuts Settings

**Goal:** Users can view, customize, and reset keyboard shortcuts from a dedicated Settings section, with click-to-record key capture, real-time conflict detection, and immediate effect on active shortcuts.
**Requirements**: KB-01, KB-02, KB-03, KB-04, KB-05, KB-06, KB-07
**Depends on:** Phase 26
**Plans:** 1/2 plans executed

Plans:

- [x] 27-01-PLAN.md — Backend/frontend Settings types, activate definitions, ShortcutsSection display UI
- [ ] 27-02-PLAN.md — Key recording, conflict detection, persistence, reset, and live override wiring

### Phase 28: Support link content type (MIME link and URL-detected plain text)

**Goal:** Link content type is fully functional across the pipeline: classification detects both text/uri-list and single-URL plain text, sync filtering respects the link toggle, and the Dashboard displays links with clickable URLs and domain information.
**Requirements**: LINK-01, LINK-02, LINK-03, LINK-04, LINK-05, LINK-06, LINK-07
**Depends on:** Phase 25
**Plans:** 2/2 plans complete

Plans:

- [ ] 28-01-PLAN.md — Backend link detection, classification, utility functions, and DTO population
- [ ] 28-02-PLAN.md — Frontend link display, multi-URL rendering, and sync toggle activation

**Success Criteria** (what must be TRUE):

1. text/uri-list MIME snapshots and single-URL plain text are both classified as Link.
2. Sync filtering respects the ct.link toggle for Link content.
3. get_clipboard_item returns structured link data (urls + domains) for link entries.
4. Dashboard list view shows clickable URLs with +N more badge for multi-URL entries.
5. Dashboard detail panel shows all URLs with domain names.
6. Link sync toggle is interactive in DeviceSettingsPanel (not "Coming Soon").
7. Mixed text content (e.g., "see https://...") remains classified as Text.

### Phase 29: Add macOS auto-unlock keychain Always Allow confirmation modal on UnlockPage

**Goal:** When macOS users toggle auto-unlock ON, a confirmation modal guides them through granting "Always Allow" in the Keychain popup and verifies the permission was granted before enabling auto-unlock. Non-macOS platforms skip the modal entirely.
**Requirements**: KC-01, KC-02, KC-03, KC-04, KC-05, KC-06
**Depends on:** Phase 25
**Plans:** 2/2 plans complete

Plans:

- [ ] 29-01-PLAN.md — Backend VerifyKeychainAccess use case, Tauri command, and runtime wiring
- [ ] 29-02-PLAN.md — Frontend API, AlertDialog modal, i18n strings, and UnlockPage integration

**Success Criteria** (what must be TRUE):

1. A dedicated `verify_keychain_access` Tauri command checks if Keychain Always Allow is granted by calling `load_kek()`.
2. macOS users see a step-by-step modal when toggling auto-unlock ON; the switch stays OFF until verification passes.
3. The modal shows a red error on verification failure and stays open for retry.
4. Cancel closes the modal with no side effects (switch stays OFF).
5. Windows/Linux users can toggle auto-unlock ON directly without any modal.
6. All modal strings are internationalized in both en-US and zh-CN locales.

### Phase 28: File sync foundation — message types, ports, classification fix, schema, settings

**Goal:** Establish the file sync foundation: define file transfer message types, create FileTransportPort trait, fix file classification (file:// vs http:// in content type filter), add database schema for file entries, and extend settings model with file sync fields.
**Requirements**: FSYNC-FOUNDATION
**Depends on:** Phase 27
**Plans:** 3/3 plans complete

Plans:

- [x] TBD (run /gsd:plan-phase 28 to break down) (completed 2026-03-13)

### Phase 30: File transfer service — chunked protocol, use cases, retry logic

**Goal:** Implement the FileTransferService with libp2p stream protocol, chunked file transfer with Blake3 hash verification, send/receive use cases, serial queue for multi-file operations, and auto-retry with exponential backoff.
**Requirements**: FSYNC-TRANSFER
**Depends on:** Phase 28
**Plans:** 4/4 plans complete

Plans:

- [x] TBD (run /gsd:plan-phase 30 to break down) (completed 2026-03-13)
- [ ] 30-04-PLAN.md — Gap closure: wire FileTransferService in bootstrap and activate transport calls in SyncOutboundFileUseCase

### Phase 31: File sync UI — Dashboard file entries, context menu, progress, notifications

**Goal:** Add file entries to Dashboard clipboard history with right-click context menu (Copy / Sync to Clipboard), progress indicators for file transfers, system notification merging for multi-file batches, and error feedback display.
**Requirements**: FSYNC-UI
**Depends on:** Phase 30
**Plans:** 3/3 plans complete

Plans:

- [x] TBD (run /gsd:plan-phase 31 to break down) (completed 2026-03-13)

### Phase 32: File sync settings and polish — settings UI, quota enforcement, auto-cleanup

**Goal:** Add file sync settings UI (enable toggle, thresholds, quotas), enforce per-device file cache quotas, implement auto-cleanup of expired temp files, and polish error handling across the file sync pipeline.
**Requirements**: FSYNC-POLISH
**Depends on:** Phase 31
**Plans:** 3/3 plans complete

Plans:

- [x] TBD (run /gsd:plan-phase 32 to break down) (completed 2026-03-14)

### Phase 32.1: Inbound file sync clipboard integration with persistent file URI list for cross-platform paste (INSERTED)

**Goal:** Integrate received files with system clipboard so users can paste (Cmd+V / Ctrl+V) in Finder/Explorer after file transfer completes. Includes auto-write on transfer completion, manual copy from Dashboard with file validation, persistent file URI storage, stale file handling, and delete cascade for cache files.
**Requirements**: FCLIP-01, FCLIP-02, FCLIP-03, FCLIP-04, FCLIP-05, FCLIP-06, FCLIP-07, FCLIP-08, FCLIP-09, FCLIP-10
**Depends on:** Phase 31
**Plans:** 3/3 plans complete

Plans:

- [x] 32.1-01-PLAN.md — Backend clipboard write on file transfer complete, CopyFileToClipboardUseCase, Tauri command
- [x] 32.1-02-PLAN.md — Frontend file entry display with extension icons, stale file styling, delete cascade
- [ ] 32.1-03-PLAN.md — Gap closure: clipboard race detection in write_file_to_clipboard_after_transfer (FCLIP-03)

### Phase 33: Fix file sync eventual consistency - ensure atomic sync with metadata and blob together

**Goal:** Make receiver-side file sync atomic from the user's perspective by durably tracking metadata and blob lifecycle together. File entries may appear immediately on metadata receipt, but they must surface truthful `pending / transferring / completed / failed` state, enforce the locked timeout budgets, clean failed partial files, and survive restart via persisted transfer status.
**Requirements**: FSYNC-CONSISTENCY
**Depends on:** Phase 32
**Plans:** 6/6 plans complete

Plans:

- [ ] 33-01-PLAN.md — Core/app transfer tracking contract, metadata seeding, projection aggregation
- [ ] 33-02-PLAN.md — Infra schema upgrade and file transfer repository adapter
- [ ] 33-03-PLAN.md — Tauri/runtime wiring, timeout sweeps, cleanup, and status event emission
- [ ] 33-04-PLAN.md — Frontend data hydration for durable transfer status
- [ ] 33-05-PLAN.md — Frontend UI rendering and action gating for file transfer states
- [ ] 33-06-PLAN.md — Gap closure: wire API hydration to Redux and add camelCase serialization test

### Phase 34: Optimize JoinPickDevice page: event-driven discovery with scanning UX

**Goal:** Replace 3-second polling with event-driven device discovery using existing backend events, and transform the JoinPickDevice step into a Bluetooth/AirDrop-like scanning experience with pulse animation, animated device list, and troubleshooting empty state.
**Requirements**: SCAN-01, SCAN-02, SCAN-03, SCAN-04, SCAN-05, SCAN-06
**Depends on:** Phase 33
**Plans:** 2/2 plans complete

Plans:

- [ ] 34-01-PLAN.md — Create useDeviceDiscovery hook, update types, add CSS ripple animation and i18n keys
- [ ] 34-02-PLAN.md — Wire hook into SetupPage, rebuild JoinPickDeviceStep UI with scanning phases, rewrite tests

### Phase 35: Extract OutboundSyncPlanner to consolidate scattered sync policy checks

**Goal:** Consolidate all outbound sync eligibility decisions (settings load, content type classification, file extraction, size filtering, all_files_excluded guard) from three scattered stages in on_clipboard_changed() into a single OutboundSyncPlanner::plan() call that produces an OutboundSyncPlan, making the runtime a thin dispatcher with no inline policy logic.
**Requirements**: SYNCPLAN-01, SYNCPLAN-02, SYNCPLAN-03, SYNCPLAN-04
**Depends on:** Phase 34
**Plans:** 2/2 plans complete

Plans:

- [ ] 35-01-PLAN.md — Define OutboundSyncPlan types and implement OutboundSyncPlanner with TDD
- [ ] 35-02-PLAN.md — Wire planner into runtime.rs and remove redundant guards from SyncOutboundFileUseCase
