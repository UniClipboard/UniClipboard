---
phase: 20
slug: clipboard-capture-flow-correlation
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-11
---

# Phase 20 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                         |
| ---------------------- | ------------------------------------------------------------- |
| **Framework**          | cargo test (built-in Rust test framework)                     |
| **Config file**        | `src-tauri/Cargo.toml` workspace                              |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-observability --lib stages` |
| **Full suite command** | `cd src-tauri && cargo test -p uc-observability -p uc-app`    |
| **Estimated runtime**  | ~15 seconds                                                   |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-observability --lib stages`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-observability -p uc-app`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 15 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement | Test Type   | Automated Command                                                        | File Exists       | Status     |
| -------- | ---- | ---- | ----------- | ----------- | ------------------------------------------------------------------------ | ----------------- | ---------- |
| 20-03-01 | 03   | 1    | FLOW-03     | unit        | `cd src-tauri && cargo test -p uc-observability --lib stages`            | ✅ (needs update) | ⬜ pending |
| 20-03-02 | 03   | 1    | FLOW-03     | manual-only | Run app with LOG_PROFILE=debug_clipboard, copy text, inspect JSON output | N/A               | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

_Existing infrastructure covers all phase requirements._

---

## Manual-Only Verifications

| Behavior                                                                        | Requirement | Why Manual                                                                          | Test Instructions                                                                                                                                       |
| ------------------------------------------------------------------------------- | ----------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| spool_blobs span appears as sibling of cache_representations in structured logs | FLOW-03     | Span hierarchy is runtime behavior of tracing subscriber — cannot verify statically | Start app with `LOG_PROFILE=debug_clipboard`, copy text to clipboard, inspect terminal JSON output for `spool_blobs` stage span with matching `flow_id` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 15s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
