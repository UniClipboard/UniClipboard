---
phase: 45
slug: daemon-api-foundation-add-local-http-and-websocket-transport-with-read-only-runtime-queries
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-19
---

# Phase 45 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                                                           |
| ---------------------- | --------------------------------------------------------------------------------------------------------------- |
| **Framework**          | Rust `cargo test`                                                                                               |
| **Config file**        | `src-tauri/Cargo.toml`                                                                                          |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon --test api_auth --test api_query --test http_api --test websocket_api` |
| **Full suite command** | `cd src-tauri && cargo test -p uc-daemon -p uc-cli -- --test-threads=1`                                         |
| **Estimated runtime**  | ~90 seconds                                                                                                     |

---

## Sampling Rate

- **After every task commit:** Run the task-local command from the verification map below
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-daemon -p uc-cli -- --test-threads=1`
- **Before `$gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type      | Automated Command                                                                  | File Exists | Status     |
| -------- | ---- | ---- | ----------- | -------------- | ---------------------------------------------------------------------------------- | ----------- | ---------- |
| 45-01-01 | 01   | 1    | PH45-01     | unit           | `cd src-tauri && cargo test -p uc-daemon --test api_auth`                          | ❌ W0       | ⬜ pending |
| 45-01-02 | 01   | 1    | PH45-02     | unit           | `cd src-tauri && cargo test -p uc-daemon --test api_query`                         | ❌ W0       | ⬜ pending |
| 45-01-03 | 01   | 1    | PH45-02     | unit           | `cd src-tauri && cargo test -p uc-daemon`                                          | ✅          | ⬜ pending |
| 45-02-01 | 02   | 2    | PH45-03     | integration    | `cd src-tauri && cargo test -p uc-daemon --test http_api -- --test-threads=1`      | ❌ W0       | ⬜ pending |
| 45-02-02 | 02   | 2    | PH45-04     | integration    | `cd src-tauri && cargo test -p uc-daemon --test websocket_api -- --test-threads=1` | ❌ W0       | ⬜ pending |
| 45-03-01 | 03   | 3    | PH45-05     | integration    | `cd src-tauri && cargo test -p uc-cli -- --test-threads=1`                         | ❌ W0       | ⬜ pending |
| 45-03-02 | 03   | 3    | PH45-06     | manual + smoke | `cd src-tauri && cargo build`                                                      | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-daemon/src/api/mod.rs` — API module scaffold so route/auth/type tests have a stable home
- [ ] `src-tauri/crates/uc-daemon/tests/api_auth.rs` — auth token and connection info tests
- [ ] `src-tauri/crates/uc-daemon/tests/api_query.rs` — query DTO and transport-neutral state tests
- [ ] `src-tauri/crates/uc-daemon/tests/http_api.rs` — integration tests for loopback bind, auth, and read-only routes
- [ ] `src-tauri/crates/uc-daemon/tests/websocket_api.rs` — integration tests for subscribe, snapshot, and incremental event flow
- [ ] `src-tauri/crates/uc-cli/tests/` or equivalent command-level test coverage — CLI status/devices over HTTP

---

## Manual-Only Verifications

| Behavior                                                                       | Requirement | Why Manual                                        | Test Instructions                                                                                                                                                                        |
| ------------------------------------------------------------------------------ | ----------- | ------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Tauri injects daemon connection info into the webview without persisting token | PH45-06     | Requires runtime + desktop shell inspection       | Launch desktop app, inspect webview storage and runtime bridge, verify token is present only in in-memory injection path and absent from `localStorage`/URL                              |
| Web frontend can establish a daemon WebSocket with injected token              | PH45-06     | Requires real Tauri shell and webview environment | Start daemon and desktop app, listen for `daemon://connection-info`, then use devtools network panel to confirm loopback WebSocket connects with auth header and receives snapshot event |

---

## Validation Sign-Off

- [ ] All tasks have `<acceptance_criteria>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 120s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
