---
gsd_state_version: 1.0
milestone: v0.7.0
milestone_name: LAN-only Mode
status: Phase 95 verifier=gaps_found (16/17 must-haves); 1 gap blocking closing-loop — dev-mode `app.restart()` respawn 不可达。next: `/gsd-plan-phase 95 --gaps`
last_updated: "2026-05-05T00:40:00.000Z"
last_activity: 2026-05-05 -- Phase 95 verifier 16/17 PASS（自动化 54/54、所有 fence 0 命中、5/6 UAT PASS）。唯一 gap：Tauri 2 + bun + macOS 组合下 `tauri:dev` 不 respawn binary 导致 NETSET-05 closing-loop 半生效。VERIFICATION.md 已 commit。
progress:
  total_phases: 4
  completed_phases: 1
  total_plans: 6
  completed_plans: 6
  percent: 100
---

# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-05-04)

**Core value:** Seamless clipboard synchronization across devices — copy on one, paste on another
**Current focus:** v0.7.0 LAN-only Mode — 把 iroh 公网中继回落做成用户可控开关，并暴露连接通道指示器

## Current Position

Phase: 95 前端 NetworkSection + 重启 UX — 自动化全部完成 (6/6 plans automated)，等用户人工 UAT
Plan: 6 plans (095-01 ~ 095-06)，Wave 1 ✅、Wave 2 ✅、Wave 3 ✅ automated；checkpoint 95-06 等用户 UAT
Status: 6 plans merged，前端 372/372 PASS、cargo restart 6/6 PASS、Pitfall 1/5/11 + D-A1/D-C1 fence 0 命中
Last activity: 2026-05-05 -- Wave 3 落地：95-06 NetworkSection 完全重写（占位 25 行 → 实装 175 行）；checkpoint 等用户 6 项 UAT

## Roadmap

| Phase | Name                              | Requirements                                | 依赖           | 状态                |
|-------|-----------------------------------|---------------------------------------------|----------------|---------------------|
| 94    | 后端字段落地                      | NETSET-01, NETSET-02, NETSET-03             | 无             | ✅ 完成              |
| 95    | 前端 NetworkSection + 重启 UX     | NETSET-04, NETSET-05, NETSET-06             | Phase 94       | ⚠️ gaps_found (16/17) |
| 96    | 连接通道指示器                    | INDIC-01, INDIC-02, INDIC-03, INDIC-04      | Phase 94       | 待开始              |
| 97    | onboarding + 文档 + 跨平台 QA     | ONBORD-01, DOC-01, DOC-02, DOC-03           | Phase 95, 96   | 待开始              |

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

无 BLOCKER。Phase 94 完整闭环 — 自动测试 + 人 UAT 双重通过，可推进 Phase 95。

### Quick Tasks Completed

| # | Description | Date | Commit | Directory |
|---|-------------|------|--------|-----------|
| 260423-9do | Windows 多表示原子写入 + 解除平台层单 rep 契约 | 2026-04-23 | 2dde3312 | [260423-9do-windows-rep](./quick/260423-9do-windows-rep/) |
| 260423-mxu | macOS / Linux 多 rep 原子写入 — 当前这两平台仍走单 rep policy 降级 | 2026-04-23 | 0960e7ee | [260423-mxu-macos-linux-rep-rep-policy](./quick/260423-mxu-macos-linux-rep-rep-policy/) |
| 260505-keychain-prompts | 减少 macOS 首次使用时 Keychain 多次弹窗（kek_observed 进程级缓存） | 2026-05-05 | 39ce6f39 | [260505-keychain-prompts](./quick/260505-keychain-prompts/) |
| 260505-iroh-identity-file-storage | 启动期 iroh 设备身份脱离 macOS Keychain，改走 0600 文件后端，彻底消除"用户没操作就弹"的根因 | 2026-05-05 | aa1b1d93 | [260505-iroh-identity-file-storage](./quick/260505-iroh-identity-file-storage/) |
| 260505-keychain-startup-resume-gate | daemon startup_recovery 守 try_resume_session — auto_unlock=false 时不再下沉到 keychain | 2026-05-05 | 5912465c | [260505-keychain-startup-resume-gate](./quick/260505-keychain-startup-resume-gate/) |

## Session Continuity

Last milestone archive completed: v0.5.0 on 2026-04-13 (audit backfilled 2026-05-04)
Current milestone: v0.7.0 LAN-only Mode (started 2026-05-04)
Phase 94 context gathered: 2026-05-04 — 4 gray areas resolved (A1 / B3 / C1 / D1)
Phase 94 execution complete: 2026-05-04 — 6/6 plans, 53/53 自动测试 PASS, 4 个 pitfall 防御铁律全部 VERIFIED
Phase 94 human UAT complete: 2026-05-04 — 2/2 PASSED via real daemon log（双向证据：home_relay 注册/缺席对比 + 配置翻译 tracing + LAN-only 死循环重试外网 peer）
Phase 95 context gathered: 2026-05-04 — 4 灰色地带全敲定（RestartBanner / Section 内部 / 只靠 Banner / Tauri GUI 范围 / Popover / get_restart_state Tauri command）
Phase 95 UI-SPEC.md approved: 2026-05-04 — gsd-ui-checker 6/6 维度 PASS（双语 i18n 字典最终敲定，Phase 97 反向复制基准）
Phase 95 plans ready: 2026-05-04 — 6 PLAN.md（5 TDD + 1 execute），plan-checker 12/12 PASS（D-A1..D-D3 全覆盖 / Phase 94 边界锁定 / Pitfall 5/10/11 防御铁律 / PATTERNS.md 12/12 文件 analog 全引用）
Next recommended step: `/gsd-execute-phase 95` 执行 Phase 95（建议 wave 1 [01,02,03] 并行；wave 2 [04,05] 并行；wave 3 [06] 含人工 UAT checkpoint）
