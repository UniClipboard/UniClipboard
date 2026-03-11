# Roadmap: UniClipboard Desktop

## Milestones

- ✅ **v0.1.0 Daily Driver** - Phases 1-9 (shipped 2026-03-06)
- ✅ **v0.2.0 Architecture Remediation** - Phases 10-18 (shipped 2026-03-09)
- 📋 **v0.3.0 Log Observability** - Phases 19-22 (planned)

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

### 📋 v0.3.0 Log Observability (Planned)

**Milestone Goal:** Make the clipboard capture pipeline fully observable with structured logging, dual output, and Seq-based local visualization.

- [x] **Phase 19: Dual Output Logging Foundation** - Establish structured dual-output logging, profiles, and configuration-controlled activation. (completed 2026-03-10)
- [x] **Phase 20: Clipboard Capture Flow Correlation** - Correlate a single clipboard capture across spans, stages, layers, and spawned work. (gap closure in progress) (completed 2026-03-10)
- [ ] **Phase 21: Sync Flow Correlation** - Extend the same flow model to inbound and outbound sync activity on a device.
- [ ] **Phase 22: Seq Local Visualization** - Deliver configurable Seq ingestion and searchable flow visualization for local developer debugging.

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
**Plans:** 1/2 plans executed

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
**Success Criteria** (what must be TRUE):

1. Developers can enable or disable Seq ingestion through configuration and run the app against a local Seq endpoint without code changes.
2. Structured events arrive in Seq in CLEF-compatible form with `flow_id` and `stage` fields preserved.
3. Seq ingestion happens asynchronously with batching so normal application activity does not pause while logs are shipped.
4. Developers can query a single `flow_id` in Seq and see the related capture or sync stages in time order.
5. Local Seq defaults are sensible enough that developers can get observability working with minimal setup, while still supporting explicit endpoint and API key overrides.
   **Plans**: TBD

## Progress

| Phase                                  | Milestone | Plans Complete | Status      | Completed  |
| -------------------------------------- | --------- | -------------- | ----------- | ---------- |
| 1-9                                    | v0.1.0    | 17/17          | Complete    | 2026-03-06 |
| 10-18                                  | v0.2.0    | 22/22          | Complete    | 2026-03-09 |
| 19. Dual Output Logging Foundation     | 2/2       | Complete       | 2026-03-10  | -          |
| 20. Clipboard Capture Flow Correlation | 3/3       | Complete       | 2026-03-10  | -          |
| 21. Sync Flow Correlation              | 1/2       | In Progress    |             | -          |
| 22. Seq Local Visualization            | v0.3.0    | 0/TBD          | Not started | -          |
