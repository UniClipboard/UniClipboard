---
id: delivery-bus-unregister
title: in-process 双栈下 daemon 关闭后 Tauri 端 delivery 事件仍 fire
topology: in-process-stack
runtime: [linux, windows]
modules:
  - src-tauri/crates/uc-application/src/usecases/clipboard_sync/host_event_bus.rs
  - src-tauri/crates/uc-desktop/src/daemon/app.rs
  - src-tauri/crates/uc-desktop/src/runtime.rs
  - src-tauri/crates/uc-bootstrap/src/assembly.rs
event_paths:
  - host.delivery.status_changed
  - bus.emitter.registered
  - bus.emitter.unregistered
selectors:
  delivery_badge_state: '[data-testid="clipboard-detail"] [data-delivery-state]'
budget_ms: 1500
preconditions:
  - A 端以 in-process 模式启动（daemon + Tauri 共进程，默认装配）
  - A 端与 B 已配对，B 端在线
---

## 前置

- 单机 A 上以共进程模式启动（daemon 模块和 Tauri 应用同进程）
- A 已与 B（独立机器或独立 UC_PROFILE）配对完成，A 端 Tauri 窗口打开任意 entry 的 detail view

## 步骤

1. 通过测试钩子（或 daemon 控制端口）向 A 进程发送 daemon 关停指令，使 daemon 模块退出
2. 检查 A 端日志包含 `bus.emitter.unregistered` 且 emitter name 指向 daemon 端 WS emitter
3. 在 B 端写入剪贴板 `e2e-bus-unreg-{{run_id}}`
4. 在 A 端 Tauri 窗口观察 `[data-delivery-state]` 变化与日志中 emit 路径

## 断言

- daemon 关闭后，A 端日志中不再出现 daemon 端 emit 路径
- A 端 Tauri 仍能在 `budget_ms` 内收到 `host.delivery.status_changed`，badge state 正常变化到终态
- 整个流程无 panic、无 "emitter already registered" / "emitter not found" 警告
- 同一事件未被重复 emit 到 Tauri 端（断言 badge state 变化日志只出现 1 次）

## 已知失败模式

- daemon 关闭后 Tauri 也收不到 delivery → 嫌疑：`HostEventBus.unregister` 误删了 Tauri emitter（命名冲突 / 名称未隔离）
- daemon 关闭后出现重复 emit → 嫌疑：`unregister` 未真正从 registry 移除，旧引用还在 fan-out
- 出现 panic → 嫌疑：daemon 关停时 emitter 持有的资源未优雅释放
