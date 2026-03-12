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
- [x] **FLOW-05**: Sync outbound and inbound clipboard flows use the same `flow_id` and `stage` pattern, enabling end-to-end tracing of sync operations on a single device.

### Seq Integration

- [x] **SEQ-01**: The application can send structured log events to a local Seq instance via HTTP in CLEF-compatible JSON format.
- [x] **SEQ-02**: Seq integration is implemented as a dedicated tracing Layer that can be enabled or disabled via configuration without code changes.
- [x] **SEQ-03**: The Seq Layer batches events and flushes asynchronously so that log ingestion does not block the main application execution path.
- [x] **SEQ-04**: Events ingested into Seq include `flow_id` and `stage` fields, allowing developers to query and follow a single clipboard capture or sync flow.
- [x] **SEQ-05**: Seq configuration (endpoint URL and API key, if needed) can be set via configuration (env var or settings) with sensible defaults for local development.
- [x] **SEQ-06**: Seq displays clipboard capture flows as time-ordered sequences of stages for a given `flow_id`, either via trace/waterfall view or equivalent queryable structure.

### Content Type Sync Filtering

- [x] **CT-01**: Clipboard snapshots are classified into a content type category (text, image, rich_text, link, file, code_snippet, unknown) based on the primary MIME type of their representations.
- [x] **CT-02**: The outbound sync engine filters peers by content type toggle in addition to auto_sync, skipping peers whose content type is disabled for the snapshot being synced.
- [x] **CT-03**: Unknown or unimplemented content types (rich_text, link, file, code_snippet) always sync regardless of toggle state.
- [x] **CT-04**: ContentTypes defaults to all-true so new devices sync all content by default.
- [x] **CT-05**: Text and image content type toggles are interactive in the DeviceSettingsPanel when auto_sync is enabled.
- [x] **CT-06**: Unimplemented content type toggles (file, link, code_snippet, rich_text) display a "Coming Soon" badge and are non-interactive.
- [x] **CT-07**: An inline warning appears when auto_sync is on but all content types are disabled for a device.

### Global Sync Master Toggle

- [x] **GSYNC-01**: When global auto_sync is false, apply_sync_policy returns an empty peer list before any per-device evaluation, preventing all outbound sync.
- [x] **GSYNC-02**: Per-device sync settings are never modified when the global auto_sync toggle changes; they persist unchanged in storage and resume automatically when re-enabled.
- [ ] **GSYNC-03**: A warning banner appears at the top of the Devices page when global auto_sync is off, with a "Go to Settings" link that navigates directly to the Settings Sync section.
- [ ] **GSYNC-04**: All interactive controls in DeviceSettingsPanel (per-device auto_sync, content type toggles, restore defaults) are cascade-disabled when global auto_sync is off, while preserving their visual on/off state.
- [x] **GSYNC-05**: The auto sync toggle description in Settings displays master switch copy in both EN and ZH locales using the existing i18n infrastructure.

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
| In-app log viewer UI                                 | High effort, low value compared to Seq's dedicated log UI; would duplicate existing tooling.                                                 |
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
| FLOW-05     | Phase 21 | Complete |
| SEQ-01      | Phase 22 | Complete |
| SEQ-02      | Phase 22 | Complete |
| SEQ-03      | Phase 22 | Complete |
| SEQ-04      | Phase 22 | Complete |
| SEQ-05      | Phase 22 | Complete |
| SEQ-06      | Phase 22 | Complete |
| CT-01       | Phase 25 | Planned  |
| CT-02       | Phase 25 | Planned  |
| CT-03       | Phase 25 | Planned  |
| CT-04       | Phase 25 | Planned  |
| CT-05       | Phase 25 | Planned  |
| CT-06       | Phase 25 | Planned  |
| CT-07       | Phase 25 | Planned  |
| GSYNC-01    | Phase 26 | Planned  |
| GSYNC-02    | Phase 26 | Planned  |
| GSYNC-03    | Phase 26 | Planned  |
| GSYNC-04    | Phase 26 | Planned  |
| GSYNC-05    | Phase 26 | Planned  |

**Coverage:**

- v1 requirements: 27 total
- Mapped to phases: 27
- Unmapped: 0

---

_Requirements defined: 2026-03-09_
_Last updated: 2026-03-12 after Phase 26 planning_
