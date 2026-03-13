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
