# Phase 86: cli-host-join-flow-phase - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-03
**Phase:** 86-cli-host-join-flow-phase
**Areas discussed:** Phase 0 scope, Phase 1 module location, Phase 1 naming/boundary, Phase 2 session_id relationship, Phase 3 context structure + error handling + on_phase_changed

---

## Phase 0 Scope

| Option                                 | Description                                                        | Selected |
| -------------------------------------- | ------------------------------------------------------------------ | -------- |
| 只修 run_pair 可编译                   | 只修 if/else if、双重 if let、空分支、变量遮蔽；不加测试不加新功能 |          |
| 修 run_pair + 加测试                   | 在修复同时加最小回归测试，把现状钉住                               |          |
| 修 run_pair + 整理 Helper              | 修复+去掉重复变量遮蔽+把 Helper 里的字符串常量收口                 |          |
| 修 run_pair + Helper + state_signature | 全部做：修复代码 + 整理 Helper + 加 state_signature 用于 debug log | ✓        |

**User's choice:** 修 run_pair + Helper + state_signature
**Notes:** state_signature 用于"仅在状态变化时打印 debug log"；如果这个功能没有价值则整个删掉

---

## Phase 1 Module Location

| Option                   | Description                                                             | Selected |
| ------------------------ | ----------------------------------------------------------------------- | -------- |
| uc-daemon-client（推荐） | 放在 uc-daemon-client；daemon-client 已经依赖 uc-cli，解析逻辑离 DTO 近 | ✓        |
| uc-cli                   | 直接放在 uc-cli/src/commands/setup/parsed_state.rs；CLI 专用            |          |
| uc-core                  | 上移到 uc-core（ports 层）；协议解释属于核心 domain                     |          |

**User's choice:** uc-daemon-client

---

## Phase 1 Naming & Boundary

| Option                    | Description                                         | Selected |
| ------------------------- | --------------------------------------------------- | -------- |
| 保持两个独立 enum（推荐） | SetupHint + SetupVariant 分开；语义对应两个不同字段 | ✓        |
| 合并成 SetupStateKind     | 用一个 enum；根据 state + next_step_hint 联合推导   |          |

**User's choice:** 保持两个独立 enum

### Old Helpers Handling

| Option            | Description                                                                             | Selected |
| ----------------- | --------------------------------------------------------------------------------------- | -------- |
| deprecated 但保留 | 标记 #[deprecated] + 调用新 parse_setup_state()；等 Phase 3 稳定后在下个 milestone 删除 |          |
| 直接删除          | Phase 1 中直接删除；所有调用方迁移到新模块；更干净但风险稍高                            | ✓        |
| 内部化（私有化）  | 不标记 deprecated 但移到 parsed_state.rs 内部作为私有函数                               |          |

**User's choice:** 直接删除

---

## Phase 2 — session_id 与 phase 的关系

| Option                        | Description                                                                                 | Selected |
| ----------------------------- | ------------------------------------------------------------------------------------------- | -------- |
| 放在 phase variant 里（推荐） | NeedDecision { session_id }；phase 切换时自然丢弃                                           | ✓        |
| 分离到 HostCliSession         | phase 不带 session_id；session_id 作为 last_submitted_decision_session 在 HostCliSession 里 |          |

**User's choice:** 放在 phase variant 里

---

## Phase 3 — Error Handling

| Option                       | Description                                        | Selected |
| ---------------------------- | -------------------------------------------------- | -------- |
| abort（立即返回 EXIT_ERROR） | 网络/daemon 错误立即终止；用户需要重新 run_pair    | ✓        |
| retry 一次后 abort           | action 失败先 retry 一次；再次失败才 abort         |          |
| retry + spinner 提示         | 失败后显示错误 spinner 提示用户；用户按 Enter 重试 |          |

**User's choice:** abort immediately

---

## Phase 3 — on_phase_changed Scope

| Option                     | Description                                                                 | Selected |
| -------------------------- | --------------------------------------------------------------------------- | -------- |
| 仅处理 UI 状态变化（推荐） | 打印阶段切换的日志/提示；清理旧 spinner；不涉及业务逻辑                     | ✓        |
| 处理 UI + 幂等去重         | UI 变化 + 调用方决定是否需要 submit；业务逻辑仍然在 match arm 里            |          |
| 统一处理所有 side-effect   | on_phase_changed 里处理：打印提示 + enable/disable presence + refresh lease |          |

**User's choice:** 仅处理 UI 状态变化

---

## Claude's Discretion

The following were left to planner/researcher judgment:

- Exact location of `parsed_state.rs` within `uc-daemon-client/src/setup/`
- Exact `on_phase_changed` function signature (parameters, who implements it)
- How `HostCliSession` / `JoinCliSession` interact with the existing `DaemonPairingClient` for presence registration
- How `derive_host_phase` / `derive_join_phase` handle the `Completed` / `Canceled` terminal phases

## Deferred Ideas

- **Merge `prompt_host_verification` + `prompt_join_peer_confirmation`** into `prompt_peer_trust_confirmation(peer_label, short_code, title)` — noted as easy to do during Phase 86 execution; not separated as its own phase
- **state_signature deletion** — if state_signature feature has no value (no meaningful state changes to detect), it should be removed entirely in Phase 0, not kept as dead code
