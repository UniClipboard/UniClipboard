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
use uc_bootstrap::{init_tracing_subscriber, BackgroundRuntimeDeps};
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
///
/// GUI 进程的 daemon sidecar 拉起、pairing 推进、托盘等 Tauri/AppKit
/// 特定的事情不在这里——交给各自的 shell crate（`uc-tauri::run` 等）。
pub fn build_gui_app() -> anyhow::Result<GuiBootstrapContext> {
    // Idempotent — safe to call multiple times.
    init_tracing_subscriber()?;
    // Mirror panic events into jsonl(target = "panic"). Must be installed
    // after tracing init so the subscriber is in place when a panic fires.
    install_panic_logging_hook();

    let config = AppConfig::empty();
    let wired = wire_dependencies(&config)
        .map_err(|e| anyhow::anyhow!("Dependency wiring failed: {}", e))?;
    let storage_paths = get_storage_paths(&config)?;

    Ok(GuiBootstrapContext {
        deps: wired.deps,
        background: wired.background,
        storage_paths,
        config,
    })
}
