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

use std::sync::Arc;

use uc_application::deps::AppDeps;
use uc_application::facade::AppPaths;
use uc_bootstrap::assembly::{get_storage_paths, wire_dependencies_with_overrides};
use uc_bootstrap::tracing::install_panic_logging_hook;
use uc_bootstrap::{init_tracing_subscriber, BackgroundRuntimeDeps, WireOverrides};
use uc_core::config::AppConfig;
use uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter;

/// 进程级运行时装配的全部输出。Caller 从中取 `deps` 装配自己的 runtime
/// （`TauriAppRuntime` / `DesktopRuntime`），从 `background` 启动后台任务，
/// 从 `storage_paths` / `config` 读启动期的配置与目录布局。
pub struct ProcessRuntimeContext {
    pub deps: AppDeps,
    pub background: BackgroundRuntimeDeps,
    pub storage_paths: AppPaths,
    pub config: AppConfig,
    /// 进程级共享的 mobile_sync endpoint_info Arc。GUI shell 把它保留下来,
    /// 在 daemon spawn(`bootstrap_daemon_in_process` / `reload_in_process_daemon`)
    /// 时通过 `WireOverrides` 注入,让 daemon 端 deps 与 GUI 端 deps 共享同一份 ——
    /// daemon LAN listener 写入的 status 因此能被 GUI in-process facade 读到。
    pub mobile_sync_endpoint_info: Arc<InMemoryMobileSyncEndpointInfoAdapter>,
}

/// 构造进程级运行时上下文。GUI shell 与 standalone daemon binary 都用。
///
/// 步骤：
/// 1. tracing subscriber 初始化（idempotent）
/// 2. panic logging hook 安装（idempotent）
/// 3. 读取并解析 `AppConfig`
/// 4. 通过 [`wire_dependencies`] 组装 `AppDeps` / `BackgroundRuntimeDeps`
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

    // 进程级共享的 endpoint_info —— GUI deps 与 in-process daemon deps 共用。
    // wire_dependencies 内部如果不接收 override,会自己 new 一份,导致 daemon
    // 写的 status 被 GUI facade 读不到(参见 `WireOverrides` 的文档)。
    let mobile_sync_endpoint_info = Arc::new(InMemoryMobileSyncEndpointInfoAdapter::new());
    let wire_overrides = WireOverrides {
        mobile_sync_endpoint_info: Some(Arc::clone(&mobile_sync_endpoint_info)),
    };

    let wired = wire_dependencies_with_overrides(&config, wire_overrides)
        .map_err(|e| anyhow::anyhow!("Dependency wiring failed: {}", e))?;
    let storage_paths = get_storage_paths(&config)?;

    Ok(ProcessRuntimeContext {
        deps: wired.deps,
        background: wired.background,
        storage_paths,
        config,
        mobile_sync_endpoint_info,
    })
}
