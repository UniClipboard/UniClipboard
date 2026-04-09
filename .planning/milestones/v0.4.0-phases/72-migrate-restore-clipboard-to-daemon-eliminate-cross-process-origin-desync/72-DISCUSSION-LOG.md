# Phase 72: Migrate restore-clipboard to daemon — eliminate cross-process origin desync - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-03-29
**Phase:** 72-migrate-restore-clipboard-to-daemon-eliminate-cross-process-origin-desync
**Areas discussed:** Daemon API 设计, 恢复后的同步行为, GUI 兼容路径

---

## Daemon API 传输方式

| Option           | Description                                                                      | Selected |
| ---------------- | -------------------------------------------------------------------------------- | -------- |
| HTTP POST (推荐) | POST /clipboard/restore/{entry_id}，返回成功/失败。符合现有 daemon mutation 模式 | ✓        |
| WS command       | 通过 WS 发送 restore 命令，结果通过 WS 事件返回。更复杂但可以复用现有连接        |          |
| Claude 决定      | 让 Claude 根据技术约束选择最合适的方式                                           |          |

**User's choice:** HTTP POST
**Notes:** 符合现有 daemon mutation 模式 (pairing/setup 等都走 HTTP)

---

## API 响应设计

| Option          | Description                                                                             | Selected |
| --------------- | --------------------------------------------------------------------------------------- | -------- |
| 最小响应 (推荐) | 200 OK + { success: true } 或错误码。剩余状态通过现有 WS clipboard.new_content 事件推送 | ✓        |
| 丰富响应        | 200 + { entry_id, preview, content_type }。减少前端对 WS 事件的依赖                     |          |
| Claude 决定     | 根据实现复杂度选择                                                                      |          |

**User's choice:** 最小响应
**Notes:** 前端已通过 WS 事件获取 clipboard 更新，无需在 HTTP 响应中重复

---

## 恢复后的同步行为

| Option              | Description                                                                    | Selected |
| ------------------- | ------------------------------------------------------------------------------ | -------- |
| 保持现行行为 (同步) | restore 后触发 outbound sync，设备 B 也能获取到恢复的内容。行为与普通复制一致  | ✓        |
| 不同步              | restore 只影响本地 OS 剪贴板，不推送到其他设备。避免旧内容覆盖其他设备的新内容 |          |
| Claude 决定         | 根据技术分析和用户体验权衡选择                                                 |          |

**User's choice:** 保持现行行为 (同步)
**Notes:** 无

---

## GUI 兼容路径

| Option                   | Description                                                                                       | Selected |
| ------------------------ | ------------------------------------------------------------------------------------------------- | -------- |
| 强制全部走 daemon (推荐) | 移除 GUI 中的 restore 实现，强制所有 restore 通过 daemon HTTP API。简化架构但需要 daemon 必须运行 | ✓        |
| 保留 Full mode fallback  | daemon 可用时走 daemon，不可用时 fallback 到 GUI 直接 restore。更健壮但维护两套代码               |          |
| Claude 决定              | 根据当前架构方向选择                                                                              |          |

**User's choice:** 强制全部走 daemon
**Notes:** 符合 v0.4.0 "daemon 作为唯一业务引擎" 的架构方向

---

## Claude's Discretion

- Daemon-side implementation details (reuse RestoreClipboardSelectionUseCase vs new handler)
- Error mapping between daemon HTTP errors and Tauri command errors
- touch_clipboard_entry placement

## Deferred Ideas

None
