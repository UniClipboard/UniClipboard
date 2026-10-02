---
id: daemon-ws-delivery-skip
title: daemon WS 客户端收 clipboard / transfer 事件但不收 delivery
topology: daemon-only
runtime: [linux, windows]
modules:
  - src-tauri/crates/uc-webserver/src/api/event_emitter.rs
  - src-tauri/crates/uc-application/src/usecases/clipboard_sync/host_event_bus.rs
event_paths:
  - daemon.ws.event
  - host.delivery.status_changed
  - clipboard.received
  - transfer.progress
selectors:
  # GUI 对照断言用：A 端 Tauri 主窗口 detail panel 上的 delivery badge
  main_detail_badge: '[data-testid="clipboard-detail"] [data-delivery-summary]'
budget_ms: 3000
preconditions:
  - A 端 in-process 模式启动，已与 B 配对
  - 测试 WS 客户端已连接 A 的 daemon WS 端口并订阅全部事件分类
---

## 前置

- A 端 daemon WS 暴露在已知端口
- 测试客户端订阅 `clipboard` / `transfer` / `delivery` 全部三类（用于验证哪些到达、哪些被刻意跳过）
- B 端在线并已配对

## 步骤

1. 在 B 端复制内容，触发 A 端 inbound 流程
2. 在 A 端通过 dispatch 路径触发 outbound delivery（确保产生 delivery 事件，让 Tauri 端可见）
3. 在测试 WS 客户端收集 `budget_ms` 内的所有事件，按分类聚合
4. 同步检查 A 端 Tauri 窗口的 detail view，确认 delivery 事件在 GUI 端正常 fire（对照组）

## 断言

- 测试 WS 客户端至少收到 1 条 `clipboard` 分类事件（A 端 inbound 成功落库时）
- 测试 WS 客户端至少收到 1 条 `transfer` 分类事件（dispatch 流转时）
- 测试 WS 客户端 **零** 条 `delivery` 分类事件（`DaemonApiEventEmitter` 明确跳过，GUI-only）
- A 端 Tauri 窗口 GUI 侧的 delivery badge 仍然正常变化（确认跳过只针对 daemon WS 而非全局）

## 已知失败模式

- 收到 delivery 事件 → 嫌疑：`DaemonApiEventEmitter` 未正确 filter Delivery 分类（回归）
- 完全收不到 clipboard / transfer → 嫌疑：daemon WS emitter 未注册到 `HostEventBus`；或 WS 客户端未订阅对应分类
- GUI 侧也收不到 delivery → 这条卡片不会覆盖（属于 `pairing-delivery-badge-realtime` 的责任）；若发生，归因 agent 应指出"另一张卡片应同时失败，否则两边断言不自洽"
