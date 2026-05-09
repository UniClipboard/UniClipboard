//! Desktop GUI shell 入口装配。
//!
//! 提供 GUI shell（`uc-tauri`、未来 `uc-macos-native`）共享的"启动期上下文
//! 构造"——拼出 [`GuiBootstrapContext`]，让 shell 把它喂给自己的窗口/事件
//! 循环。装配本身由 [`uc_bootstrap`] 提供的 composition root 工具完成
//! （tracing init、panic hook、`wire_dependencies`、`get_storage_paths`）。
//!
//! `uc-bootstrap` 不再持有任何"GUI shell 专属"的 entry-point builder——
//! 它退化成纯装配工具集，daemon / CLI 自己的 entry-point 装配也在各自
//! 的 crate 里完成。

use uc_application::deps::AppDeps;
use uc_application::facade::AppPaths;
use uc_bootstrap::assembly::{get_storage_paths, wire_dependencies};
use uc_bootstrap::tracing::install_panic_logging_hook;
use uc_bootstrap::{compose_event_context, init_tracing_subscriber, BackgroundRuntimeDeps};
use uc_core::config::AppConfig;

/// 桌面 GUI shell 启动需要的全部上下文。Shell 从中取 `deps` 装配自己的
/// runtime（如 `TauriAppRuntime`），从 `background` 启动后台任务，从
/// `storage_paths` / `config` 读启动期的配置与目录布局。
pub struct GuiBootstrapContext {
    pub deps: AppDeps,
    pub background: BackgroundRuntimeDeps,
    pub storage_paths: AppPaths,
    pub config: AppConfig,
}

/// 构造 GUI shell 的启动上下文。
///
/// 步骤：
/// 1. tracing subscriber 初始化（idempotent）
/// 2. panic logging hook 安装（idempotent）
/// 3. 读取并解析 `AppConfig`
/// 4. 通过 [`wire_dependencies`] 组装 `AppDeps` / `BackgroundRuntimeDeps`
/// 5. 解析 `AppPaths`
/// 6. 装配并注册进程级 product analytics `EventContext`
///
/// GUI 进程的 daemon sidecar 拉起、pairing 推进、托盘等 Tauri/AppKit
/// 特定的事情不在这里——交给各自的 shell crate（`uc-tauri::run` 等）。
///
/// ## Async
///
/// Slice 6 / Issue #549 起本函数转 async：注册 `EventContext` 需要读
/// `member_repo` / `setup_status` 这两个 async port。GUI shell 在 sync
/// 入口（如 `uc-tauri::run`）调用时，用 `tauri::async_runtime::block_on`
/// 桥接即可——所有现有 GUI shell 已经在 Tauri runtime 内运行，无新开销。
pub async fn build_gui_app() -> anyhow::Result<GuiBootstrapContext> {
    // Idempotent — safe to call multiple times.
    init_tracing_subscriber()?;
    // Mirror panic events into jsonl(target = "panic"). Must be installed
    // after tracing init so the subscriber is in place when a panic fires.
    install_panic_logging_hook();

    let config = AppConfig::empty();
    let wired = wire_dependencies(&config)
        .map_err(|e| anyhow::anyhow!("Dependency wiring failed: {}", e))?;
    let storage_paths = get_storage_paths(&config)?;

    // 注册进程级 product analytics `EventContext`。失败不阻断启动 —— 错误
    // 已在 `compose_event_context` 内 warn-log（见 `uc-bootstrap::analytics`
    // 模块文档"失败语义"）。这里再 warn 一行让 GUI 启动日志可追溯。
    if let Err(err) = compose_event_context(&wired.deps, &storage_paths).await {
        tracing::warn!(
            error = %err,
            "analytics: GUI 启动期 compose_event_context 失败，本次进程内事件 sink 将拿不到 EventContext 快照"
        );
    }

    Ok(GuiBootstrapContext {
        deps: wired.deps,
        background: wired.background,
        storage_paths,
        config,
    })
}
