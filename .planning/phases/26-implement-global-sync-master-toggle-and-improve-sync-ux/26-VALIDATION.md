---
phase: 26
slug: implement-global-sync-master-toggle-and-improve-sync-ux
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-12
---

# Phase 26 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property                   | Value                                                      |
| -------------------------- | ---------------------------------------------------------- |
| **Framework (Backend)**    | Rust built-in test + tokio::test (async)                   |
| **Framework (Frontend)**   | Manual-only (no vitest configured)                         |
| **Config file (Backend)**  | Cargo.toml per crate (existing)                            |
| **Config file (Frontend)** | None — manual testing sufficient                           |
| **Quick run command**      | `cd src-tauri && cargo test -p uc-app -- sync_outbound -x` |
| **Full suite command**     | `cd src-tauri && cargo test --workspace`                   |
| **Estimated runtime**      | ~30 seconds                                                |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-app -- sync_outbound -x`
- **After every plan wave:** Run `cd src-tauri && cargo test --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green + manual frontend checklist
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement                                 | Test Type   | Automated Command                                                          | File Exists | Status     |
| ------- | ---- | ---- | ------------------------------------------- | ----------- | -------------------------------------------------------------------------- | ----------- | ---------- |
| P26-01  | 01   | 1    | Global off returns empty peers              | unit        | `cd src-tauri && cargo test -p uc-app -- sync_outbound_global_toggle`      | ❌ W0       | ⬜ pending |
| P26-02  | 01   | 1    | Global off overrides per-device on          | unit        | `cd src-tauri && cargo test -p uc-app -- sync_outbound_global_override`    | ❌ W0       | ⬜ pending |
| P26-03  | 01   | 1    | Global on preserves per-device filtering    | unit        | `cd src-tauri && cargo test -p uc-app -- sync_outbound_global_enabled`     | ❌ W0       | ⬜ pending |
| P26-04  | 01   | 1    | Settings load failure safety fallback       | unit        | `cd src-tauri && cargo test -p uc-app -- sync_outbound_settings_fallback`  | ❌ W0       | ⬜ pending |
| P26-05  | 01   | 1    | Per-device settings not mutated             | unit        | `cd src-tauri && cargo test -p uc-app -- sync_outbound_no_device_mutation` | ❌ W0       | ⬜ pending |
| P26-06  | 02   | 1    | Banner visible when global off              | manual-only | Visual inspection                                                          | N/A         | ⬜ pending |
| P26-07  | 02   | 1    | Banner hidden when global on                | manual-only | Visual inspection                                                          | N/A         | ⬜ pending |
| P26-08  | 02   | 1    | All controls disabled when global off       | manual-only | Visual inspection                                                          | N/A         | ⬜ pending |
| P26-09  | 02   | 1    | Toggle preserves visual state when disabled | manual-only | Visual inspection                                                          | N/A         | ⬜ pending |
| P26-10  | 02   | 1    | Go to Settings navigates to sync section    | manual-only | Visual inspection                                                          | N/A         | ⬜ pending |
| P26-11  | 03   | 2    | i18n keys render in EN and ZH               | manual-only | Visual inspection                                                          | N/A         | ⬜ pending |
| P26-12  | 01   | 1    | Resume after re-enable works immediately    | integration | `cd src-tauri && cargo test -p uc-app -- sync_outbound_resume`             | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-app/tests/sync_outbound_policy_test.rs` — unit tests for global auto_sync enforcement (P26-01 through P26-05, P26-12)
- [ ] Mock `SettingsPort` with configurable `auto_sync` value — extend existing mocks or create focused mock

_Note: Frontend vitest NOT required — manual testing sufficient for UI-only visual changes._

---

## Manual-Only Verifications

| Behavior                 | Requirement | Why Manual                        | Test Instructions                                                                                |
| ------------------------ | ----------- | --------------------------------- | ------------------------------------------------------------------------------------------------ |
| Amber banner visibility  | P26-06/07   | Visual styling verification       | Toggle global auto_sync OFF → navigate to Devices → verify amber banner. Toggle ON → verify gone |
| Controls cascade disable | P26-08/09   | Visual state + disabled attribute | With global OFF, verify all device controls grayed out but preserve on/off visual state          |
| Settings navigation      | P26-10      | Requires full app routing         | Click "Go to Settings" link → verify lands on Sync section                                       |
| i18n rendering           | P26-11      | Requires locale switching         | Switch language to ZH → verify all new strings render correctly                                  |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
