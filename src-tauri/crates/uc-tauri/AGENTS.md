# uc-tauri 指南

## 定位

`uc-tauri` 是 **`uc-desktop` 的 Tauri shell 适配器**——把桌面宿主层的能力
落到 Tauri 框架的 builder / event loop / command 上。

**它不是 desktop 的子集，也不是与 desktop 平级的层**。它是 desktop 在
"Tauri 这一种 GUI 框架"上的具体落地。未来如果有 `uc-macos-native` 之类
的 AppKit shell，会与 `uc-tauri` 平级，同样消费 `uc-desktop`。

```
src-tauri/main.rs
   ↓
uc-tauri  ← 你在这里（Tauri 特定的最后一公里）
   ↓
uc-desktop  ← 桌面宿主能力（GUI-framework agnostic）
   ↓
uc-application / uc-core
```

## 职责

`uc-tauri` 负责（且**仅**负责）Tauri 框架特定的事情：

- Tauri `Builder` 装配、`setup` 钩子、plugin 注册
- `#[tauri::command]` 注册与参数/错误适配
- Tauri 事件循环：`RunEvent::ExitRequested` / `Reopen` / 窗口事件
- Tauri 系统集成：`TrayIconBuilder` 托盘、`WebviewWindowBuilder` 窗口、
  `tauri-plugin-global-shortcut`、`tauri-plugin-autostart`、
  `tauri-plugin-stronghold`、`tauri-plugin-updater` 等
- Tauri 特有 API 包装：NSPanel quick_panel、Win32 窗口装饰
- `AppHandle` 的持有与传播
- 把 `uc-desktop` 提供的 `DesktopRuntime` 包成 Tauri State（`TauriAppRuntime`）
- 把 desktop 抽象（autostart trait、daemon spawn hook 等）注入 Tauri 实现

## 不负责

- ❌ 任何**业务规则**（业务规则在 `uc-application` / `uc-core`）
- ❌ 任何**框架无关的桌面宿主能力**——daemon 生命周期协调、后台任务调度
  循环、IPC 路径策略、健康检查策略，这些必须在 `uc-desktop` 里，shell
  之间共享
- ❌ `uc-application` 内部模块的直接调用（只能走 `AppFacade`）

## 边界判断（重要）

新增功能时反复问自己：

| 问题 | 答案 → 放哪 |
|---|---|
| 这段代码用了 `tauri::*` / `tauri-plugin-*` 类型吗？ | 是 → `uc-tauri`；否 → 看下一题 |
| 未来 `uc-macos-native` shell 也会需要这段逻辑吗？ | 是 → **必须放 `uc-desktop`**；否 → 可以放 `uc-tauri` |
| 这段代码定义业务规则 / 状态迁移 / 决策吗？ | 是 → `uc-application` 或 `uc-core` |

## 与 `uc-desktop` 的协作模式

- 启动期：`build_gui_app` → `DesktopRuntime` → `TauriAppRuntime`（包一层
  `Option<AppHandle>`）→ `app.manage(Arc<TauriAppRuntime>)`
- daemon 启动期由 `uc_desktop::daemon_probe::bootstrap_daemon_in_process`
  in-process 拉起 daemon main loop（不再走 sidecar / supervise 模型）
- 运行期：command handler 取 `State<Arc<TauriAppRuntime>>`，需要 facade
  时调 `runtime.desktop().app_facade()`，需要 Tauri 能力时调
  `runtime.app_handle()`

## 命令层规范

- Command handler 只做：参数解析 → 调 `AppFacade` 或 desktop 抽象 → 序列化
  返回值
- 禁止在 command 里写业务流程（"先查这个，再判断那个，再调那个"）；这种
  流程要么是已有的 use case，要么应该新加一个 use case
- Command span 在有 `_trace: Option<TraceMetadata>` 时记录 trace 字段
- 发给前端的 event payload 必须 camelCase serde rename

## 反模式

- 在 `uc-tauri` 里实现"跨 shell 共享"的逻辑（应在 `uc-desktop`）
- 在 command handler 里直接拼业务流程
- 通过 `runtime.desktop().deps`（如果暴露的话）绕过 facade
- 给 frontend 发 snake_case payload
- 在 `bootstrap/` 做大幅重构去 fix 一个小 bug

## 高风险文件

- `src/bootstrap/runtime.rs`（`TauriAppRuntime` 包装，影响所有 command 取数据的路径）
- `src/run.rs`（builder/setup/RunEvent 主回路与退出清理）
- `src/lib.rs`（`pub mod` 决定外部入口）

## 验证命令

```bash
# from src-tauri/
cargo check -p uc-tauri
cargo test -p uc-tauri
# 确保 desktop 没有被污染
cargo tree -p uc-desktop -e normal | grep -i tauri  # 必须无输出
```
