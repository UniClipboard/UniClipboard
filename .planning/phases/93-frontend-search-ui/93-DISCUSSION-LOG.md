# Phase 93: Frontend Search UI - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions captured in CONTEXT.md — this log preserves the discussion.

**Date:** 2026-04-11
**Phase:** 93-frontend-search-ui
**Mode:** discuss
**Areas discussed:** UX 变更沟通, Dashboard Header 布局, 搜索 vs 浏览模式切换, 时间范围选择器形式

## Gray Areas Presented

| Area | Description |
|------|-------------|
| UX 变更沟通 | substring → 精确 token 是破坏性变更；STATE.md 要求在 Phase 93 前决定 |
| Dashboard Header 布局 | 现有 Header 只有内容类型 pills，加入搜索框 + 时间范围后如何排列 |
| 搜索 vs 浏览模式切换 | 有查询时列表如何表现：无缝替换 vs overlay |
| 时间范围选择器形式 | 下拉选择器 vs preset pills |

## Discussion Outcomes

### UX 变更沟通

| Question | User Choice | Reason |
|----------|-------------|--------|
| 如何让用户感知 substring → 精确 token 变更？ | 更新 placeholder 文字 | 低打扰，用户自然适应 |

### Dashboard Header 布局

| Question | User Choice |
|----------|-------------|
| 搜索框 + 时间范围 + 内容类型 pills 如何排列？ | 两行布局：Row 1 = 搜索框 + 时间范围下拉；Row 2 = 内容类型 pills |

### 搜索 vs 浏览模式切换

| Question | User Choice | Reason |
|----------|-------------|--------|
| 有查询时列表如何表现？ | 无缝替换 | 最简单，与 QuickPanel 行为一致 |

### 时间范围选择器形式

| Question | User Choice |
|----------|-------------|
| Preset 选择器 UI 形式？ | 下拉选择器（Dropdown），默认 "All time" |

## No Corrections

All areas discussed and resolved without contradictions.

## Deferred Ideas

- Absolute date range picker (custom `from_ms`/`to_ms`) — V2 follow-up
- Rebuild progress indicator in Dashboard — separate phase
