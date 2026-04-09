# Phase 57: Daemon Clipboard Watcher Integration - Research

**Researched:** 2026-03-25
**Domain:** Rust daemon service lifecycle, clipboard_rs integration, cross-process event emission
**Confidence:** HIGH

<user_constraints>

## User Constraints (from CONTEXT.md)

### Locked Decisions

- **D-01:** GUI (Tauri) 侧设置 `ClipboardIntegrationMode::Passive` — 不再启动 `ClipboardWatcherContext`。Daemon 是唯一观察 OS 剪切板变更的进程。
- **D-02:** `uc-daemon/workers/clipboard_watcher.rs` 中的现有占位符 `ClipboardWatcherWorker` 替换为调用 `clipboard_rs::ClipboardWatcherContext` 的真实实现。
- **D-03:** GUI PlatformRuntime 的 `start_clipboard_watcher()` 方法和 `WatcherControlPort`-based 启动路径停用或删除。GUI `PlatformRuntime` 不再拥有剪切板监听。
- **D-04:** Daemon 通过 `ClipboardWatcher`（`uc-platform/clipboard/watcher.rs`）捕获剪切板变更，并通过现有 `ClipboardChangeHandler` trait 触发业务逻辑。
- **D-05:** 在 daemon 启动时构建 `AppClipboardChangeHandler`（类似于 GUI 的 `AppRuntime` 构建方式），调用 `CaptureClipboardUseCase` 持久化条目并发出事件。
- **D-06:** Daemon 通过现有 `DaemonWsBridge` WebSocket 基础设施通知 GUI 新剪切板内容 — `clipboard.new_content` 事件通过 `event_tx` 广播，由 Tauri 进程中的 `DaemonWsBridge` 接收，并转换为现有前端事件契约。
- **D-07:** 使用 `tokio::task::spawn_blocking` 运行阻塞的 `ClipboardWatcherContext::start_watch()` 循环，与 `PlatformRuntime::start_clipboard_watcher()` 中的成熟模式匹配。
- **D-08:** `DaemonService::start()` 生成阻塞 watcher，持有 `WatcherShutdown` channel，取消时调用 `shutdown.stop()` 干净退出。
- **D-09:** Daemon 拥有 `SystemClipboardPort` 并在收到来自远程节点的入站同步内容时直接执行 `write_snapshot()`。

### Claude's Discretion

- `clipboard.new_content` WS 事件的确切负载结构（可遵循现有 realtime 模式）
- 是否保留 `PlatformRuntime` 的简化形式或进一步简化它
- 剪切板 watcher 在 daemon 中失败时的错误处理和重试策略
- 如何抑制 daemon 写入剪切板后的自触发剪切板变更事件（写回循环预防）

### Deferred Ideas (OUT OF SCOPE)

无 — 讨论保持在 phase 范围内
</user_constraints>

## Summary

Phase 57 的目标是将剪切板监听从 GUI (Tauri/PlatformRuntime) 迁移到 daemon 进程，使 daemon 成为唯一与 OS 剪切板交互的进程。

当前架构中，GUI 的 `PlatformRuntime` 通过 `start_clipboard_watcher()` 方法创建 `ClipboardWatcherContext`（来自 `clipboard_rs`），在 `spawn_blocking` 线程中运行，通过 `ClipboardWatcher`（`uc-platform/clipboard/watcher.rs`）捕获变更，然后调用 `AppRuntime`（实现了 `ClipboardChangeHandler`）触发 `CaptureClipboardUseCase`。

Daemon 侧已有占位符 `ClipboardWatcherWorker`（`uc-daemon/workers/clipboard_watcher.rs`），它仅等待取消令牌。需要将其替换为完整实现：直接重用 `ClipboardWatcher`（包含 dedup 和文件时间窗口抑制逻辑），并在 daemon 侧构建等效的 `ClipboardChangeHandler`。

GUI 侧需要：(1) 将 `ClipboardIntegrationMode` 改为 `Passive`，(2) 停用 `WatcherControlPort`-based 启动路径，(3) 通过 `DaemonWsBridge` 接收 daemon 发出的 `clipboard.new_content` WS 事件并转换为现有前端事件契约。

**Primary recommendation:** 直接重用 `ClipboardWatcher`（uc-platform）+ `CaptureClipboardUseCase` 模式，仅在 daemon 侧重新连接这两个现有构件，GUI 侧切换为 Passive 模式。

## Standard Stack

### Core

| Library                               | Version           | Purpose                             | Why Standard                            |
| ------------------------------------- | ----------------- | ----------------------------------- | --------------------------------------- |
| `clipboard_rs`                        | 已在 workspace 中 | OS 剪切板监听                       | 项目现有依赖，GUI 侧已验证              |
| `tokio::task::spawn_blocking`         | tokio 1.x         | 运行阻塞 watcher 循环               | 确定模式（已在 PlatformRuntime 中使用） |
| `tokio::sync::broadcast`              | tokio 1.x         | 向 WebSocket 订阅者广播 daemon 事件 | 所有现有 daemon 服务统一使用此模式      |
| `tokio_util::sync::CancellationToken` | 0.7.x             | 协作式关闭                          | `DaemonService` trait 要求              |

### Supporting

| Library                          | Version   | Purpose                                   | When to Use                |
| -------------------------------- | --------- | ----------------------------------------- | -------------------------- |
| `uc-platform` clipboard 模块     | workspace | 提供 `ClipboardWatcher`、`LocalClipboard` | daemon 侧复用现有平台层    |
| `uc-app` CaptureClipboardUseCase | workspace | 持久化剪切板条目                          | daemon handler 实现        |
| `async_trait`                    | 0.1.x     | 异步 trait 实现                           | `DaemonService` trait 需要 |

**Cargo dependency note:** `uc-daemon` 当前可能不依赖 `uc-platform`（依赖链是 `uc-tauri → uc-platform`）。需要在 `uc-daemon/Cargo.toml` 添加 `uc-platform` 依赖。

## Architecture Patterns

### Recommended Project Structure — 变更文件

```
src-tauri/crates/uc-daemon/
├── src/workers/clipboard_watcher.rs  ← 主要修改：实现真实 ClipboardWatcherWorker
└── Cargo.toml                        ← 添加 uc-platform 依赖

src-tauri/crates/uc-bootstrap/
└── src/builders.rs (或 non_gui_runtime.rs) ← build_daemon_app() 传入 LocalClipboard + handler

src-tauri/src/main.rs                 ← GUI 侧：改为 Passive 模式，处理 clipboard.new_content WS 事件
src-tauri/crates/uc-daemon-client/
└── src/ws_bridge.rs                  ← 扩展：翻译 clipboard.new_content WS 事件为前端事件
```

### Pattern 1: ClipboardWatcherWorker 实现模式

**What:** daemon service 包装 clipboard_rs watcher，通过内置 channel + `ClipboardChangeHandler` callback 驱动业务逻辑

**When to use:** 任何 daemon 侧需要响应 OS 剪切板变更的场景

**Pattern（基于 PlatformRuntime::start_clipboard_watcher 的成熟模式）:**

```rust
// Source: src-tauri/crates/uc-platform/src/runtime/runtime.rs (已验证模式)
pub struct ClipboardWatcherWorker {
    local_clipboard: Arc<dyn SystemClipboardPort>,
    change_handler: Arc<dyn ClipboardChangeHandler>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
}

#[async_trait]
impl DaemonService for ClipboardWatcherWorker {
    fn name(&self) -> &str { "clipboard-watcher" }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        let (platform_tx, mut platform_rx) = tokio::sync::mpsc::channel(64);
        let local_clipboard = self.local_clipboard.clone();
        let handler = self.change_handler.clone();

        // 1. 用 spawn_blocking 运行阻塞 watcher 循环
        let mut watcher_ctx = ClipboardWatcherContext::new()
            .map_err(|e| anyhow::anyhow!("Failed to create watcher context: {}", e))?;
        let watcher = ClipboardWatcher::new(local_clipboard.clone(), platform_tx);
        let shutdown = watcher_ctx.add_handler(watcher).get_shutdown_channel();
        let _join = tokio::task::spawn_blocking(move || {
            info!("clipboard watcher started");
            watcher_ctx.start_watch();
            info!("clipboard watcher stopped");
        });

        // 2. 在 async 任务中处理 PlatformEvent::ClipboardChanged
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    shutdown.stop();
                    break;
                }
                Some(PlatformEvent::ClipboardChanged { snapshot }) = platform_rx.recv() => {
                    if let Err(e) = handler.on_clipboard_changed(snapshot).await {
                        warn!(error = %e, "clipboard change handler failed");
                    }
                }
            }
        }
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> { Ok(()) }
    fn health_check(&self) -> ServiceHealth { ServiceHealth::Healthy }
}
```

### Pattern 2: daemon 侧 ClipboardChangeHandler 实现

**What:** daemon 等效的 `AppRuntime::on_clipboard_changed`，调用 `CaptureClipboardUseCase` 并通过 `event_tx` 广播 `clipboard.new_content` WS 事件。

**关键参考:** `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` 第 558-650 行（`impl ClipboardChangeHandler for AppRuntime`），直接移植此实现。

```rust
// Source: src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs (AppRuntime 实现)
pub struct DaemonClipboardChangeHandler {
    runtime: Arc<CoreRuntime>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
}

#[async_trait]
impl ClipboardChangeHandler for DaemonClipboardChangeHandler {
    async fn on_clipboard_changed(&self, snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
        // 1. 调用 CaptureClipboardUseCase
        let usecases = CoreUseCases::new(&self.runtime);
        // ... 使用与 AppRuntime 相同的 capture 逻辑
        // 2. 成功后通过 event_tx 发送 clipboard.new_content WS 事件
        let _ = self.event_tx.send(DaemonWsEvent {
            topic: "clipboard".to_string(),
            event_type: "clipboard.new_content".to_string(),
            // ...
        });
        Ok(())
    }
}
```

### Pattern 3: GUI 侧 Passive 模式切换

**What:** `main.rs` 不再调用 `start_clipboard_watcher`，GUI 的 `ClipboardIntegrationMode` 改为 `Passive`。

**关键发现:** `ClipboardIntegrationMode` 已经通过 `UC_CLIPBOARD_MODE` 环境变量支持 Passive 模式，但 GUI 默认用 `Full`。需要在 GUI 的 `build_gui_app()` / `AppRuntime::with_setup()` 调用路径中显式传入 `Passive`，而不是读取环境变量。

**注意事项:** 当前 `main.rs` 中创建 `PlatformRuntime` 时传入了 `Some(clipboard_handler)`，并且 `PlatformRuntime::start()` 通过 `StartClipboardWatcher` use case（经过 `WatcherControlPort`）启动监听。需要了解 GUI 何时/如何实际触发 watcher 启动。

**搜索结果确认:** GUI 侧剪切板监听通过以下路径激活：

1. `main.rs` 创建 `PlatformRuntime`，传入 `clipboard_handler: Some(Arc<AppRuntime>)`
2. `PlatformRuntime::start()` 运行事件循环
3. 收到 `PlatformCommand::StartClipboardWatcher` 时调用 `start_clipboard_watcher()`
4. `WatcherControlPort::start_watcher()` 触发该命令（通过 `StartClipboardWatcher` use case）
5. Setup orchestrator 在 setup 完成后调用 `WatcherControlPort`

**迁移策略:** 设置 `ClipboardIntegrationMode::Passive` 使 `StartClipboardWatcher::execute()` 成为 no-op（第 68 行：`if !self.mode.observe_os_clipboard() { return Ok(()); }`）——这是最干净的停用方式，无需删除代码。

### Anti-Patterns to Avoid

- **直接在 ClipboardWatcher 中调用 `handler.on_clipboard_changed()`:** `ClipboardWatcher` 运行在 `spawn_blocking` 线程中，无法直接调用 async handler。必须通过 `mpsc::Sender<PlatformEvent>` 传递到 async 任务再处理（已在 `ClipboardWatcher::new` 签名中确认：接受 `PlatformEventSender`）。
- **在 `DaemonService::start()` 中直接保存 `WatcherShutdown`:** `WatcherShutdown` 不是 `Send`，无法跨 await 持有。解决方案：在 `start()` 内部局部变量持有，只在 `cancel.cancelled()` 分支调用 `shutdown.stop()`。
- **忘记 `uc-daemon/Cargo.toml` 添加 `uc-platform` 依赖:** `ClipboardWatcher` 和 `LocalClipboard` 在 `uc-platform` 中，daemon 当前未依赖此 crate。

## Don't Hand-Roll

| Problem                      | Don't Build           | Use Instead                                            | Why                                                                  |
| ---------------------------- | --------------------- | ------------------------------------------------------ | -------------------------------------------------------------------- |
| 剪切板变更 dedup             | 自定义哈希比较        | `ClipboardWatcher`（uc-platform/clipboard/watcher.rs） | 已包含 hash-based dedup + 文件时间窗口抑制，覆盖 macOS APFS 边缘案例 |
| 阻塞 watcher 在 async 中运行 | 手动线程管理          | `tokio::task::spawn_blocking`                          | 成熟模式，与 PlatformRuntime 一致                                    |
| OS 剪切板读写                | 直接调用 clipboard_rs | `LocalClipboard`（SystemClipboardPort 实现）           | 封装了平台差异和错误处理                                             |
| 剪切板条目持久化             | 直接 DB 写入          | `CaptureClipboardUseCase`                              | 包含表示归一化、spool 队列、blob 写入等复杂逻辑                      |

**Key insight:** `ClipboardWatcher` 中的 dedup 逻辑（包含 `FILE_DEDUP_WINDOW` 500ms 时间窗口）处理了 macOS 文件复制时 APFS 路径解析导致的多次事件触发，手动实现极易遗漏此边缘案例。

## Runtime State Inventory

> 此 phase 不涉及重命名/重构，无需完整清单。但需记录剪切板监听的运行时状态转移。

| Category            | Items Found                                                                                           | Action Required               |
| ------------------- | ----------------------------------------------------------------------------------------------------- | ----------------------------- |
| Stored data         | 剪切板历史存储在 SQLite（uc-infra/db），无运行时进程名                                                | 仅代码修改，无数据迁移        |
| Live service config | GUI 通过 `UC_CLIPBOARD_MODE` 环境变量或代码默认值控制集成模式                                         | 代码修改（设置 Passive 模式） |
| OS-registered state | 无 — 剪切板监听是纯内存/OS API 调用，无持久注册                                                       | None                          |
| Secrets/env vars    | `UC_CLIPBOARD_MODE` — 当前在 GUI 中默认 Full；需确保 daemon 启动时不设置或为 Full，GUI 侧改为 Passive | 代码修改                      |
| Build artifacts     | 无过期 egg-info 或二进制                                                                              | None                          |

## Common Pitfalls

### Pitfall 1: WatcherShutdown 不是 Send

**What goes wrong:** `WatcherShutdown`（来自 clipboard_rs）不实现 `Send`，不能跨 `.await` 持有。
**Why it happens:** clipboard_rs 的 watcher 使用 C 绑定，底层句柄非线程安全。
**How to avoid:** 将 `WatcherShutdown` 持有在 `spawn_blocking` 闭包内，或通过 `oneshot::channel` 传递停止信号。参考 `PlatformRuntime` 中的处理方式：`watcher_handle: Option<WatcherShutdown>` 存储在非 async 上下文。
**Warning signs:** 编译报错 `WatcherShutdown: !Send` 或 "`future is not Send`"。

### Pitfall 2: GUI 侧仍有 PlatformRuntime 创建 LocalClipboard

**What goes wrong:** `PlatformRuntime::new()` 内部调用 `LocalClipboard::new()`，GUI 进程仍拥有剪切板访问对象，但不监听。若 daemon 也创建 LocalClipboard，两个进程均持有 OS 剪切板句柄，可能导致竞争（macOS 下一般无问题，但 Windows 上可能有 GetClipboardOwner 冲突）。
**How to avoid:** D-09 决策正确——daemon 拥有 write 路径。GUI 侧 `PlatformRuntime` 的 `local_clipboard` 用于 `ReadClipboard` 命令和 `WriteClipboard` 命令，若不再需要可清理这些路径。
**Warning signs:** 入站同步时 GUI 和 daemon 都写剪切板。

### Pitfall 3: 写回循环（Write-back Loop）

**What goes wrong:** Daemon 将远程剪切板内容写入 OS 剪切板（D-09）→ OS 触发 `ClipboardWatcherContext` 变更事件 → Daemon 的 `ClipboardWatcher` 捕获此变更 → 再次捕获入站内容，无限循环。
**Why it happens:** 写入 OS 剪切板会触发监听事件，无法区分本进程写入与用户手动复制。
**How to avoid:** 这是"Claude's Discretion"范围内的问题。两种方案：

1. **标记原点（推荐）:** 使用类似 GUI 的 `ClipboardChangeOriginPort` — 写入前标记哈希，`ClipboardWatcher` 的 dedup 逻辑会过滤相同哈希（`last_meaningful_dedupe_key`）。
2. **短暂抑制:** 写入后设置标志，在短时间窗口内忽略首次捕获。
   **Warning signs:** 入站同步条目在历史中出现两次。

### Pitfall 4: uc-daemon 未依赖 uc-platform

**What goes wrong:** `uc-daemon/Cargo.toml` 当前不包含 `uc-platform` 依赖，`ClipboardWatcher` 和 `LocalClipboard` 不可用。
**How to avoid:** 第一个任务就是在 `uc-daemon/Cargo.toml` 添加 `uc-platform = { path = "../uc-platform" }`（并确认无循环依赖：`uc-daemon → uc-platform` 方向安全，因为 `uc-platform` 不依赖 `uc-daemon`）。
**Warning signs:** `cargo check` 报 `use of undeclared crate or module uc_platform`。

### Pitfall 5: DaemonApiEventEmitter 不处理 Clipboard HostEvent

**What goes wrong:** 当前 `DaemonApiEventEmitter::emit()` 对 `HostEvent::Clipboard(_)` 仅调用 `log_non_setup_event("clipboard")`（第 97 行），不发出 WS 事件。
**Why it happens:** Clipboard WS 事件尚未实现。
**How to avoid:** 需要扩展 `DaemonApiEventEmitter` 来处理 `HostEvent::Clipboard(ClipboardHostEvent::NewContent {...})`，或者在 `DaemonClipboardChangeHandler` 中直接向 `event_tx` 发送 `DaemonWsEvent`（绕过 HostEvent 路径）。后者更简单，且不影响其他代码路径。

## Code Examples

### 1. 现有 spawn_blocking 模式（来自 PlatformRuntime）

```rust
// Source: src-tauri/crates/uc-platform/src/runtime/runtime.rs 第 94-117 行
fn start_clipboard_watcher(&mut self) -> Result<()> {
    let mut watcher_ctx = ClipboardWatcherContext::new()
        .map_err(|e| anyhow::anyhow!("Failed to create watcher context: {}", e))?;
    let handler = ClipboardWatcher::new(self.local_clipboard.clone(), self.event_tx.clone());
    let shutdown = watcher_ctx.add_handler(handler).get_shutdown_channel();
    let join = tokio::task::spawn_blocking(move || {
        info!("start clipboard watch");
        watcher_ctx.start_watch();
        info!("clipboard watch stopped");
    });
    self.watcher_join = Some(join);
    self.watcher_handle = Some(shutdown);
    self.watcher_running = true;
    Ok(())
}
```

### 2. DaemonService trait（daemon 服务契约）

```rust
// Source: src-tauri/crates/uc-daemon/src/service.rs
#[async_trait]
pub trait DaemonService: Send + Sync {
    fn name(&self) -> &str;
    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
    fn health_check(&self) -> ServiceHealth;
}
```

### 3. 现有占位符（被替换的文件）

```rust
// Source: src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs
pub struct ClipboardWatcherWorker;  // 无任何字段 — 需要添加依赖

#[async_trait]
impl DaemonService for ClipboardWatcherWorker {
    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        cancel.cancelled().await;  // 仅等待取消 — 需替换为真实实现
        Ok(())
    }
}
```

### 4. DaemonWsEvent 结构（daemon WebSocket 事件契约）

```rust
// Source: src-tauri/crates/uc-daemon/src/api/types.rs
pub struct DaemonWsEvent {
    pub topic: String,         // 例如 "clipboard"
    pub event_type: String,    // 例如 "clipboard.new_content"
    pub session_id: Option<String>,
    pub ts: i64,
    pub payload: Value,        // serde_json::Value
}
```

### 5. GUI 侧 ClipboardIntegrationMode::Passive 使 watcher 成为 no-op

```rust
// Source: src-tauri/crates/uc-platform/src/usecases/start_clipboard_watcher.rs 第 64-71 行
pub async fn execute(&self) -> Result<(), StartClipboardWatcherError> {
    if !self.mode.observe_os_clipboard() {
        info!("Clipboard watcher disabled by integration mode (passive)");
        return Ok(());  // 设置 Passive 后，此处直接返回，不启动 watcher
    }
    self.watcher_control.start_watcher().await?;
    Ok(())
}
```

### 6. main.rs 中 daemon 服务注册模式

```rust
// Source: src-tauri/crates/uc-daemon/src/main.rs 第 94-104 行
let services: Vec<Arc<dyn DaemonService>> = vec![
    Arc::new(ClipboardWatcherWorker) as Arc<dyn DaemonService>,  // 当前占位符
    // ... 其他服务
];
// ClipboardWatcherWorker 已在 services vec 中 — 无需更改注册，只替换实现
```

## State of the Art

| Old Approach                             | Current Approach                  | When Changed | Impact                            |
| ---------------------------------------- | --------------------------------- | ------------ | --------------------------------- |
| GUI 拥有剪切板监听（当前）               | Daemon 拥有剪切板监听（目标）     | Phase 57     | GUI 需切换到 Passive 模式         |
| ClipboardWatcherWorker 占位符            | 真实 ClipboardWatcherContext 实现 | Phase 57     | daemon 实际捕获剪切板             |
| GUI ClipboardChangeHandler（AppRuntime） | Daemon ClipboardChangeHandler     | Phase 57     | capture use case 在 daemon 侧执行 |

## Open Questions

1. **写回循环防止的具体实现方式**
   - What we know: daemon 写入 OS 剪切板（D-09）会触发自身监听事件；`ClipboardWatcher` 有 hash-based dedup，写入相同内容可能被过滤。
   - What's unclear: 入站远程内容是否与本地已有内容相同（可能被 dedup 过滤）？还是不同内容且需要主动标记？
   - Recommendation: 先实现基础版本，利用 `ClipboardWatcher` 内置 dedup；若测试显示循环发生，再添加 `ClipboardChangeOriginPort` 标记。

2. **GUI 侧 `PlatformRuntime` 是否仍需要 LocalClipboard**
   - What we know: GUI 的 `PlatformRuntime` 通过 `PlatformCommand::WriteClipboard` 写剪切板（用于 "restore clipboard selection"）；`PlatformCommand::ReadClipboard` 也通过它读取。
   - What's unclear: 迁移后 GUI 是否仍需这些能力？还是所有剪切板操作都转移到 daemon？
   - Recommendation: 保持 GUI 的 `PlatformRuntime` 对 ReadClipboard/WriteClipboard 命令的处理，仅停用监听功能（设置 Passive 模式）。CONTEXT.md D-01 明确只停用"观察"，写入能力不在此 phase 内讨论。

3. **`clipboard.new_content` WS 事件的前端消费路径**
   - What we know: `DaemonWsBridge`（`uc-daemon-client/src/ws_bridge.rs`）翻译 daemon WS 事件为 `RealtimeEvent`；前端监听 `daemon://realtime`。
   - What's unclear: 前端当前如何处理剪切板新内容事件（`listen_clipboard_new_content` Tauri 命令）？是否已有 `clipboard.new_content` 事件类型？
   - Recommendation: 研究前端 `src/api/` 中的剪切板事件监听代码，确认现有事件契约再决定 WS 事件 payload 结构。

## Environment Availability

| Dependency           | Required By                      | Available | Version                                           | Fallback |
| -------------------- | -------------------------------- | --------- | ------------------------------------------------- | -------- |
| `clipboard_rs` crate | ClipboardWatcherWorker           | ✓         | workspace 中已有                                  | —        |
| `uc-platform` crate  | ClipboardWatcher, LocalClipboard | ✓         | workspace 中已有（需添加到 uc-daemon Cargo.toml） | —        |
| macOS clipboard API  | clipboard_rs                     | ✓         | macOS 25.2.0                                      | —        |
| tokio spawn_blocking | 阻塞 watcher                     | ✓         | tokio 1.x                                         | —        |

**Missing dependencies with no fallback:** 无

**Missing dependencies with fallback:** 无

**注意:** `uc-daemon/Cargo.toml` 需要添加 `uc-platform` 依赖，但该 crate 已存在于 workspace。

## Validation Architecture

### Test Framework

| Property           | Value                                                              |
| ------------------ | ------------------------------------------------------------------ |
| Framework          | cargo test（Rust 单元 + 集成测试）                                 |
| Config file        | src-tauri/Cargo.toml（workspace），各 crate 的 Cargo.toml          |
| Quick run command  | `cd src-tauri && cargo test -p uc-daemon`                          |
| Full suite command | `cd src-tauri && cargo test -p uc-daemon -p uc-platform -p uc-app` |

### Phase Requirements → Test Map

本 phase 无正式 requirement IDs（"TBD"），基于 CONTEXT.md 决策推导：

| Decision | Behavior                                                      | Test Type          | Automated Command                                                                           | File Exists? |
| -------- | ------------------------------------------------------------- | ------------------ | ------------------------------------------------------------------------------------------- | ------------ |
| D-02     | ClipboardWatcherWorker 不再是占位符，start() 实际启动 watcher | unit（结构验证）   | `cd src-tauri && cargo test -p uc-daemon clipboard_watcher`                                 | ❌ Wave 0    |
| D-01     | GUI ClipboardIntegrationMode 为 Passive 时 watcher 不启动     | unit               | `cd src-tauri && cargo test -p uc-platform start_clipboard_watcher_is_noop_in_passive_mode` | ✅ 已有      |
| D-06     | daemon 捕获剪切板后发送 clipboard.new_content WS 事件         | unit               | `cd src-tauri && cargo test -p uc-daemon clipboard_new_content`                             | ❌ Wave 0    |
| D-08     | 取消令牌触发时 watcher 干净退出                               | unit               | `cd src-tauri && cargo test -p uc-daemon clipboard_watcher_cancels_cleanly`                 | ❌ Wave 0    |
| D-09     | daemon 的 ClipboardChangeHandler 调用 CaptureClipboardUseCase | unit（mock ports） | `cd src-tauri && cargo test -p uc-daemon daemon_capture_clipboard`                          | ❌ Wave 0    |

### Sampling Rate

- **Per task commit:** `cd src-tauri && cargo test -p uc-daemon`
- **Per wave merge:** `cd src-tauri && cargo test -p uc-daemon -p uc-platform`
- **Phase gate:** 全套测试通过后 `/gsd:verify-work`

### Wave 0 Gaps

- [ ] `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` 内的单元测试 — 覆盖 D-02、D-08
- [ ] `src-tauri/crates/uc-daemon/tests/clipboard_handler.rs` — 覆盖 D-06、D-09（使用 mock ports，参考 `tests/websocket_api.rs` 的 `build_runtime()` 模式）

_(Note: D-01 的测试已存在于 `uc-platform/src/usecases/start_clipboard_watcher.rs`)_

## Sources

### Primary (HIGH confidence)

- `src-tauri/crates/uc-platform/src/clipboard/watcher.rs` — ClipboardWatcher 完整实现，dedup 逻辑
- `src-tauri/crates/uc-platform/src/runtime/runtime.rs` — start_clipboard_watcher() spawn_blocking 模式
- `src-tauri/crates/uc-daemon/src/workers/clipboard_watcher.rs` — 当前占位符（直接读取）
- `src-tauri/crates/uc-daemon/src/app.rs` — DaemonApp 服务生命周期，JoinSet 模式
- `src-tauri/crates/uc-daemon/src/service.rs` — DaemonService trait 定义
- `src-tauri/crates/uc-daemon/src/main.rs` — 服务注册和 CoreRuntime 构建
- `src-tauri/crates/uc-core/src/clipboard/integration_mode.rs` — ClipboardIntegrationMode 枚举
- `src-tauri/crates/uc-bootstrap/src/non_gui_runtime.rs` — NoopWatcherControl 模式
- `src-tauri/crates/uc-tauri/src/bootstrap/runtime.rs` — AppRuntime ClipboardChangeHandler 实现
- `src-tauri/crates/uc-daemon/src/api/event_emitter.rs` — DaemonApiEventEmitter（当前不处理 Clipboard 事件）
- `src-tauri/crates/uc-platform/src/usecases/start_clipboard_watcher.rs` — Passive 模式 no-op 行为
- `src-tauri/src/main.rs` — GUI 端剪切板监听启动路径

### Secondary (MEDIUM confidence)

- `src-tauri/crates/uc-daemon-client/src/ws_bridge.rs` — DaemonWsBridge 事件翻译模式（用于确认 clipboard 事件扩展点）
- `src-tauri/crates/uc-daemon/tests/websocket_api.rs` — 集成测试模式，`build_runtime()` 辅助函数

## Metadata

**Confidence breakdown:**

- Standard stack: HIGH — 直接读取现有代码，所有依赖已在 workspace 中
- Architecture: HIGH — 成熟的 `spawn_blocking` + `DaemonService` 模式，有多个参考实现
- Pitfalls: HIGH — WatcherShutdown !Send 和写回循环来自直接代码分析；uc-platform 依赖缺失通过检查 Cargo.toml 确认
- 写回循环防止方案: MEDIUM — 需要运行时验证，但已有缓解路径

**Research date:** 2026-03-25
**Valid until:** 2026-04-25（clipboard_rs API 稳定，内部架构稳定）
