# Phase 92: Bootstrap Wiring and Daemon HTTP Routes - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in `92-CONTEXT.md`; this log preserves the alternatives considered.

**Date:** 2026-04-11
**Phase:** 92-Bootstrap Wiring and Daemon HTTP Routes
**Areas discussed:** Query Contract, Status Contract, Rebuild Progress Events

---

## Query Contract

### Question: Successful response shape

| Option | Description | Selected |
|--------|-------------|----------|
| 带总数列表 | Return result rows together with total-count metadata so later UI can show result counts directly. | ✓ |
| 纯列表 | Return only result rows; later work would need another source for total counts. | |
| 完整元数据 | Return rows plus richer paging/query echo metadata. | |

**User's choice:** 带总数列表
**Notes:** Successful `/search/query` responses should include the result list plus `total`.

### Question: Error shape

| Option | Description | Selected |
|--------|-------------|----------|
| 沿用简单错误 | Keep the existing daemon `{ code, message }` envelope and differentiate failures by code. | ✓ |
| 加提示字段 | Add hint/suggestion fields to search errors. | |
| 加调试细节 | Return lower-level parse/debug details in the API response. | |

**User's choice:** 沿用简单错误
**Notes:** Search-specific failures should stay inside the existing daemon error contract, with precise codes such as `invalid_query`, `session_locked`, and `index_not_ready`.

### Question: Filter parameter style

| Option | Description | Selected |
|--------|-------------|----------|
| 直白参数 | Use readable query parameters for query, operator, time filters, repeated file types/extensions, limit, and offset. | ✓ |
| 压缩参数 | Pack filters into a single encoded parameter. | |
| 混合风格 | Mix explicit basic params with a combined filters parameter. | |

**User's choice:** 直白参数
**Notes:** Keep `GET /search/query` easy to read and debug. Do not hide the filter contract inside an encoded blob.

### Question: Paging metadata

| Option | Description | Selected |
|--------|-------------|----------|
| 带 hasMore | Return `hasMore` in addition to `total` so clients know whether another page exists. | ✓ |
| 只要 total | Return only total count; clients infer paging themselves. | |
| 带完整分页回显 | Return limit/offset/hasMore together. | |

**User's choice:** 带 hasMore
**Notes:** Query responses should include both `total` and `hasMore`.

## Status Contract

### Question: Status response style

| Option | Description | Selected |
|--------|-------------|----------|
| 产品状态 | Return ready/locked/rebuilding-style product states that clients can render directly. | ✓ |
| 原始元信息 | Expose raw index metadata as the primary API contract. | |
| 混合返回 | Return both product-facing state and raw metadata. | |

**User's choice:** 产品状态
**Notes:** `/search/status` should be a product-facing contract, not a thin dump of `search_index_meta`.

### Question: Unavailable-state detail

| Option | Description | Selected |
|--------|-------------|----------|
| 状态+原因 | Keep a compact state model, but add a separate reason code for why search is unavailable. | ✓ |
| 只给大状态 | Return only broad states like ready / locked / notReady. | |
| 全量细分状态 | Split every unavailable case into its own top-level state value. | |

**User's choice:** 状态+原因
**Notes:** Status should distinguish locked session, first-time backfill, version-mismatch rebuild, manual rebuild, and rebuild-failed-waiting-for-retry.

## Rebuild Progress Events

### Question: Event granularity

| Option | Description | Selected |
|--------|-------------|----------|
| 开始+计数+结束 | Emit start, incremental `indexed/total` progress, and terminal complete/failed states. | ✓ |
| 只发节点 | Emit only start, complete, and failed. | |
| 超细粒度 | Emit more detailed/frequent progress updates. | |

**User's choice:** 开始+计数+结束
**Notes:** Rebuild progress should support real progress UI later, not only a generic busy indicator.

### Question: Event channel

| Option | Description | Selected |
|--------|-------------|----------|
| 独立 search 主题 | Give search its own WebSocket topic/stream, parallel to clipboard and file-transfer. | ✓ |
| 复用 status 主题 | Reuse the global status topic for search events. | |
| 复用其他主题 | Attach search progress to another existing topic. | |

**User's choice:** 独立 search 主题
**Notes:** Search rebuild progress should live on its own domain-scoped WebSocket stream.

## the agent's Discretion

- Exact DTO field names, as long as they preserve the decisions above.
- Exact progress emission cadence, as long as start, incremental counters, and terminal states remain visible.
- Exact reconnect rules between `/search/status` and WebSocket subscriptions.

## Deferred Ideas

- Rich hint/debug payloads for search API errors.
- Raw index metadata as the main public `/search/status` contract.
- `修复 setup 配对确认提示缺失` — reviewed and left out of Phase 92 scope.
