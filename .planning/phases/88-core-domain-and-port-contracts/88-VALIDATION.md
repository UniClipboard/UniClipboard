---
phase: 88
slug: core-domain-and-port-contracts
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-04-10
---

# Phase 88 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                               |
| ---------------------- | --------------------------------------------------- |
| **Framework**          | cargo test (Rust built-in)                          |
| **Config file**        | Cargo.toml (workspace)                              |
| **Quick run command**  | `cargo check -p uc-core`                            |
| **Full suite command** | `cargo test -p uc-core && cargo check --workspace`  |
| **Estimated runtime**  | ~10 seconds                                         |

---

## Sampling Rate

- **After every task commit:** Run `cargo check -p uc-core`
- **After every plan wave:** Run `cargo test -p uc-core && cargo check --workspace`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 10 seconds

---

## Per-Task Verification Map

| Task ID   | Plan | Wave | Requirement | Test Type | Automated Command | File Exists | Status     |
| --------- | ---- | ---- | ----------- | --------- | ----------------- | ----------- | ---------- |
| 88-01-01  | 01   | 1    | Phase 88 SC1 | compile  | `cargo check -p uc-core` | ❌ W0 | ⬜ pending |
| 88-01-02  | 01   | 1    | Phase 88 SC2 | unit     | `cargo test -p uc-core -- search_key` | ❌ W0 | ⬜ pending |
| 88-01-03  | 01   | 1    | Phase 88 SC3 | compile  | `cargo check -p uc-core` | ❌ W0 | ⬜ pending |
| 88-01-04  | 01   | 1    | Phase 88 SC4 | compile  | `cargo check -p uc-core` | ❌ W0 | ⬜ pending |
| 88-01-05  | 01   | 2    | Phase 88 SC5 | compile  | `cargo check --workspace` | ❌ W0 | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `uc-core/src/search/mod.rs` — module stub
- [ ] `uc-core/src/search/key.rs` — SearchKey stub
- [ ] `uc-core/src/search/ports.rs` — port trait stubs
- [ ] `uc-core/src/search/query.rs` — SearchQuery stub
- [ ] `uc-core/src/search/result.rs` — SearchResult stub

---

## Manual-Only Verifications

| Behavior   | Requirement | Why Manual | Test Instructions |
| ---------- | ----------- | ---------- | ----------------- |
| SearchKey bytes not accessible from outside uc-core | SC2 | Compile-time visibility enforcement — `cargo check` confirms, but human review validates no public byte accessors exist | Review `uc-core/src/search/key.rs` and confirm no `pub fn` exposes `[u8; N]` or raw bytes |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 10s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
