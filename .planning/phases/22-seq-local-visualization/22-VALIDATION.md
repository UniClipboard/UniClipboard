---
phase: 22
slug: seq-local-visualization
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-11
---

# Phase 22 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                        |
| ---------------------- | ------------------------------------------------------------ |
| **Framework**          | cargo test (built-in)                                        |
| **Config file**        | src-tauri/crates/uc-observability/Cargo.toml                 |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-observability`             |
| **Full suite command** | `cd src-tauri && cargo test -p uc-observability -p uc-tauri` |
| **Estimated runtime**  | ~15 seconds                                                  |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-observability`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-observability -p uc-tauri`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type   | Automated Command                                                         | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ----------- | ------------------------------------------------------------------------- | ----------- | ---------- |
| 22-01-01 | 01   | 1    | SEQ-01      | unit        | `cd src-tauri && cargo test -p uc-observability clef_format`              | ❌ W0       | ⬜ pending |
| 22-01-02 | 01   | 1    | SEQ-04      | unit        | `cd src-tauri && cargo test -p uc-observability clef_format::span_fields` | ❌ W0       | ⬜ pending |
| 22-01-03 | 01   | 1    | SEQ-06      | unit        | `cd src-tauri && cargo test -p uc-observability clef_format::timestamp`   | ❌ W0       | ⬜ pending |
| 22-02-01 | 02   | 1    | SEQ-02      | unit        | `cd src-tauri && cargo test -p uc-observability seq::build_seq_layer`     | ❌ W0       | ⬜ pending |
| 22-02-02 | 02   | 1    | SEQ-03      | unit        | `cd src-tauri && cargo test -p uc-observability seq::sender`              | ❌ W0       | ⬜ pending |
| 22-02-03 | 02   | 1    | SEQ-05      | unit        | `cd src-tauri && cargo test -p uc-observability seq::config`              | ❌ W0       | ⬜ pending |
| 22-03-01 | 03   | 2    | SEQ-02      | integration | `cd src-tauri && cargo test -p uc-tauri seq_integration`                  | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-observability/src/clef_format.rs` — CLEFFormat unit test stubs for SEQ-01, SEQ-04, SEQ-06
- [ ] `src-tauri/crates/uc-observability/src/seq/` — Sender, config, and layer test stubs for SEQ-02, SEQ-03, SEQ-05
- [ ] reqwest + tokio dependencies added to uc-observability/Cargo.toml

_Existing infrastructure (cargo test) covers framework requirements._

---

## Manual-Only Verifications

| Behavior                     | Requirement | Why Manual                 | Test Instructions                                                                       |
| ---------------------------- | ----------- | -------------------------- | --------------------------------------------------------------------------------------- |
| Docker Compose Seq startup   | SEQ-05      | Requires Docker runtime    | `docker compose -f docker-compose.seq.yml up -d`, verify Seq UI at http://localhost:80  |
| End-to-end flow query in Seq | SEQ-04      | Requires running Seq + app | Capture clipboard, open Seq UI, filter by `flow_id`, verify stages appear in time order |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
