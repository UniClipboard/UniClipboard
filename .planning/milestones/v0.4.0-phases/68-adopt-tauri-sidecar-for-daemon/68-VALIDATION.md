---
phase: 68
slug: adopt-tauri-sidecar-for-daemon
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-28
---

# Phase 68 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                      |
| ---------------------- | ------------------------------------------ |
| **Framework**          | cargo test (Rust), vitest (Frontend)       |
| **Config file**        | `src-tauri/Cargo.toml`, `vitest.config.ts` |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-tauri`   |
| **Full suite command** | `cd src-tauri && cargo test`               |
| **Estimated runtime**  | ~30 seconds                                |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-tauri`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Test Type   | Automated Command                            | File Exists | Status  |
| ------- | ---- | ---- | ----------- | ----------- | -------------------------------------------- | ----------- | ------- |
| TBD     | 01   | 1    | D-01/D-02   | config      | `grep externalBin src-tauri/tauri.conf.json` | TBD         | pending |
| TBD     | 01   | 1    | D-05        | build       | `cd src-tauri && cargo build`                | TBD         | pending |
| TBD     | 02   | 1    | D-03/D-04   | integration | `cd src-tauri && cargo test -p uc-tauri`     | TBD         | pending |
| TBD     | 02   | 1    | D-06        | integration | `cd src-tauri && cargo test -p uc-tauri`     | TBD         | pending |

_Status: pending / green / red / flaky_

---

## Wave 0 Requirements

- Existing infrastructure covers all phase requirements. No new test framework needed.

---

## Manual-Only Verifications

| Behavior                                | Requirement | Why Manual                     | Test Instructions                            |
| --------------------------------------- | ----------- | ------------------------------ | -------------------------------------------- |
| Daemon launches via sidecar in dev mode | D-03        | Requires running Tauri app     | Run `bun tauri dev`, verify daemon starts    |
| Daemon included in production bundle    | D-01        | Requires full build            | Run `bun tauri build`, check bundle contents |
| stdin pipe tether shutdown              | D-06        | Requires GUI process lifecycle | Close app, verify daemon exits               |

---

## Validation Sign-Off

- [ ] All tasks have automated verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
