---
phase: 39
slug: config-resolution-extraction
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-18
---

# Phase 39 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                            |
| ---------------------- | ---------------------------------------------------------------- |
| **Framework**          | cargo test (Rust)                                                |
| **Config file**        | src-tauri/Cargo.toml                                             |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-tauri --lib config_resolution` |
| **Full suite command** | `cd src-tauri && cargo test`                                     |
| **Estimated runtime**  | ~30 seconds                                                      |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-tauri --lib config_resolution`
- **After every plan wave:** Run `cd src-tauri && cargo test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type   | Automated Command                                                | File Exists | Status     |
| -------- | ---- | ---- | ----------- | ----------- | ---------------------------------------------------------------- | ----------- | ---------- |
| 39-01-01 | 01   | 1    | RNTM-03     | unit        | `cd src-tauri && cargo test -p uc-tauri --lib config_resolution` | ❌ W0       | ⬜ pending |
| 39-01-02 | 01   | 1    | RNTM-03     | unit        | `cd src-tauri && cargo test -p uc-tauri --lib config_resolution` | ❌ W0       | ⬜ pending |
| 39-01-03 | 01   | 1    | RNTM-03     | integration | `cd src-tauri && cargo test`                                     | ✅          | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-tauri/src/bootstrap/config_resolution.rs` — new module with extracted functions
- [ ] Unit tests for `resolve_config_path`, `apply_profile_suffix`, `get_storage_paths`, `build_key_slot_store`

_Existing cargo test infrastructure covers framework needs._

---

## Manual-Only Verifications

| Behavior                                    | Requirement | Why Manual                                     | Test Instructions                                               |
| ------------------------------------------- | ----------- | ---------------------------------------------- | --------------------------------------------------------------- |
| GUI app launches correctly after extraction | RNTM-03     | Requires running Tauri app with full windowing | Run `bun tauri dev`, verify app starts and clipboard sync works |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
