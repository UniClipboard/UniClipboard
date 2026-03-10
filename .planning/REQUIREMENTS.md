# Requirements: UniClipboard Desktop

**Defined:** 2026-03-09
**Core Value:** Seamless clipboard synchronization across devices — users can copy on one device and paste on another without interrupting their workflow

## v1 Requirements

Requirements for v0.3.0 Log Observability. Each will be mapped to roadmap phases.

### Logging Foundation

- [x] **LOG-01**: Application emits logs to both pretty-formatted console (for developers) and JSON file (for tools) using a single shared tracing subscriber.
- [x] **LOG-02**: JSON log output includes current span context and parent span fields (e.g., `flow_id`, `stage`, `entry_id`) for every event.
- [x] **LOG-03**: The logging subsystem supports three log profiles — `dev`, `prod`, and `debug_clipboard` — each with clearly defined filter levels and output targets.
- [x] **LOG-04**: Log profile selection is controlled via configuration (env var or settings) and is documented for developers.

### Flow Observability

- [x] **FLOW-01**: Each clipboard capture flow is assigned a unique `flow_id` at the platform entry point and this `flow_id` is attached to the root span.
- [x] **FLOW-02**: All spans and events participating in a clipboard capture flow (from detection through normalize, persist, and publish) carry the same `flow_id` field.
- [x] **FLOW-03**: Each major step of the capture pipeline (detect, normalize, persist_event, select_policy, persist_entry, spool_blobs, publish) is represented by a named span with a `stage` field.
- [x] **FLOW-04**: Cross-layer operations within a capture flow (platform, app, infra) preserve `flow_id` and `stage` context, including across `tokio::spawn` boundaries.
- [ ] **FLOW-05**: Sync outbound and inbound clipboard flows use the same `flow_id` and `stage` pattern, enabling end-to-end tracing of sync operations on a single device.

### Seq Integration

- [ ] **SEQ-01**: The application can send structured log events to a local Seq instance via HTTP in CLEF-compatible JSON format.
- [ ] **SEQ-02**: Seq integration is implemented as a dedicated tracing Layer that can be enabled or disabled via configuration without code changes.
- [ ] **SEQ-03**: The Seq Layer batches events and flushes asynchronously so that log ingestion does not block the main application execution path.
- [ ] **SEQ-04**: Events ingested into Seq include `flow_id` and `stage` fields, allowing developers to query and follow a single clipboard capture or sync flow.
- [ ] **SEQ-05**: Seq configuration (endpoint URL and API key, if needed) can be set via configuration (env var or settings) with sensible defaults for local development.
- [ ] **SEQ-06**: Seq displays clipboard capture flows as time-ordered sequences of stages for a given `flow_id`, either via trace/waterfall view or equivalent queryable structure.

## v2 Requirements

Deferred to a future milestone. Tracked but not in the current roadmap.

### Advanced Observability

- **OBS-01**: Log profile can be switched at runtime (without app restart) using a supported control surface.
- **OBS-02**: Representation-level spans include `representation_id`, `mime_type`, and `size_bytes` fields for each normalized representation.
- **OBS-03**: Metrics (e.g., captures per minute, average capture duration) are exposed via a standard metrics backend.

## Out of Scope

Explicitly excluded from v0.3.0. Documented to prevent scope creep.

| Feature                                              | Reason                                                                                                                                       |
| ---------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| Full OpenTelemetry integration (traces/logs/metrics) | Adds significant dependency and configuration complexity; reserved for a later milestone focused on multi-backend and collector integration. |
| Remote/cloud log shipping (Datadog/Honeycomb/etc.)   | Clipboard logs may contain sensitive content; current milestone is local developer observability only.                                       |
| React/frontend log integration into Rust tracing     | Cross-boundary logging adds complexity; frontend debugging handled via browser/React DevTools and Redux tooling.                             |
| In-app log viewer UI                                 | High effort, low value compared to Seq’s dedicated log UI; would duplicate existing tooling.                                                 |
| Metrics dashboards and alerting rules                | Belong to a separate observability/metrics milestone; this milestone focuses on structured logs and flows.                                   |
| Distributed tracing across devices                   | Requires protocol-level changes for trace context propagation; current milestone limits scope to single-device flows.                        |

## Traceability

Which phases cover which requirements.

| Requirement | Phase    | Status   |
| ----------- | -------- | -------- |
| LOG-01      | Phase 19 | Complete |
| LOG-02      | Phase 19 | Complete |
| LOG-03      | Phase 19 | Complete |
| LOG-04      | Phase 19 | Complete |
| FLOW-01     | Phase 20 | Complete |
| FLOW-02     | Phase 20 | Complete |
| FLOW-03     | Phase 20 | Complete |
| FLOW-04     | Phase 20 | Complete |
| FLOW-05     | Phase 21 | Pending  |
| SEQ-01      | Phase 22 | Pending  |
| SEQ-02      | Phase 22 | Pending  |
| SEQ-03      | Phase 22 | Pending  |
| SEQ-04      | Phase 22 | Pending  |
| SEQ-05      | Phase 22 | Pending  |
| SEQ-06      | Phase 22 | Pending  |

**Coverage:**

- v1 requirements: 15 total
- Mapped to phases: 15
- Unmapped: 0 ✓

---

_Requirements defined: 2026-03-09_
_Last updated: 2026-03-09 after roadmap creation_
