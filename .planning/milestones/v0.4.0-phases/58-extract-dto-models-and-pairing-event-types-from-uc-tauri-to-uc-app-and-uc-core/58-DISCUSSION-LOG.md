# Phase 58: Extract DTO models and pairing event types from uc-tauri to uc-app and uc-core - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-25
**Phase:** 58-extract-dto-models-and-pairing-event-types-from-uc-tauri-to-uc-app-and-uc-core
**Areas discussed:** Type placement strategy, Extraction scope, Migration strategy, DaemonPairingRequestError

---

## Type Placement Strategy

### Aggregation DTOs (P2PPeerInfo, PairedPeer)

| Option         | Description                                                                 | Selected |
| -------------- | --------------------------------------------------------------------------- | -------- |
| uc-app（推荐） | 跟 P2pPeerSnapshot 一样放 uc-app — 它们是应用层聚合查询结果，不是纯领域模型 | ✓        |
| uc-core        | 放 uc-core 领域层 — 使其成为所有层都能直接使用的基础类型                    |          |
| 你决定         | 让 Claude 根据现有代码模式自行判断                                          |          |

**User's choice:** uc-app（推荐）
**Notes:** Follows established pattern — P2pPeerSnapshot already in uc-app

### Pairing Event Types (P2PPairingVerificationEvent, P2PPairingVerificationKind)

| Option          | Description                                                            | Selected |
| --------------- | ---------------------------------------------------------------------- | -------- |
| uc-core（推荐） | 跟 HostEvent/RealtimeFrontendEvent 一样放 uc-core — 它们是跨层事件合约 | ✓        |
| uc-app          | 放 uc-app — 与配对 use case 一起，属于应用层关注点                     |          |
| 你决定          | 让 Claude 根据代码模式自行判断                                         |          |

**User's choice:** uc-core（推荐）
**Notes:** Consistent with existing event type placement in uc-core

---

## Extraction Scope

### Clipboard DTOs (11 types in uc-tauri/models/)

| Option         | Description                                                                 | Selected |
| -------------- | --------------------------------------------------------------------------- | -------- |
| 不提取（推荐） | 保留在 uc-tauri — 它们是纯前端投影 DTO，daemon/CLI 不需要。未来有需要再提取 |          |
| 一并提取       | 趁这次一起移到 uc-app，保持 uc-tauri 的 models/ 干净                        | ✓        |
| 你决定         | 让 Claude 根据实际依赖关系判断                                              |          |

**User's choice:** 一并提取
**Notes:** User wants uc-tauri models/ completely clean after this phase

### Frontend Request/Response DTOs (P2PPairingRequest/Response/PinVerifyRequest)

| Option         | Description                                                   | Selected |
| -------------- | ------------------------------------------------------------- | -------- |
| 不提取（推荐） | 保留在 uc-tauri — 这些是 Tauri command 的前端协议 DTO，不共享 |          |
| 提取到 uc-app  | 移到 uc-app 以备将来 daemon API 也能复用同样的请求类型        | ✓        |
| 你决定         | 让 Claude 判断                                                |          |

**User's choice:** 提取到 uc-app
**Notes:** User chose broader extraction for future reuse by daemon API

---

## Migration Strategy

| Option                       | Description                                                                     | Selected |
| ---------------------------- | ------------------------------------------------------------------------------- | -------- |
| 直接删除+更新 import（推荐） | 彻底移除原文件，所有 import 路径更新为新位置。干净利落，无残留                  | ✓        |
| Re-export stub               | 原位置保留 pub use uc_app::xxx 重导出，不破坏现有 import。Phase 40 用过这个模式 |          |
| 你决定                       | 让 Claude 根据具体情况判断                                                      |          |

**User's choice:** 直接删除+更新 import（推荐）
**Notes:** Clean cut, no backward compatibility stubs

---

## DaemonPairingRequestError

| Option                   | Description                                                  | Selected |
| ------------------------ | ------------------------------------------------------------ | -------- |
| uc-daemon-client（推荐） | 它是 daemon 客户端请求错误类型，逻辑属于 daemon-client crate | ✓        |
| uc-app                   | 放到应用层，与其他配对 DTO 一起                              |          |
| 你决定                   | 让 Claude 根据依赖关系判断                                   |          |

**User's choice:** uc-daemon-client（推荐）
**Notes:** None

---

## Claude's Discretion

- Exact module organization within uc-app for extracted DTOs
- Exact file within uc-core for pairing event types
- Order of extraction
- Helper method placement

## Deferred Ideas

- "修复 setup 配对确认提示缺失" — reviewed, not folded (UI bug, unrelated to extraction)
