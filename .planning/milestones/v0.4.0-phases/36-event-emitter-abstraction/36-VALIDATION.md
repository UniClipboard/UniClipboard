---
phase: 36
slug: event-emitter-abstraction
status: ready
nyquist_compliant: true
wave_0_complete: true
created: 2026-03-17
---

# Phase 36 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                               |
| ---------------------- | --------------------------------------------------- |
| **Framework**          | cargo test (Rust)                                   |
| **Config file**        | `src-tauri/Cargo.toml`                              |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-core -p uc-tauri` |
| **Full suite command** | `cd src-tauri && cargo test`                        |
| **Estimated runtime**  | ~30 seconds                                         |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-core -p uc-tauri`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `$gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement       | Test Type       | Automated Command                                           | Coverage                                                                                                                                                                                                                                                               | Status   |
| -------- | ---- | ---- | ----------------- | --------------- | ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| 36-01-01 | 01   | 1    | EVNT-01           | unit            | `cd src-tauri && cargo test -p uc-core host_event`          | `ports::host_event_emitter::tests::host_event_port_accepts_all_in_scope_events_without_infra_types`                                                                                                                                                                    | ✅ green |
| 36-01-02 | 01   | 1    | EVNT-02 / EVNT-03 | contract + unit | `cd src-tauri && cargo test -p uc-tauri host_event_emitter` | 9 个 Tauri 契约测试 + `test_logging_emitter_always_returns_ok` + `test_logging_emitter_writes_structured_tracing_fields`                                                                                                                                               | ✅ green |
| 36-02-01 | 02   | 2    | EVNT-04           | integration     | `cd src-tauri && cargo test -p uc-tauri host_event_emitter` | `runtime_emits_clipboard_new_content_via_host_event_emitter`、`runtime_event_emitter_can_be_swapped_after_setup`、`clipboard_receive_loop_emits_inbound_error_via_host_event_emitter`                                                                                  | ✅ green |
| 36-02-02 | 02   | 2    | EVNT-04           | integration     | `cd src-tauri && cargo test -p uc-tauri`                    | `peer_name_updated_emits_frontend_event`、`peer_discovery_events_emit_frontend_event`、`peer_connection_events_emit_via_host_event_emitter`、`transfer_progress_events_emit_via_host_event_emitter`、`emit_pending_status_emits_one_status_changed_event_per_transfer` | ✅ green |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Justification

Wave 0 gaps are now closed inline by tests added to the phase-owned Rust modules:

- `src-tauri/crates/uc-core/src/ports/host_event_emitter.rs` locks the pure-Rust port/event surface in `uc-core`
- `src-tauri/crates/uc-tauri/src/adapters/host_event_emitter.rs` verifies Tauri wire contracts and LoggingEventEmitter structured tracing output
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` verifies clipboard watcher emission and post-setup emitter swap semantics
- `src-tauri/crates/uc-tauri/src/bootstrap/wiring.rs` verifies migrated background-loop emit paths use `HostEventEmitterPort`
- `src-tauri/crates/uc-tauri/src/bootstrap/file_transfer_wiring.rs` verifies `emit_pending_status` emits one status event per pending linkage

No separate Wave 0 plan is required.

---

## Manual-Only Verifications

| Behavior                                                   | Requirement | Why Manual                       | Test Instructions                                                                 |
| ---------------------------------------------------------- | ----------- | -------------------------------- | --------------------------------------------------------------------------------- |
| GUI app receives clipboard events after real Tauri startup | EVNT-04     | Requires real GUI lifecycle      | 1. `bun tauri dev` 2. Copy local text 3. Verify Dashboard refreshes immediately   |
| Cross-device sync emits peer/file events in a live session | EVNT-04     | Requires multi-device networking | 1. Run two peers 2. Trigger discovery and file transfer 3. Verify frontend events |

---

## Validation Sign-Off

- [x] All tasks have automated verification
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covered inline by phase-owned tests
- [x] No watch-mode flags
- [x] Feedback latency < 30s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

## Validation Audit 2026-03-17

| Metric     | Count |
| ---------- | ----- |
| Gaps found | 4     |
| Resolved   | 4     |
| Escalated  | 0     |
