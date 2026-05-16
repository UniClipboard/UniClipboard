---
id: pairing-delivery-badge-realtime
title: 已配对双端复制后，B 端 detail view delivery badge 在 1.5s 内显示终态
topology: dual-device
runtime: [linux, windows]
modules:
  - src/components/clipboard/EntryDeliveryBadge.tsx
  - src/api/tauri-command/clipboard_delivery.ts
  - src-tauri/crates/uc-application/src/usecases/clipboard_sync/apply_inbound
  - src-tauri/crates/uc-application/src/usecases/clipboard_sync/host_event_bus.rs
event_paths:
  - host.delivery.status_changed
  - clipboard.delivery
selectors:
  delivery_badge_state: '[data-testid="clipboard-detail"] [data-delivery-state]'
  delivery_badge_reason: '[data-testid="clipboard-detail"] [data-delivery-reason]'
  current_entry_id: '[data-testid="clipboard-detail"] [data-entry-id]'
budget_ms: 1500
preconditions:
  - A 与 B 已通过 redeem code 配对（双端均完成 setup）
  - B 端主窗口 detail view 已打开任意一条 entry（不要求与即将到来的 entry 相同）
known_flakes:
  - 首次 mDNS 发现可能 >1s；卡片应重试 1 次，仍超 budget 才算失败
---

## 前置

- A、B 两个 Tauri 实例分别用 `UC_PROFILE=e2e-a` / `UC_PROFILE=e2e-b` 启动，已完成配对
- B 端 detail view 已打开任意一条历史 entry

## 步骤

1. 在 B 端记录当前 detail view 的 `[data-entry-id]`，记为 `before_entry`
2. 在 A 端写入剪贴板内容 `e2e-delivery-{{run_id}}`
3. 等待 B 端日志出现 `host.delivery.status_changed`，记录到达时间 `t1`
4. 不手动切换 entry，读取 B 端 `[data-delivery-state]`、`[data-delivery-reason]`、`[data-entry-id]`

## 断言

- `[data-delivery-state]` 在 `t1` 之后、`budget_ms` 内 ∈ {`delivered`, `duplicate`, `failed`}
- 若 state == `failed`，`[data-delivery-reason]` 非空且为已知 reason 枚举之一
- `[data-entry-id]` == `before_entry`（detail view 未被自动切走）

## 已知失败模式

- state 始终停在 `pending` → 嫌疑：`HostEventBus` 未注册 Tauri 端 emitter；或 `useEntryDelivery` 没订阅 `clipboard-delivery-status-changed`
- state 终态正确但 detail view 未刷新 → 嫌疑：事件 `entry_id` 与 hook subscribe 的 `entry_id` 不匹配，refetch 路径未触发
- state == `failed` 但 reason 空 → 嫌疑：`apply_inbound` 错误未透传到 `DeliveryFailureReason`
- detail view 被自动切到新 entry → 回归：dashboard 自动切换策略错误覆盖了"用户当前关注"
