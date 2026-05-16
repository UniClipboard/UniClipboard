---
id: quick-panel-delivery-badge
title: quick-panel 标题栏显示 delivery badge 且不挤占 preview 高度
topology: single
runtime: [linux, windows]
modules:
  - src/quick-panel/ClipboardPreviewPane.tsx
  - src/components/clipboard/EntryDeliveryBadge.tsx
  - src/components/clipboard/ClipboardPreviewInfo.tsx
selectors:
  quick_panel_titlebar_badge: '[data-testid="quick-panel-titlebar"] [data-delivery-summary]'
  # popover 通过 Radix portal 渲染到 body 末尾，不在 titlebar 子树，selector 不限祖先
  quick_panel_titlebar_popover: '[data-delivery-popover]'
  quick_panel_preview_area: '[data-testid="quick-panel-preview-area"]'
  main_detail_badge: '[data-testid="clipboard-detail"] [data-delivery-summary]'
budget_ms: 1000
preconditions:
  - clipboard 历史至少含两条 entry：E1（有 delivery 记录）、E2（无 delivery 记录）
  - 主窗口和 quick-panel 都可访问
---

## 前置

- A 端单机启动；clipboard 历史预置或由 fixture 注入两条 entry：E1 有 delivery 记录、E2 无 delivery 记录
- 主窗口与 quick-panel 均能正常打开

## 步骤

1. 打开 quick-panel，选中 E1
2. 在 E1 标题栏区域读取 badge state；hover 到 badge 上读取 popover 内容
3. 记录 `[data-testid="quick-panel-preview-area"]` 的高度为 `h_e1`
4. 切到 E2，记录同一节点高度为 `h_e2`
5. 打开主窗口 detail panel，导航到 E1，读取 badge state

## 断言

- E1 quick-panel 标题栏 `[data-delivery-summary]` 存在且 ∈ {`synced`, `syncing`, `partial`, `failed`, `pending`}
- E1 hover 后 `[data-delivery-popover]` 在 document 内出现（Radix portal 不在 titlebar 子树）；内容含 device-name，缺失时回落到 truncated device-id
- `h_e1 == h_e2`（badge 不挤占 preview 高度，不引入额外行）
- 主窗口 E1 detail panel 的 `[data-delivery-summary]` 与 quick-panel 一致（行为未回归）

## 已知失败模式

- `h_e1 > h_e2` → 嫌疑：badge 未真正进入 titlebar，仍占额外 row（refactor 不彻底）
- 主窗口 detail panel badge 缺失 → 嫌疑：refactor 误删了 `ClipboardPreviewInfo` 渲染路径
- popover 显示原始 device-id 而非 device-name → 嫌疑：device-name 回落策略错误，或 device 元数据未传到 popover
