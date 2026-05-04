---
gsd_state_version: 1.0
milestone: v0.7.0
milestone_name: LAN-only Mode
status: planning
stopped_at: null
last_updated: "2026-05-04T00:00:00Z"
last_activity: 2026-05-04
progress:
  total_phases: 0
  completed_phases: 0
  total_plans: 0
  completed_plans: 0
  percent: 0
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-05-04)

**Core value:** Seamless clipboard synchronization across devices — copy on one, paste on another
**Current focus:** v0.7.0 LAN-only Mode — 把 iroh 公网中继回落做成用户可控开关，并暴露连接通道指示器

## Current Position

Phase: Not started (defining requirements)
Plan: —
Status: Defining requirements
Last activity: 2026-05-04 — Milestone v0.7.0 started

## Pending Todos

10 pending (see `.planning/todos/pending/`):
- 修复 setup 配对确认提示缺失 (ui)
- 给 FileTransferEvent::Cancelled 接入真实发射方 (file-transfer)
- 让 timeout sweep 走事件存储而非直接操作 projection (file-transfer)
- 复制图片跨设备同步时 narrow_to_primary 选中发送端本地文件路径导致对端粘贴失效 (clipboard-sync)
- 收口 setup v2 application 输入模型 (api)
- 收口 daemon clipboard HTTP 入口 (api)
- 收口 daemon search 入口 (api)
- 收口 daemon clipboard workers (clipboard-sync)
- 收口 daemon query peers ws (api)
- 收口 daemon 装配根 (architecture)

## Blockers/Concerns

无（v0.7.0 已启动，进入 requirements 定义）。

### Quick Tasks Completed

| # | Description | Date | Commit | Directory |
|---|-------------|------|--------|-----------|
| 260423-9do | Windows 多表示原子写入 + 解除平台层单 rep 契约 | 2026-04-23 | 2dde3312 | [260423-9do-windows-rep](./quick/260423-9do-windows-rep/) |
| 260423-mxu | macOS / Linux 多 rep 原子写入 — 当前这两平台仍走单 rep policy 降级 | 2026-04-23 | 0960e7ee | [260423-mxu-macos-linux-rep-rep-policy](./quick/260423-mxu-macos-linux-rep-rep-policy/) |

## Session Continuity

Last milestone archive completed: v0.5.0 on 2026-04-13 (audit backfilled 2026-05-04)
Current milestone: v0.7.0 LAN-only Mode (started 2026-05-04)
Next recommended step: confirm REQUIREMENTS.md → spawn gsd-roadmapper
