---
phase: 84
slug: cli-gui-daemon
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-02
---

# Phase 84 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                                          |
| --------------------- | ---------------------------------------------------------------------------------------------- |
| **Framework**         | Vitest (frontend) + Rust `#[tokio::test]` (backend)                                          |
| **Config file**       | `vitest.config.ts` (frontend); `Cargo.toml` feature flags (backend)                           |
| **Quick run command** | `cd src-tauri && cargo test -p uc-daemon security_middleware && cd ../.. && bun test -- src/__tests__/lib/daemon-auth.test.ts` |
| **Full suite command**| `cd src-tauri && cargo test -p uc-daemon && cd ../.. && bun test -- src/__tests__/lib/daemon` |
| **Estimated runtime** | ~60 seconds                                                                                   |

---

## Sampling Rate

- **After every task commit:** Run quick command
- **After every plan wave:** Run full suite command
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Task ID   | Plan | Wave | Requirement | Test Type     | Automated Command                                                           | File Exists | Status     |
| --------- | ---- | ---- | ----------- | ------------- | -------------------------------------------------------------------------- | ----------- | ---------- |
| AUTH-01   | 01   | 1    | CLI uses POST /auth/connect | unit | `cargo test -p uc-cli daemon_client`                              | ✅         | ⬜ pending |
| AUTH-02   | 01   | 1    | CLI PID registered in daemon whitelist | unit | `cargo test -p uc-daemon security_middleware`                 | ✅         | ⬜ pending |
| AUTH-03   | 01   | 1    | CLI rate limited same as GUI | unit | `cargo test -p uc-daemon rate_limiter`                                 | ✅         | ⬜ pending |
| AUTH-04   | 01   | 2    | Daemon L2+ routes reject bare bearer tokens | integration | `cargo test -p uc-daemon api_auth -- auth_connect` | ✅         | ⬜ pending |
| AUTH-05   | 01   | 2    | CLI and GUI get independent session tokens | unit | `bun test -- src/__tests__/lib/daemon-auth.test.ts`              | ✅         | ⬜ pending |
| AUTH-06   | 01   | 2    | Bearer token only at /auth/connect | integration | `cargo test -p uc-daemon api_auth`                              | ✅         | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-cli/tests/cli_auth.rs` — new integration tests for CLI auth flow (create)
- [ ] `src/__tests__/lib/daemon-auth.test.ts` — add CLI-focused tests for independent session scopes (extend)
- [ ] `src-tauri/crates/uc-daemon/tests/api_auth.rs` — verify /auth/connect with CLI clientType coverage (extend)

_Existing infrastructure: Vitest + Rust test frameworks already present in project._

---

## Manual-Only Verifications

| Behavior                                              | Requirement | Why Manual              | Test Instructions |
| ----------------------------------------------------- | ----------- | ----------------------- | ----------------- |
| CLI and GUI tokens truly independent (different jti)  | AUTH-05     | Requires two separate processes exchanging | Run CLI command + GUI action, check daemon logs for distinct jti claims |
| Rate limiter counters per PID                         | AUTH-03     | Rate limit state not exposed via API | Check daemon logs after 5+ CLI invocations from different PIDs |

_Where possible, prefer automated verification. Manual checks are fallback only._

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
