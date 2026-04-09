---
phase: 72
slug: migrate-restore-clipboard-to-daemon-eliminate-cross-process-origin-desync
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-29
---

# Phase 72 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                                                 |
| ---------------------- | ----------------------------------------------------------------------------------------------------- |
| **Framework**          | Rust built-in test + tokio::test                                                                      |
| **Config file**        | src-tauri/Cargo.toml (workspace)                                                                      |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon`                                                             |
| **Full suite command** | `cd src-tauri && cargo test -p uc-daemon && cargo test -p uc-daemon-client && cargo test -p uc-tauri` |
| **Estimated runtime**  | ~30 seconds                                                                                           |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-daemon && cargo check -p uc-tauri`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-daemon && cargo test -p uc-daemon-client && cargo test -p uc-tauri`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type | Automated Command                                                                    | File Exists | Status     |
| -------- | ---- | ---- | ----------- | --------- | ------------------------------------------------------------------------------------ | ----------- | ---------- |
| 72-01-01 | 01   | 1    | PH72-01     | unit      | `cd src-tauri && cargo test -p uc-daemon restore_clipboard`                          | ❌ W0       | ⬜ pending |
| 72-01-02 | 01   | 1    | PH72-02     | unit      | `cd src-tauri && cargo test -p uc-daemon restore_clipboard_not_found`                | ❌ W0       | ⬜ pending |
| 72-01-03 | 01   | 1    | PH72-03     | unit      | `cd src-tauri && cargo test -p uc-daemon restore_clipboard_unauthorized`             | ❌ W0       | ⬜ pending |
| 72-02-01 | 02   | 1    | PH72-04     | unit      | `cd src-tauri && cargo test -p uc-tauri restore_clipboard_proxies_to_daemon`         | ❌ W0       | ⬜ pending |
| 72-XX-XX | —    | —    | PH72-05     | unit      | `cd src-tauri && cargo test -p uc-app restore_clipboard_selection`                   | ✅          | ⬜ pending |
| 72-XX-XX | —    | —    | PH72-06     | unit      | `cd src-tauri && cargo test -p uc-app restore_snapshot_clears_origin_on_write_error` | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-daemon/src/api/routes.rs` — integration tests for `/clipboard/restore/:entry_id` (3 cases: success, 404, 401)
- [ ] `src-tauri/crates/uc-tauri/src/commands/clipboard.rs` — test that `restore_clipboard_entry` uses daemon HTTP client, not direct use case

_Existing infrastructure covers PH72-05 and PH72-06._

---

## Manual-Only Verifications

| Behavior                                         | Requirement | Why Manual                            | Test Instructions                                                                                                                                             |
| ------------------------------------------------ | ----------- | ------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Frontend clipboard history updates after restore | PH72-07     | Requires running GUI + daemon         | 1. Start app with daemon. 2. Copy text. 3. Copy different text. 4. Click restore on first entry. 5. Verify clipboard contains first text and history updates. |
| No duplicate DB entries after restore            | PH72-08     | Requires full integration environment | 1. Enable debug logging. 2. Restore an entry. 3. Check logs for single CaptureClipboard execution with LocalRestore origin.                                   |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
