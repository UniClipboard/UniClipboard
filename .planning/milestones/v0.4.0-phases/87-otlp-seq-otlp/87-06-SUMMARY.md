---
phase: 87-otlp-seq-otlp
plan: "06"
subsystem: observability
tags: [otlp, tracing, documentation, seq, w3c-traceparent, otel-semconv]
dependency_graph:
  requires:
    - phase: 87-04
      provides: "Dotted clipboard stage span names, clipboard.flow root span, traceparent inject/extract in sync use cases"
  provides:
    - "logging-architecture.md fully rewritten for OTLP/OTel semantics"
    - "flow-timeline.json Seq signal queries by SpanName/TraceId (not flow_id)"
    - "cross-device-flow.json Seq signal queries by clipboard.flow root span + TraceId"
    - "docker-compose.seq.yml documents OTLP base URL with critical pitfall note"
  affects: [docs, developer-onboarding, seq-visualization]
tech-stack:
  added: []
  patterns:
    - "OTEL_EXPORTER_OTLP_ENDPOINT base URL pattern: /ingest/otlp (no /v1/traces suffix — SDK appends it)"
    - "Seq signal queries use SpanName + TraceId (OTel semantics) instead of flow_id/origin_flow_id (CLEF semantics)"

key-files:
  created: []
  modified:
    - docs/architecture/logging-architecture.md
    - docs/seq/signals/flow-timeline.json
    - docs/seq/signals/cross-device-flow.json
    - docker-compose.seq.yml

key-decisions:
  - "87-06: UC_SEQ_URL documented as removed in Phase 87 with migration note — single controlled occurrence in logging-architecture.md migration note, not a live reference"
  - "87-06: Seq signal JSON files reference 'TraceId'/'SpanName' (PascalCase) matching Seq 2025.2 OTLP UI; _note field documents version uncertainty as MEDIUM confidence requiring live Seq smoke test"
  - "87-06: flow-timeline.json uses SpanName like 'clipboard.%' to group all pipeline spans; cross-device-flow.json uses SpanName = 'clipboard.flow' for root-span-only view with trace drill-down"
  - "87-06: origin_flow_id documented as deprecated in protocol section (Phase 87 tombstone, removal deferred)"

requirements-completed:
  - REQ-87-11
  - REQ-87-12
  - REQ-87-13

duration: 7min
completed: "2026-04-05"
---

# Phase 87 Plan 06: Documentation Rewrite for OTel OTLP Semantics

Rewrote `docs/architecture/logging-architecture.md` from CLEF/UC_SEQ_URL model to OTLP/standard-OTel-env-var model; updated both Seq saved-search signals to query by SpanName/TraceId; added OTLP endpoint comment block to docker-compose.seq.yml.

## Performance

- **Duration:** 7 min
- **Started:** 2026-04-05T04:33:23Z
- **Completed:** 2026-04-05T04:39:37Z
- **Tasks:** 1
- **Files modified:** 4

## Accomplishments

- Replaced the "Seq Integration (Local Visualization)" section with "OpenTelemetry OTLP Integration" covering env vars, base URL pitfall, resource attributes, span topology, activation rules, and Seq query patterns
- Replaced "Cross-Device Tracing" / `origin_flow_id` section with "Distributed Tracing with W3C Trace Context" covering traceparent inject/extract, legacy peer fallback, and protocol field details
- Rewrote both Seq signal JSON files to use OTel `SpanName`/`TraceId` semantics instead of legacy `flow_id`/`origin_flow_id`
- Added OTLP endpoint documentation block to `docker-compose.seq.yml` with the critical base-URL pitfall note

## Task Commits

1. **Task 1: Rewrite logging-architecture.md and Seq signals JSON for OTel semantics** - `d40216db` (docs)

**Plan metadata:** (created below)

## Files Created/Modified

- `docs/architecture/logging-architecture.md` — Full rewrite: OTLP Integration section, W3C Trace Context section, span naming table, updated env vars table, Seq Signals section
- `docs/seq/signals/flow-timeline.json` — Query: `SpanName like 'clipboard.%'`; columns: TraceId, SpanName, service.instance.id
- `docs/seq/signals/cross-device-flow.json` — Query: `SpanName = 'clipboard.flow'`; columns: TraceId, SpanName, origin, service.instance.id
- `docker-compose.seq.yml` — Added OTLP endpoint comment block above `services:` section

## Decisions Made

- **OTEL_EXPORTER_OTLP_ENDPOINT base URL**: Documented the critical pitfall (#7 from RESEARCH.md) that the SDK auto-appends `/v1/traces` and `/v1/logs`. Correct value is `http://localhost:5341/ingest/otlp` — not the full path shown in Seq docs.
- **UC_SEQ_URL migration note**: Kept a single controlled reference in the migration note paragraph (explicitly labelled "removed in Phase 87"). No live references remain.
- **Seq signal property casing**: Used `TraceId`/`SpanName` (PascalCase) matching Seq 2025.2 OTLP UI behavior. Added `_note` field in both JSONs acknowledging MEDIUM confidence — a live Seq smoke test is the final gate (documented as such in RESEARCH.md §7).
- **cross-device-flow.json approach**: Root-span-only filter (`SpanName = 'clipboard.flow'`) lets Seq's native trace view handle the cross-device tree. No custom FieldMapping needed since TraceId links both peers natively under W3C traceparent.

## Deviations from Plan

None — plan executed exactly as written. All acceptance criteria passed on first run.

## Issues Encountered

**flow_id in flow-timeline.json description text:** The initial description string contained "legacy flow_id field" as a migration note. The acceptance criterion `! grep -q 'flow_id' docs/seq/signals/flow-timeline.json` caught this. Removed the reference from the description string — it is not needed for developer understanding. Fixed immediately, no separate commit required.

## Known Stubs

None. The `_note` field in both signal JSON files acknowledges MEDIUM confidence on Seq OTLP property name casing — this is intentional documentation of uncertainty, not a code stub. A live Seq 2025.2 smoke test is the final validation gate (REQ-87-11).

## Self-Check: PASSED

Files exist:
- [x] `docs/architecture/logging-architecture.md` (OTEL_EXPORTER_OTLP_ENDPOINT, traceparent, clipboard.flow, service.instance.id)
- [x] `docs/seq/signals/flow-timeline.json` (no flow_id, TraceId/SpanName present, JSON valid)
- [x] `docs/seq/signals/cross-device-flow.json` (no origin_flow_id, TraceId/SpanName present, JSON valid)
- [x] `docker-compose.seq.yml` (ingest/otlp, 5341, YAML valid)

Commits exist:
- [x] d40216db (Task 1)
