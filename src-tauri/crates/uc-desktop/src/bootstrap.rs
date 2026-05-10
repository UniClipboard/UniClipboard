//! 进程级运行时装配（GUI shell 与 standalone daemon binary 共用）。
//!
//! 提供"启动期一次性"的进程级上下文构造——拼出 [`ProcessRuntimeContext`]，
//! 让 caller 喂给自己的运行时（GUI shell 的 `TauriAppRuntime` / standalone
//! binary 的 `DesktopRuntime`）。装配本身由 [`uc_bootstrap`] 提供的
//! composition root 工具完成（tracing init、panic hook、`wire_dependencies`、
//! `get_storage_paths`）。
//!
//! 这条装配链涵盖**进程级一次性资源**：sqlite pool / 所有 repos /
//! settings / secure storage / blob store / clipboard write coordinator /
//! mobile_sync_endpoint_info adapter / spool & blob worker receivers。它们
//! 在进程启动时建一次，daemon reload 不重建（daemon-lifecycle 装配在
//! [`crate::daemon::bootstrap`]）。
//!
//! `uc-bootstrap` 不再持有任何"shell 专属"的 entry-point builder——它退化
//! 成纯装配工具集，daemon / CLI / GUI 各自的 entry-point 装配在各自的
//! crate 里完成。

use uc_application::facade::AppPaths;
use uc_bootstrap::assembly::{get_storage_paths, wire_dependencies, WiredDependencies};
use uc_bootstrap::tracing::install_panic_logging_hook;
use uc_bootstrap::{init_tracing_subscriber, BackgroundRuntimeDeps};
use uc_core::config::AppConfig;

/// 进程级运行时装配的全部输出。Caller 从中:
///
/// - clone `wired.deps` 装配自己的 runtime(`TauriAppRuntime` /
///   `DesktopRuntime`),并把同一份 wired 透传给 in-process daemon spawn,
///   让 daemon-lifecycle 装配复用同一份 sqlite pool / repos / 各种 adapter。
/// - 用 `background` + `BlobProcessingPorts::from_app_deps(&wired.deps)`
///   spawn 一次性 spool/blob worker(挂在 runtime.task_registry 上)。
/// - 从 `storage_paths` / `config` 读启动期的配置与目录布局。
///
/// **不再持有** 单独的 `mobile_sync_endpoint_info` 字段 —— 它已经在
/// `wired.mobile_sync_endpoint_info`(同时也在 `wired.deps.mobile_sync.endpoint_info`),
/// 单一来源。
pub struct ProcessRuntimeContext {
    pub wired: WiredDependencies,
    pub background: BackgroundRuntimeDeps,
    pub storage_paths: AppPaths,
    pub config: AppConfig,
}

/// 构造进程级运行时上下文。GUI shell 与 standalone daemon binary 都用。
///
/// 步骤：
/// 1. tracing subscriber 初始化（idempotent）
/// 2. panic logging hook 安装（idempotent）
/// 3. 读取并解析 `AppConfig`
/// 4. 通过 [`wire_dependencies`] 组装 `WiredDependencies` /
///    `BackgroundRuntimeDeps`
/// 5. 解析 `AppPaths`
///
/// daemon-lifecycle 资源（iroh node / space_setup / HTTP server / LAN
/// listener / PID 文件）**不在这里**装——它们走
/// [`crate::daemon::bootstrap::build_daemon_bootstrap_assembly`]，每次
/// daemon start/stop 重建。
///
/// GUI 进程的托盘、pairing 推进、quick panel 等 Tauri/AppKit 特定的事情
/// 也不在这里——交给各自的 shell crate（`uc-tauri::run` 等）。
pub fn build_process_runtime() -> anyhow::Result<ProcessRuntimeContext> {
    // Idempotent — safe to call multiple times.
    init_tracing_subscriber()?;
    // Mirror panic events into jsonl(target = "panic"). Must be installed
    // after tracing init so the subscriber is in place when a panic fires.
    install_panic_logging_hook();

    let config = AppConfig::empty();

    let (wired, background) = wire_dependencies(&config)
        .map_err(|e| anyhow::anyhow!("Dependency wiring failed: {}", e))?;
    let storage_paths = get_storage_paths(&config)?;

    Ok(ProcessRuntimeContext {
        wired,
        background,
        storage_paths,
        config,
    })
}
