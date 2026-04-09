# Phase 54: Extract daemon client and realtime infrastructure from uc-tauri - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-23
**Phase:** 54-extract-daemon-client-and-realtime-infrastructure-from-uc-tauri
**Areas discussed:** 新 crate 归属, DaemonConnectionState 归属, 迁移策略, uc-tauri 对新 crate 的依赖方向

---

## 新 crate 归属

| Option                | Description                                                                                                                | Selected |
| --------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------- |
| 新建 uc-daemon-client | 独立 crate，职责单一："与 daemon 通信"。uc-tauri 和 uc-cli 都能依赖它。不会让 uc-daemon 膨胀                               | ✓        |
| 放进 uc-daemon lib    | uc-daemon 已有 [lib] section。优点：不新建 crate。缺点：uc-daemon 同时包含服务端和客户端代码，可能引入循环依赖             |          |
| 放进 uc-bootstrap     | 组合根 crate。优点：不新建 crate。缺点：uc-bootstrap 的职责是"组装依赖"而不是"运行时通信"，会引入 reqwest/tungstenite 依赖 |          |

**User's choice:** 新建 uc-daemon-client
**Notes:** 用户选择了推荐方案，职责分离最清晰

---

## DaemonConnectionState 归属

| Option                | Description                                                                              | Selected |
| --------------------- | ---------------------------------------------------------------------------------------- | -------- |
| 放进 uc-daemon-client | 跟 daemon 客户端代码住在一起。"连接状态"就是客户端的关切                                 | ✓        |
| 放进 uc-daemon lib    | 和 DaemonConnectionInfo 在同一个 crate。逻辑连贯，但会让 uc-daemon-client 依赖 uc-daemon |          |
| 放进 uc-core          | 作为域层类型。但它不是业务概念，是基础设施类型，放在 core 会污染域层                     |          |

**User's choice:** 放进 uc-daemon-client
**Notes:** uc-daemon-client 会依赖 uc-daemon (lib) 获取 DaemonConnectionInfo 类型

---

## 迁移策略

| Option                | Description                                                                    | Selected |
| --------------------- | ------------------------------------------------------------------------------ | -------- |
| Re-export stub 渐进式 | Phase 40 已验证的模式：先搬代码，uc-tauri 保留 pub use re-export。后续逐步清理 |          |
| 一次性完成            | 直接搬代码，同时更新所有调用方的 import 路径。不留 re-export stub              | ✓        |

**User's choice:** 一次性完成
**Notes:** 用户偏好干净利落，不想留历史包袱

---

## uc-tauri 对新 crate 的依赖方向

| Option                     | Description                                                             | Selected |
| -------------------------- | ----------------------------------------------------------------------- | -------- |
| 直接依赖                   | uc-tauri Cargo.toml 直接加 uc-daemon-client。简单明确                   | ✓        |
| 通过 uc-bootstrap 间接获取 | uc-bootstrap 依赖 uc-daemon-client，通过 builder context 传入。过度间接 |          |

**User's choice:** 直接依赖
**Notes:** commands 直接 use uc_daemon_client::http::DaemonPairingClient

---

## Claude's Discretion

- module 内部组织细节
- test 文件归属
- DaemonWsBridgeConfig 是否跟随搬移

## Deferred Ideas

- EventHub 泛化（Issue #316）
- daemon_lifecycle.rs / setup_pairing_bridge.rs 提取 → Phase 55
- wiring.rs 部分提取——未排期
