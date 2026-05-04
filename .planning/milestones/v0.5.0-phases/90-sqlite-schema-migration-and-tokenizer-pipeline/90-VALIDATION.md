---
phase: 90
slug: sqlite-schema-migration-and-tokenizer-pipeline
status: ready
nyquist_compliant: true
wave_0_complete: false
created: 2026-04-11
---

# Phase 90 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust unit tests + migration smoke tests |
| **Config file** | none — existing Cargo workspace + embedded Diesel migrations |
| **Quick run command** | `cd src-tauri && cargo test -p uc-infra search::` |
| **Full suite command** | `cd src-tauri && cargo test -p uc-infra && cargo check -p uc-infra` |
| **Estimated runtime** | ~90 seconds |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-infra search::`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-infra && cargo check -p uc-infra`
- **Before `$gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 90 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------|-------------------|-------------|--------|
| 90-01-01 | 01 | 1 | SIDX-07 | migration smoke | `cd src-tauri && cargo test -p uc-infra search::migration` | ❌ W0 | ⬜ pending |
| 90-01-02 | 01 | 1 | SIDX-07 | static/schema | `cd src-tauri && cargo test -p uc-infra search::rows` | ❌ W0 | ⬜ pending |
| 90-02-01 | 02 | 2 | SIDX-05, SIDX-06 | unit | `cd src-tauri && cargo test -p uc-infra search::tokenizer` | ❌ W0 | ⬜ pending |
| 90-02-02 | 02 | 2 | SIDX-03, SIDX-04 | unit | `cd src-tauri && cargo test -p uc-infra search::key` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-infra/src/search/mod.rs` — create the `search::` test namespace used by quick-run filters
- [ ] `src-tauri/crates/uc-infra/src/search/test_support.rs` or equivalent local fixtures — representative HTML/URL/file-path samples for extractor tests
- [ ] migration smoke test module under `uc-infra` — asserts the three search tables and required columns exist after embedded migrations run

---

## Manual-Only Verifications

All phase behaviors have automated verification.

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 90s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-04-11
