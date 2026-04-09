---
phase: 87
slug: otlp-seq-otlp
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-04-04
---

# Phase 87 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution. Detailed test mapping is authored by the planner from RESEARCH.md § Validation Architecture.

---

## Test Infrastructure

| Property               | Value                                                                     |
| ---------------------- | ------------------------------------------------------------------------- |
| **Framework**          | Rust `cargo test` (unit + integration) — workspace-level                  |
| **Config file**        | `src-tauri/Cargo.toml` (workspace + crate manifests)                      |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-observability`                          |
| **Full suite command** | `cd src-tauri && cargo test`                                              |
| **Estimated runtime**  | ~30s (uc-observability only) · ~3–5 min (workspace)                       |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p <crate_under_change>` for the crate touched by the task (uc-observability / uc-core / uc-tauri / uc-app)
- **After every plan wave:** Run `cd src-tauri && cargo test` (full workspace)
- **Before `/gsd:verify-work`:** Full suite green + one manual Seq visibility smoke check
- **Max feedback latency:** ~30s per-task / ~5 min per-wave

---

## Per-Task Verification Map

_To be filled by gsd-planner from RESEARCH.md § Validation Architecture. Each task in every PLAN.md MUST map to either an automated command here or a Wave 0 dependency._

| Task ID | Plan  | Wave | Requirement                                   | Test Type         | Automated Command                                                                                                                                       | File Exists | Status    |
| ------- | ----- | ---- | --------------------------------------------- | ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------- | --------- |
| 01-T1   | 87-01 | 1    | REQ-87-01, REQ-87-04, REQ-87-14, REQ-87-15    | Wave 0 scaffold   | `cd src-tauri && cargo check -p uc-observability --tests --features __wave0_scaffold_87`                                                                | ⬜ new      | ⬜ pending |
| 01-T2   | 87-01 | 1    | REQ-87-03, REQ-87-06                          | Wave 0 scaffold   | `cd src-tauri && cargo check -p uc-observability --tests --features __wave0_scaffold_87 && cargo check -p uc-core --tests --features __wave0_scaffold_87_traceparent` | ⬜ new      | ⬜ pending |
| 02-T1   | 87-02 | 2    | REQ-87-01, REQ-87-02, REQ-87-03, REQ-87-14, REQ-87-15 | Unit + dep graph  | `cd src-tauri && cargo test -p uc-observability --features __wave0_scaffold_87 --test otlp_pipeline --test propagation`                                 | ⬜ new      | ⬜ pending |
| 03-T1   | 87-03 | 3    | REQ-87-06, REQ-87-08                          | Unit serde        | `cd src-tauri && cargo test -p uc-core --test clipboard_message_traceparent && cargo build -p uc-core -p uc-app`                                        | ⬜          | ⬜ pending |
| 03-T2   | 87-03 | 3    | REQ-87-09, REQ-87-10                          | Build + smoke     | `cd src-tauri && cargo build -p uc-bootstrap -p uc-tauri && cargo test -p uc-observability --features __wave0_scaffold_87 --test otlp_pipeline`         | ⬜          | ⬜ pending |
| 04-T1   | 87-04 | 4    | REQ-87-04, REQ-87-05                          | Build + unit      | `cd src-tauri && cargo build -p uc-app && cargo test -p uc-app usecases::internal::capture_clipboard`                                                   | ⬜          | ⬜ pending |
| 04-T2   | 87-04 | 4    | REQ-87-06, REQ-87-07                          | Build + unit      | `cd src-tauri && cargo build -p uc-app && cargo test -p uc-app usecases::clipboard::sync`                                                               | ⬜          | ⬜ pending |
| 05-T1   | 87-05 | 5    | REQ-87-01 (deletion completion)               | Full workspace    | `cd src-tauri && cargo build && cargo test -p uc-observability`                                                                                         | ⬜          | ⬜ pending |
| 06-T1   | 87-06 | 5    | REQ-87-11, REQ-87-12, REQ-87-13               | grep + JSON/YAML  | `grep -q 'OTEL_EXPORTER_OTLP_ENDPOINT' docs/architecture/logging-architecture.md && ! grep -q 'flow_id' docs/seq/signals/flow-timeline.json && ! grep -q 'origin_flow_id' docs/seq/signals/cross-device-flow.json && grep -q 'ingest/otlp' docker-compose.seq.yml` | ⬜          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

_New test files / stubs / dev-dependencies that must exist before Wave 1+ tasks can run. Planner fills from RESEARCH.md — expected scope:_

- [ ] `src-tauri/crates/uc-observability/tests/otlp_pipeline.rs` — integration test stubs for OTLP pipeline init
- [ ] `src-tauri/crates/uc-observability/tests/propagation.rs` — traceparent inject/extract unit stubs
- [ ] `src-tauri/crates/uc-core/tests/clipboard_message_traceparent.rs` — protocol field backward-compat stubs
- [ ] Dev-dependency: `opentelemetry-stdout` (or equivalent in-memory exporter) added to `uc-observability/Cargo.toml` `[dev-dependencies]`
- [ ] Mock OTLP collector helper (in-process HTTP server or stdout exporter) for assertion tests

_Planner MUST confirm or replace this list based on research findings._

---

## Manual-Only Verifications

| Behavior                                                 | Requirement | Why Manual                               | Test Instructions                                                                                                   |
| -------------------------------------------------------- | ----------- | ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| Seq UI shows traces with correct parent-child hierarchy  | REQ-87-XX   | Requires running Seq container + Tauri   | `docker compose -f docker-compose.seq.yml up -d` → `bun tauri dev` → copy something → open Seq → verify trace tree  |
| Cross-device traceparent continuation visible in Seq     | REQ-87-XX   | Requires two peers + live network        | Launch peerA + peerB, clipboard sync, verify single trace in Seq with spans from both instances                      |
| `UC_SEQ_URL` legacy warning fires on startup             | REQ-87-XX   | Requires env var + log inspection         | `UC_SEQ_URL=http://x bun tauri dev` → verify `warn!` log line prompting migration to `OTEL_EXPORTER_OTLP_ENDPOINT`  |

_Planner may add/remove rows based on final task decomposition._

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s (per-crate) / < 5 min (workspace)
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
