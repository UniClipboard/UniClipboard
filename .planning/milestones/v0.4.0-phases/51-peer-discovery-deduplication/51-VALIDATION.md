---
phase: 51
slug: peer-discovery-deduplication
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-23
---

# Phase 51 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                                          |
| ---------------------- | ------------------------------------------------------------------------------ |
| **Framework**          | cargo test (Rust) + vitest (TypeScript)                                        |
| **Config file**        | `src-tauri/Cargo.toml` / `vitest.config.ts`                                    |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-platform --lib`                              |
| **Full suite command** | `cd src-tauri && cargo test -p uc-platform -p uc-daemon -p uc-app && bun test` |
| **Estimated runtime**  | ~30 seconds                                                                    |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-platform --lib`
- **After every plan wave:** Run `cd src-tauri && cargo test -p uc-platform -p uc-daemon -p uc-app && bun test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID  | Plan | Wave | Requirement                  | Test Type | Automated Command                                                | File Exists | Status     |
| -------- | ---- | ---- | ---------------------------- | --------- | ---------------------------------------------------------------- | ----------- | ---------- |
| 51-01-01 | 01   | 1    | local_peer_id filter         | unit      | `cd src-tauri && cargo test -p uc-platform get_discovered_peers` | ❌ W0       | ⬜ pending |
| 51-01-02 | 01   | 1    | peers.changed full snapshot  | unit      | `cd src-tauri && cargo test -p uc-daemon peers_changed`          | ❌ W0       | ⬜ pending |
| 51-01-03 | 01   | 1    | daemon_ws_bridge translation | unit      | `cd src-tauri && cargo test -p uc-tauri daemon_ws_bridge`        | ❌ W0       | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] Unit test for `get_discovered_peers()` filtering local_peer_id
- [ ] Unit test for `peers.changed` full snapshot payload
- [ ] Unit test for `DaemonWsBridge` translating full payload

_Existing `list_sendable_peers_excludes_local_peer_id` test provides baseline but needs extension._

---

## Manual-Only Verifications

| Behavior                     | Requirement | Why Manual                               | Test Instructions                                                                                    |
| ---------------------------- | ----------- | ---------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| dual mode no duplicate peers | dedup fix   | Requires two Tauri instances on same LAN | 1. `bun run tauri:dev:dual` 2. Open Setup pairing page on peerA 3. Verify peerB appears exactly once |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
