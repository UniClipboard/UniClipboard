---
id: update-dialog-bg-download
title: 点"后台下载"后 update dialog 立即关闭，侧边栏图标显示进度
topology: single
runtime: [linux, windows]
requires_fixture: update-mock
modules:
  - src/components/layout/Sidebar.tsx
event_paths:
  - update.phase_changed
selectors:
  update_icon: '[data-testid="sidebar-update-icon"]'
  update_dialog: '[data-testid="update-dialog"]'
  bg_download_btn: '[data-testid="update-bg-download"]'
  sidebar_progress_ring: '[data-testid="sidebar-update-icon"][data-update-phase="downloading"]'
  dialog_progress_bar: '[data-testid="update-dialog"] [data-update-phase="downloading"]'
  cancel_download_btn: '[data-testid="update-cancel-download"]'
  toast_error: '[data-testid="toast-error"]'
budget_ms: 500
preconditions:
  - update-mock fixture 已注入：update 状态置为 `available`，download 行为可由测试切换 success / reject
---

## 前置

- A 端单机启动
- update-mock fixture 把 update phase 置为 `available`；download 行为可在两种模式之间切换：`success`（缓慢下载完成）/ `reject`（短延迟后 reject）

## 步骤（成功路径）

1. 切换 update-mock 为 `success` 模式
2. 点击 `[data-testid="sidebar-update-icon"]` 打开 dialog
3. 在 dialog 内点击 `[data-testid="update-bg-download"]`
4. 立即（budget_ms 内）检查 dialog 状态
5. 检查侧边栏图标是否进入下载态
6. 再次点击侧边栏图标重新打开 dialog

## 断言（成功路径）

- 步骤 4：`[data-testid="update-dialog"]` 不可见（已关闭）
- 步骤 5：`[data-testid="sidebar-update-icon"][data-update-phase="downloading"]` 在 DOM 中存在
- 步骤 6：dialog 重新打开后，`[data-testid="update-dialog"] [data-update-phase="downloading"]` 在 dialog 内可见；`[data-testid="update-cancel-download"]` 可点击

## 步骤（失败路径）

1. 切换 update-mock 为 `reject` 模式
2. 点击侧边栏图标 → 点击"后台下载"
3. 等待 reject 发生（mock 决定时机，通常 <500ms）

## 断言（失败路径）

- `[data-testid="toast-error"]` 出现，文本含 i18n key `update.downloadFailed` 对应的当前 locale 文本
- 侧边栏图标 `data-update-phase` 回退到 `available`

## 已知失败模式

- dialog 在点击后仍可见 → 回归：又把 `setUpdateDialogOpen(false)` 放回了 `await downloadUpdate()` 之后，下载不完成 dialog 不关
- 成功路径下取消按钮不可点 → 嫌疑：`UpdateContext` phase 状态机断裂；或 dialog 重开时未读取 context 当前 phase
- 失败路径下 toast 缺失 → 嫌疑：`.catch` 分支被删 / i18n key 改名
- 失败路径下 phase 不回退 → 嫌疑：reject 路径未通知 `UpdateContext` 复位
