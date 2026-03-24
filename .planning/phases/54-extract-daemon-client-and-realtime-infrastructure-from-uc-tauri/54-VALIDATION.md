---
phase: 54
slug: extract-daemon-client-and-realtime-infrastructure-from-uc-tauri
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-24
---

# Phase 54 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                            |
| ---------------------- | ------------------------------------------------ |
| **Framework**          | Rust built-in `#[test]` / `#[tokio::test]`       |
| **Config file**        | None — standard `cargo test`                     |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon-client` |
| **Full suite command** | `cd src-tauri && cargo test --workspace`         |
| **Estimated runtime**  | ~30 seconds                                      |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-daemon-client`
- **After every plan wave:** Run `cd src-tauri && cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement          | Test Type   | Automated Command                                         | File Exists | Status  |
| -------- | ---- | ---- | -------------------- | ----------- | --------------------------------------------------------- | ----------- | ------- |
| 54-01-01 | 01   | 1    | N/A (crate creation) | cargo check | `cargo check -p uc-daemon-client`                         | n/a         | pending |
| 54-02-01 | 02   | 1    | N/A (file moves)     | grep        | `rg 'crate::daemon_client' src-tauri/crates/uc-tauri/src` | n/a         | pending |
| 54-03-01 | 03   | 1    | N/A (import updates) | cargo check | `cargo check -p uc-tauri`                                 | n/a         | pending |
| 54-04-01 | 04   | 2    | N/A (cleanup)        | cargo test  | `cargo test --workspace`                                  | n/a         | pending |

_Status: pending · green · red · flaky_

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements. No new test framework needed.

The following tests are **moved** from `uc-tauri` to `uc-daemon-client` (not new gaps):

- [ ] `uc-daemon-client/src/http/pairing.rs` — inline tests moved from `uc-tauri/src/daemon_client/pairing.rs`
- [ ] `uc-daemon-client/src/http/query.rs` — `query_tests` module inlined from `uc-tauri/src/daemon_client/query_tests.rs`
- [ ] `uc-daemon-client/src/http/setup.rs` — inline tests moved from `uc-tauri/src/daemon_client/setup.rs`
- [ ] `uc-daemon-client/src/ws_bridge.rs` — inline tests moved from `uc-tauri/src/bootstrap/daemon_ws_bridge.rs`
- [ ] `uc-daemon-client/src/realtime.rs` — inline tests moved from `uc-tauri/src/bootstrap/realtime_runtime.rs`
- [ ] `uc-daemon-client/src/connection.rs` — inline tests moved from `uc-tauri/src/bootstrap/runtime.rs`

---

## Manual-Only Verifications

All phase behaviors have automated verification via `cargo check` and `cargo test`.

---

## Validation Sign-Off

- [ ] All tasks have automated verification (cargo check / cargo test / grep)
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] `uc-daemon-client` compiles cleanly after move
- [ ] `uc-tauri` compiles cleanly after import updates
- [ ] No `crate::daemon_client` references remain in `uc-tauri` source
- [ ] All moved tests pass in new crate
- [ ] Workspace tests pass
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending

---
