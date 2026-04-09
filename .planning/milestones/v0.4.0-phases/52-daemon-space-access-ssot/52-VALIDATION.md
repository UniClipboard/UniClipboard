---
phase: 52
slug: daemon-space-access-ssot
status: draft
nyquist_compliant: false
wave_0_complete: false
created: 2026-03-23
---

# Phase 52 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property               | Value                                                          |
| ---------------------- | -------------------------------------------------------------- |
| **Framework**          | Rust cargo test + Vitest (TypeScript)                          |
| **Config file**        | `src-tauri/` (cargo test), `vitest.config.ts`                  |
| **Quick run command**  | `cd src-tauri && cargo test -p uc-daemon -- space_access 2>&1` |
| **Full suite command** | `cd src-tauri && cargo test && bun test`                       |
| **Estimated runtime**  | ~60 seconds                                                    |

---

## Sampling Rate

- **After every task commit:** Run `cd src-tauri && cargo test -p uc-daemon -- space_access`
- **After every plan wave:** Run `cd src-tauri && cargo test && bun test`
- **Before `/gsd:verify-work`:** Full suite must be green
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

| Behavior                                                    | Test Type | Automated Command                                               | Notes                                      | Status     |
| ----------------------------------------------------------- | --------- | --------------------------------------------------------------- | ------------------------------------------ | ---------- |
| `space_access.state_changed` WS 事件在 daemon 侧被广播      | unit      | `cd src-tauri && cargo test -p uc-daemon -- space_access`       | 参照 `DaemonApiEventEmitter` 现有测试模式  | ⬜ pending |
| `DaemonWsBridge` 正确翻译 `space_access.state_changed` 事件 | unit      | `cd src-tauri && cargo test -p uc-tauri -- space_access`        | 参照 `daemon_ws_bridge.rs` 现有 peers 测试 | ⬜ pending |
| GUI 端 `space_access_orchestrator` 字段不再存在             | compile   | `cd src-tauri && cargo check`                                   | 编译不再引用相关字段即通过                 | ⬜ pending |
| `GET /space-access/state` 返回正确状态                      | unit      | `cd src-tauri && cargo test -p uc-daemon -- space_access_state` | 参照 setup HTTP 路由测试                   | ⬜ pending |

_Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky_

---

## Wave 0 Requirements

- [ ] `src-tauri/crates/uc-daemon/tests/` 中新增 `space_access_ws_event` 测试（覆盖 DaemonApiEventEmitter 广播行为）
- [ ] `src-tauri/crates/uc-tauri/tests/daemon_ws_bridge.rs` 新增 `space_access_state_changed` 翻译测试（现有 fixture 需清理 `space_access_orchestrator` 字段）

---

## Manual-Only Verifications

| Behavior                                        | Why Manual           | Test Instructions                                                           |
| ----------------------------------------------- | -------------------- | --------------------------------------------------------------------------- |
| 前端 space access UI 响应 daemon 推送的状态变更 | 需要观察 UI 渲染效果 | 1. 启动 daemon + GUI 2. 发起 space access 流程 3. 观察 GUI 实时显示状态变更 |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
