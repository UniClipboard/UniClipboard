---
phase: 35
slug: extract-outboundsyncplanner-to-consolidate-scattered-sync-policy-checks
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-16
---

# Phase 35 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                       |
| ---------------------- | ----------------------------------------------------------- |
| **Framework**          | Rust `#[tokio::test]` (tokio 1, already in uc-app dev-deps) |
| **Config file**        | none — inline `#[cfg(test)]` modules per-file               |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-app sync_planner 2>&1`    |
| **Full suite command** | `cd src-tauri && cargo test -p uc-app 2>&1`                 |
| **Estimated runtime**  | ~5 seconds                                                  |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-app sync_planner 2>&1`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-app && cargo test -p uc-tauri 2>&1`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement                                                    | Test Type   | Automated Command                   | File Exists | Status     |
| -------- | ---- | ---- | -------------------------------------------------------------- | ----------- | ----------------------------------- | ----------- | ---------- |
| 35-01-01 | 01   | 1    | plan() returns clipboard: None when origin == RemotePush       | unit        | `cargo test -p uc-app sync_planner` | ❌ W0       | ⬜ pending |
| 35-01-02 | 01   | 1    | plan() returns files: [] when file_sync_enabled == false       | unit        | `cargo test -p uc-app sync_planner` | ❌ W0       | ⬜ pending |
| 35-01-03 | 01   | 1    | plan() returns files: [] when origin != LocalCapture           | unit        | `cargo test -p uc-app sync_planner` | ❌ W0       | ⬜ pending |
| 35-01-04 | 01   | 1    | plan() excludes files exceeding max_file_size                  | unit        | `cargo test -p uc-app sync_planner` | ❌ W0       | ⬜ pending |
| 35-01-05 | 01   | 1    | plan() returns clipboard: None when all files excluded         | unit        | `cargo test -p uc-app sync_planner` | ❌ W0       | ⬜ pending |
| 35-01-06 | 01   | 1    | plan() returns both clipboard and files when mixed sizes       | unit        | `cargo test -p uc-app sync_planner` | ❌ W0       | ⬜ pending |
| 35-01-07 | 01   | 1    | plan() proceeds safely when settings load fails                | unit        | `cargo test -p uc-app sync_planner` | ❌ W0       | ⬜ pending |
| 35-02-01 | 02   | 2    | Runtime dispatches clipboard sync iff plan.clipboard.is_some() | integration | `cargo test -p uc-tauri`            | ❌ W0       | ⬜ pending |
| 35-02-02 | 02   | 2    | SyncOutboundFileUseCase no longer re-checks file_sync_enabled  | review      | manual code review                  | N/A         | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-app/src/usecases/sync_planner/` — module directory (does not exist yet)
- [ ] `src-tauri/crates/uc-app/src/usecases/sync_planner/planner.rs` — inline `#[cfg(test)]` stubs created with implementation

_Framework already installed; no new dev-dependencies required._

---

## Manual-Only Verifications

| Behavior                                                      | Requirement        | Why Manual                          | Test Instructions                            |
| ------------------------------------------------------------- | ------------------ | ----------------------------------- | -------------------------------------------- |
| SyncOutboundFileUseCase no longer re-checks file_sync_enabled | Redundancy removal | Code review — verify guards deleted | Inspect sync_outbound.rs lines 54-67 removed |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
