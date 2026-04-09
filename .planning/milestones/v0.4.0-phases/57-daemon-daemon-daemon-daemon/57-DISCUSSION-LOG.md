# Phase 57: Daemon Clipboard Watcher Integration - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-25
**Phase:** 57-daemon-daemon-daemon-daemon
**Areas discussed:** Monitoring Ownership Migration, Event Flow Architecture, clipboard_rs Thread Model, Clipboard Write

---

## Monitoring Ownership Migration

| Option                  | Description                                                                                                                                                         | Selected |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| GUI 完全移除监听 (推荐) | GUI 侧将 ClipboardIntegrationMode 设为 Passive，不再启动 ClipboardWatcherContext。剪切板变化统一由 daemon 捕获，通过 WS 事件通知 GUI 更新。避免双重监听和竞争问题。 | ✓        |
| GUI 保留监听作 fallback | daemon 运行时 GUI 不监听，daemon 不可用时 GUI 回退为自己监听。更复杂但提供容错。                                                                                    |          |

**User's choice:** GUI 完全移除监听
**Notes:** 无额外说明

---

## Event Flow Architecture — GUI 通知机制

| Option                     | Description                                                                                                                                             | Selected |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| 复用 DaemonWsBridge (推荐) | daemon 通过 WS broadcast 发送 clipboard.new_content 事件，GUI 的 DaemonWsBridge 接收并转译为现有的 Tauri 前端事件。复用已有 realtime 架构，无需新通道。 | ✓        |
| 新增 daemon HTTP API       | 前端通过 HTTP polling 或 HTTP long-poll 获取新剪切板内容。简单但延迟高。                                                                                |          |

**User's choice:** 复用 DaemonWsBridge
**Notes:** 无额外说明

---

## Event Flow Architecture — 业务逻辑触发

| Option                             | Description                                                                                                                                                                          | Selected |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------- |
| 复用 ClipboardChangeHandler (推荐) | 在 daemon 中构建 AppClipboardChangeHandler（类似 GUI 侧 AppRuntime 的实现），实现 ClipboardChangeHandler trait，调用 CaptureClipboardUseCase。复用已有端口和用例，不需新建事件管道。 | ✓        |
| DaemonService 直接调用 UseCase     | ClipboardWatcherWorker 直接拥有 CoreUseCases 引用，在 on_clipboard_change 中直接调用 capture_clipboard。更直接但耦合更紧。                                                           |          |

**User's choice:** 复用 ClipboardChangeHandler
**Notes:** 无额外说明

---

## clipboard_rs Thread Model

| Option                                   | Description                                                                                                                                                                                     | Selected |
| ---------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------- |
| spawn_blocking + shutdown channel (推荐) | 复用 PlatformRuntime 的现有模式: tokio::task::spawn_blocking 运行 watcher，通过 WatcherShutdown channel 停止。DaemonService::start() 等待 cancel token，收到后调用 shutdown。已经证明过的模式。 | ✓        |
| 抽象为独立 watcher service               | 将 clipboard_rs 的阻塞循环封装为独立服务，通过 mpsc channel 与 daemon 通信。更解耦但复杂度更高。                                                                                                |          |

**User's choice:** spawn_blocking + shutdown channel
**Notes:** 无额外说明

---

## Clipboard Write (Inbound Sync)

| Option                           | Description                                                                                                                          | Selected |
| -------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ | -------- |
| daemon 直接写入系统剪切板 (推荐) | daemon 拥有 SystemClipboardPort，收到远程同步内容后直接调用 write_snapshot()。简单直接，且 daemon 作为剪切板的唯一所有者，逻辑清晰。 | ✓        |
| 本 Phase 不处理写入迁移          | 本 Phase 只关注监听迁移，写入作为单独 Phase 处理。降低当前 Phase 复杂度。                                                            |          |

**User's choice:** daemon 直接写入系统剪切板
**Notes:** 无额外说明

---

## Claude's Discretion

- Exact event payload structure for clipboard.new_content WS event
- Whether to keep PlatformRuntime alive in reduced form or further simplify
- Error handling and retry for clipboard watcher failures
- Write-back loop prevention (suppressing self-triggered events after daemon writes)

## Deferred Ideas

None
