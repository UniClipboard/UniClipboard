---
phase: 91
slug: sqlite-index-adapter-and-rebuild-strategy
status: ready
nyquist_compliant: true
wave_0_complete: false
created: 2026-04-11
---

# Phase 91 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust module/integration tests in `uc-infra` |
| **Config file** | none — existing Cargo workspace + Diesel SQLite pool |
| **Quick run command** | `cd src-tauri && cargo test -p uc-infra search::sqlite_index` |
| **Full suite command** | `cd src-tauri && cargo test -p uc-infra search:: && cargo check -p uc-infra` |
| **Estimated runtime** | ~120 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-infra search::sqlite_index`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-infra search:: && cargo check -p uc-infra`
- **Before `$gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 91-01-01 | 01 | 1 | REBLD-01 | integration | `cd src-tauri && cargo test -p uc-infra search::sqlite_index::tests::meta_and_live_write` | ❌ W0 | ⬜ pending |
| 91-01-02 | 01 | 1 | REBLD-01 | integration | `cd src-tauri && cargo test -p uc-infra search::sqlite_index::tests::search_query` | ❌ W0 | ⬜ pending |
| 91-02-01 | 02 | 2 | REBLD-02 | integration | `cd src-tauri && cargo test -p uc-infra search::sqlite_index::tests::rebuild_cutover` | ❌ W0 | ⬜ pending |
| 91-02-02 | 02 | 2 | REBLD-03 | integration | `cd src-tauri && cargo test -p uc-infra search::sqlite_index::tests::rebuild_mirroring` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-infra/src/search/sqlite_index.rs` — create the adapter module and test namespace used by the quick-run filter
- [ ] `src-tauri/crates/uc-infra/src/search/test_support.rs` or equivalent local fixtures — fixed profile scope, deterministic search-key derivation, and temp-file DB helpers
- [ ] long-lived reader helper for temp-file SQLite — holds a read transaction open while rebuild finalizes so the no-rename cutover claim is testable

---

## Manual-Only Verifications

All phase behaviors should be automatable with temp-file SQLite fixtures and port-level tests. No manual-only behavior is expected for this phase.

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 120s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-04-11
