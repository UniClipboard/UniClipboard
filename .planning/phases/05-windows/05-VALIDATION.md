---
phase: 5
slug: windows
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-05
---

# Phase 5 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                      |
| ---------------------- | ---------------------------------------------------------- |
| **Framework**          | cargo test (Rust built-in)                                 |
| **Config file**        | src-tauri/Cargo.toml (workspace)                           |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-platform -- --nocapture` |
| **Full suite command** | `cd src-tauri && cargo test`                               |
| **Estimated runtime**  | ~30 seconds                                                |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-platform -- --nocapture`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID    | Plan | Wave | Requirement                               | Test Type   | Automated Command                                            | File Exists | Status  |
| ---------- | ---- | ---- | ----------------------------------------- | ----------- | ------------------------------------------------------------ | ----------- | ------- |
| WIN-IMG-01 | 01   | 1    | Text capture unbroken                     | unit        | `cd src-tauri && cargo test -p uc-platform -- watcher`       | Existing    | pending |
| WIN-IMG-02 | 01   | 1    | read_image_windows_as_png valid PNG       | unit        | `cd src-tauri && cargo test -p uc-platform -- image_convert` | W0          | pending |
| WIN-IMG-03 | 01   | 1    | PNG encoding from DynamicImage            | unit        | `cd src-tauri && cargo test -p uc-platform -- image_convert` | W0          | pending |
| WIN-IMG-04 | 02   | 2    | Fallback triggers when clipboard-rs fails | unit        | `cd src-tauri && cargo test -p uc-platform -- --nocapture`   | W0          | pending |
| WIN-IMG-05 | 02   | 2    | E2E screenshot capture                    | manual-only | Manual: Win+Shift+S                                          | N/A         | pending |
| WIN-IMG-06 | 02   | 2    | E2E browser image copy                    | manual-only | Manual: right-click > Copy Image                             | N/A         | pending |

_Status: pending / green / red / flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-platform/src/clipboard/image_convert.rs` — cross-platform unit tests for dib_to_png conversion (uses `image` crate, runs on all platforms)
- [ ] Cross-platform test for BMP-to-PNG conversion logic — extract conversion to a testable function in `image_convert.rs`, guard Windows clipboard access behind `#[cfg(target_os = "windows")]` in `windows.rs`

_Existing watcher tests cover WIN-IMG-01 (text capture regression)._

---

## Manual-Only Verifications

| Behavior                         | Requirement | Why Manual                           | Test Instructions                                                                            |
| -------------------------------- | ----------- | ------------------------------------ | -------------------------------------------------------------------------------------------- |
| Screenshot capture (Win+Shift+S) | WIN-IMG-05  | Requires Windows GUI + snipping tool | 1. Press Win+Shift+S, capture area 2. Verify image appears in clipboard history UI           |
| Browser image copy               | WIN-IMG-06  | Requires Windows GUI + browser       | 1. Right-click image in browser > Copy Image 2. Verify image appears in clipboard history UI |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
