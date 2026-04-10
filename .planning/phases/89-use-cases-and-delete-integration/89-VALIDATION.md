---
phase: 89
slug: use-cases-and-delete-integration
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-10
---

# Phase 89 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                               |
| ---------------------- | --------------------------------------------------- |
| **Framework**          | cargo test (Rust unit tests)                        |
| **Config file**        | src-tauri/Cargo.toml                                |
| **Quick run command**  | `cargo test -p uc-app -- usecases::search`          |
| **Full suite command** | `cargo test -p uc-app`                              |
| **Estimated runtime**  | ~10 seconds                                         |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p uc-app -- usecases::search`
- **After every plan wave:** Run `cargo test -p uc-app`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID   | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status     |
| --------- | ---- | ---- | ----------- | --------- | ----------------- | ----------- | ---------- |
| 89-01-01 | 01   | 1    | SIDX-01     | unit      | `cargo test -p uc-app -- usecases::search::index`  | ❌ W0  | ⬜ pending |
| 89-01-02 | 01   | 1    | SIDX-01     | unit      | `cargo test -p uc-app -- usecases::search::remove` | ❌ W0  | ⬜ pending |
| 89-01-03 | 01   | 1    | SIDX-01     | unit      | `cargo test -p uc-app -- usecases::search::search_entries` | ❌ W0  | ⬜ pending |
| 89-01-04 | 01   | 1    | SIDX-01     | unit      | `cargo test -p uc-app -- usecases::search::rebuild` | ❌ W0  | ⬜ pending |
| 89-02-01 | 02   | 2    | SIDX-02     | unit      | `cargo test -p uc-app -- usecases::delete_clipboard_entry` | ✅  | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/uc-app/src/usecases/search/mod.rs` — module stub
- [ ] `src-tauri/uc-app/src/usecases/search/index_clipboard_entry.rs` — stub for SIDX-01
- [ ] `src-tauri/uc-app/src/usecases/search/remove_indexed_entry.rs` — stub for SIDX-01
- [ ] `src-tauri/uc-app/src/usecases/search/search_clipboard_entries.rs` — stub for SIDX-01
- [ ] `src-tauri/uc-app/src/usecases/search/rebuild_search_index.rs` — stub for SIDX-01

_If none: "Existing infrastructure covers all phase requirements."_

---

## Manual-Only Verifications

| Behavior   | Requirement | Why Manual | Test Instructions |
| ---------- | ----------- | ---------- | ----------------- |

_All phase behaviors have automated verification._

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
