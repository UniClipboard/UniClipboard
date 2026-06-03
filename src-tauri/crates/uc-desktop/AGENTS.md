# uc-desktop 指南

## 定位

`uc-desktop` 是 UniClipboard 的 **桌面宿主层（desktop host layer）**，
不是业务层，**也不是任何特定 GUI 框架的实现**。

它负责把 UniClipboard 的 app runtime（`uc-application`）跑在桌面环境
里：接入系统能力、后台任务、HTTP/IPC、桌面事件源、daemon 进程协调。
**核心业务规则不在这里，特定 GUI 框架代码也不在这里。**

可以理解为：

- 业务规则归 `uc-core` / `uc-application`
- 桌面环境的「外壳」与「胶水」归 `uc-desktop`
- 具体 GUI 框架（Tauri webview、AppKit、未来其他）归各自的 **shell crate**

## ⚠️ 硬约束：GUI-framework agnostic

`uc-desktop` **禁止依赖任何 GUI / UI 框架**：

- ❌ `tauri` / `tauri-*` 任何插件
- ❌ `iced` / `egui` / `dioxus` / `slint`
- ❌ `objc2-app-kit` / `cocoa` / `core-graphics`（UI 层面）
- ❌ Win32 UI（`Win32_UI_*`）

允许（系统能力、非 UI）：

- ✅ `clipboard-rs`（OS 剪贴板事件源）
- ✅ `tokio` / `axum` / `tokio-util`
- ✅ 文件系统、进程管理、IPC、网络

如果某项能力同时 **同时** 有"GUI 框架版本"和"系统能力版本"，desktop 只能
依赖系统能力版本；GUI 适配交给 shell crate。

### 已知 shell（桌面 GUI 实现）

| Shell crate | GUI 框架 | 进程模型 |
|---|---|---|
| `uc-tauri` | Tauri webview | GUI 进程 |
| `uc-macos-native`（规划） | AppKit / SwiftUI | GUI 进程 |
| `uc-daemon` | 无 GUI | 后台 sidecar 进程 |

每个 shell crate 是 `uc-desktop` 的 **consumer + 框架适配器**，提供"最后
一公里"——把桌面宿主能力落到具体框架的 builder / event loop / command。

它可以负责（shell 之间共享的、框架无关的部分）：

- 组装桌面运行时（`DesktopRuntime`：facade、task_registry、settings、storage、event_emitter）
- daemon 生命周期与进程模型协调（spawn 由 shell 注入 hook）
- 本地 HTTP / WebSocket / IPC 接入（桌面侧路由、鉴权外壳）
- 桌面事件源（剪贴板监听、文件系统、电源/网络事件）
- 后台任务调度运行时（tokio task registry、定时器、循环）
- 系统能力抽象（autostart / 单实例 / 通知 / 托盘等的 trait 与共用策略，**不** 含任何 GUI 框架的具体 builder）
- 桌面侧策略（IPC 路径约定、token 策略、健康检查）

它不负责：

- 任何 GUI 框架的 builder / setup / event loop / command 注册
- 任何 webview / window / 系统托盘的具体绘制 / NSPanel / Win32 窗口操作
- setup 状态迁移规则
- pairing 协议推进
- sync 决策
- transfer 会话决策
- 剪贴板内容分类、去重、压缩等内容语义规则
- 设备/用户身份的核心模型

业务能力必须留在 `uc-application` 或 `uc-core`。GUI 框架特定能力必须留在
shell crate。如果在 desktop 里需要写 `if cfg!(feature = "tauri")` 或
`use tauri::*`，**那是错的**——应该把抽象提取到 trait，shell 各自实现。

## 边界规则

- 外部业务调用只走 `uc_application::facade::AppFacade`。
- 不要在 HTTP handler、daemon worker 里重新拼业务流程；这些入口只做
  参数解析、鉴权外壳、转调 facade、序列化结果。**Tauri command 等
  GUI 框架入口在 shell crate 里，desktop 不直接拥有它们**。
- 事件源只负责监听桌面事件，并把事件 **原样** 交给应用层入口；不在
  desktop 层做内容判断或路由决策。
- 后台任务的 **运行时调度**（什么时候触发、跑在哪个线程）可以在这里，
  任务的 **业务定义**（具体做什么、状态如何流转）放在应用层。
- 桌面侧只能依赖 `uc-application` / `uc-core` 暴露的稳定接口，禁止
  依赖应用层内部模块。
- 涉及外部进程/UI 框架的扩展点（如 daemon spawn、托盘渲染、autostart 写入），
  desktop 提供 trait + 默认协调逻辑，shell 注入具体实现。
- daemon runtime + host entry points 已全部迁至 `uc-daemon`（ADR-008 P1+P2）；
  本 crate `src/daemon/` 仅保留 **re-export shim**，`src/bootstrap.rs` 同理。
  `uc-daemon` 产出 `uniclipd` 独立二进制。

## 当前落地边界

- daemon runtime + host entry points + process bootstrap 已全部迁至 `uc-daemon`
  （ADR-008 P1+P2）。本 crate `src/daemon/` 和 `src/bootstrap.rs` 现在是纯
  re-export shim。`uc_desktop::daemon::*` / `uc_desktop::bootstrap::*` 公共面
  经 re-export 保持不变；新增 runtime 构件请加到 `uc-daemon`，**不要** 回流本 crate。
  `uc-cli` 已不再依赖本 crate（P2 Slice 2d）。
- `uc-webserver` 暂时保持独立 crate，由 `uc-desktop` 作为宿主调用；不要为了
  目录一致性直接把 HTTP/WS 物理迁入 `uc-desktop`。它不依赖 GUI 框架，符合
  "可被多 shell 共享" 的约束。
- `uc-daemon-local` 是 desktop 宿主的"进程协调工具集"——逻辑上属于 desktop
  范畴，因为需要被 GUI shell 与 daemon 同时消费而物理外置。它**不依赖任何
  GUI 框架**，仅承载 PID 文件、socket 路径、auth token、健康探测、
  错误契约等纯协调工具。ADR-008 P3-3 (B2'-3) 起 GUI 是外部 daemon 的纯
  客户端：探测到没有 daemon 时，`daemon_probe::bootstrap_daemon_in_process`
  调 `uc_daemon_local::spawn::spawn_detached_daemon` detached 拉起 `uniclipd`
  外部进程 (GUI 与 CLI 共用同一 spawn 原语),再 poll `/health`。不再有
  in-process daemon。
- `uc-tauri` 是 desktop 的 **Tauri shell 适配器**，不是 desktop 的子集，
  也不是与 desktop 平级的层。它消费 `uc-desktop` 的能力，提供 Tauri 框架
  特定的 builder / commands / tray / quick_panel。新增 Tauri-only 能力放
  这里，新增"未来 native shell 也会用到的"能力放 `uc-desktop`。
- `uc-daemon`（[ADR-008](../../../docs/architecture/adr-008-uniclipd-split-gui-as-client.md) P1+P2 已落地）：
    承载 **GUI-agnostic daemon runtime 全部构件**（run_mode、后台 worker / 服务、
    装配链、main loop、startup recovery、process bootstrap、host entry points）+
    `uniclipd` 独立二进制。不依赖 GUI 框架、**不反依赖 `uc-desktop`**。
    ADR-008 P3-3 (B2'-3) 已落地：GUI 永久转纯 client，删除 in-process
    daemon 拉起路径 (`start_in_process`/`ProcessRuntimeHandles` re-export
    shim 已移除);`GuiInProcess` run-mode 变体作为死代码在后续 cleanup
    中删除，`DaemonProcessMode::InProcess` enum 保留供 legacy PID 文件读取。
