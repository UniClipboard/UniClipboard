//! # Non-GUI Runtime Helpers
//!
//! Provides [`LoggingHostEventEmitter`] and [`build_non_gui_bundle()`] for
//! non-GUI entry points (daemon, CLI). D16-2 retired the legacy `CoreRuntime`
//! wrapper; helpers here now return a flat [`NonGuiBundle`] that the caller
//! destructures into independent locals.
//!
//! [`LoggingHostEventEmitter`] logs event type names via `tracing::debug!`
//! without printing inner payloads (which may contain sensitive data like
//! clipboard content, pairing codes, or file paths).

use std::sync::Arc;

use uc_application::deps::AppDeps;
use uc_application::facade::space_setup::SpaceSetupFacade;
use uc_application::facade::{
    AppFacade, AppFacadeParts, AppPaths, BlobTransferFacade, ClipboardHistoryFacade,
    ClipboardHistoryFacadeDeps, ClipboardRestoreFacade, ClipboardRestoreFacadeDeps,
    ClipboardSyncFacade, DeviceFacade, EmitError, EncryptionFacade, EncryptionFacadeDeps,
    HostEvent, HostEventEmitterPort, InMemoryLifecycleStatus, LifecycleFacade, LifecycleFacadeDeps,
    LifecycleStatusGateway, MemberRosterFacade, ResourceFacade, ResourceFacadeDeps,
    SearchCoordinator, SearchCoordinatorDeps, SearchFacade, SearchFacadeDeps, SettingsFacade,
    StorageFacade, StorageFacadeDeps, UpgradeFacade, UpgradeFacadeDeps,
};
use uc_core::clipboard::ClipboardIntegrationMode;

use crate::assembly::get_storage_paths;
use crate::space_setup::{build_space_setup_assembly, IrohNodeConfig, SpaceSetupAssembly};
use crate::task_registry::TaskRegistry;

// ---------------------------------------------------------------------------
// LoggingHostEventEmitter
// ---------------------------------------------------------------------------

/// Event emitter that logs event type names via `tracing::debug!`.
///
/// Always returns `Ok(())` — infallible by design. Inner event payloads are
/// NOT logged because they may contain sensitive data (clipboard content,
/// pairing codes/fingerprints, transfer file paths).
pub struct LoggingHostEventEmitter;

impl HostEventEmitterPort for LoggingHostEventEmitter {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        match event {
            HostEvent::Clipboard(_) => {
                tracing::debug!(event_type = "clipboard", "host event (non-gui)");
            }
            HostEvent::Transfer(_) => {
                tracing::debug!(event_type = "transfer", "host event (non-gui)");
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// NonGuiBundle
// ---------------------------------------------------------------------------

/// Flat bundle of bootstrap-built handles consumed by daemon entry points.
///
/// Replaces the retired `CoreRuntime` wrapper. Composition-root code
/// destructures the bundle into independent locals (`deps`, `task_registry`,
/// `lifecycle_status`, etc.) and feeds them into facade construction.
pub struct NonGuiBundle {
    pub deps: AppDeps,
    pub storage_paths: AppPaths,
    pub emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
    pub lifecycle_status: Arc<dyn LifecycleStatusGateway>,
    pub task_registry: Arc<TaskRegistry>,
    pub clipboard_integration_mode: ClipboardIntegrationMode,
}

/// Construct a [`NonGuiBundle`] for non-GUI entry points with an explicit
/// shared emitter cell. Daemon uses this so its `DaemonApiEventEmitter`
/// can be swapped in after construction.
pub fn build_non_gui_bundle(
    deps: AppDeps,
    storage_paths: AppPaths,
    emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
) -> anyhow::Result<NonGuiBundle> {
    let lifecycle_status: Arc<dyn LifecycleStatusGateway> =
        Arc::new(InMemoryLifecycleStatus::new());
    let task_registry = Arc::new(TaskRegistry::new());
    let clipboard_integration_mode = resolve_clipboard_integration_mode();

    Ok(NonGuiBundle {
        deps,
        storage_paths,
        emitter_cell,
        lifecycle_status,
        task_registry,
        clipboard_integration_mode,
    })
}

/// `ClipboardRestoreFacade` 的可选装配输入。
///
/// GUI 和 daemon 需要 restore 能力；部分 CLI 查询入口不需要，因此通过
/// 显式选项传入，避免各入口各自复制 facade 拼装代码。
pub struct ClipboardRestoreAssembly {
    pub write_coordinator: Arc<uc_application::clipboard_write::ClipboardWriteCoordinator>,
    pub integration_mode: ClipboardIntegrationMode,
}

/// 通用 `AppFacade` 装配选项。
///
/// 不同桌面入口只在这些可选能力上有差异。共同 facade 由
/// [`build_app_facade_from_deps`] 统一创建，避免 daemon、Tauri、CLI 各自
/// 手写一份相同的子 facade 拼装。
#[derive(Default)]
pub struct AppFacadeAssemblyOptions {
    pub space_setup: Option<Arc<SpaceSetupFacade>>,
    pub member_roster: Option<Arc<MemberRosterFacade>>,
    pub clipboard_sync: Option<Arc<ClipboardSyncFacade>>,
    pub blob_transfer: Option<Arc<BlobTransferFacade>>,
    pub clipboard_restore: Option<ClipboardRestoreAssembly>,
    pub search_coordinator: Option<Arc<SearchCoordinator>>,
}

/// 从已注入的 application deps 构造统一业务入口。
///
/// 这是 GUI、daemon、CLI 共享的 application facade 装配点。调用方仍然
/// 决定运行模式、事件源、HTTP/WS/Tauri 接入和后台任务；本函数只负责把
/// ports 组合成 `AppFacade`。
pub fn build_app_facade_from_deps(
    deps: &AppDeps,
    storage_paths: &AppPaths,
    lifecycle_status: Arc<dyn LifecycleStatusGateway>,
    options: AppFacadeAssemblyOptions,
) -> Arc<AppFacade> {
    let clipboard_restore = options.clipboard_restore.map(|restore| {
        Arc::new(ClipboardRestoreFacade::new(ClipboardRestoreFacadeDeps {
            entry_repo: deps.clipboard.clipboard_entry_repo.clone(),
            selection_repo: deps.clipboard.selection_repo.clone(),
            representation_repo: deps.clipboard.representation_repo.clone(),
            blob_store: deps.storage.blob_store.clone(),
            clock: deps.system.clock.clone(),
            write_coordinator: restore.write_coordinator,
            integration_mode: restore.integration_mode,
        }))
    });

    Arc::new(AppFacade::new(AppFacadeParts {
        space_setup: options.space_setup,
        member_roster: options.member_roster,
        lifecycle: Arc::new(LifecycleFacade::new(LifecycleFacadeDeps {
            status: lifecycle_status,
        })),
        encryption: Arc::new(EncryptionFacade::new(EncryptionFacadeDeps {
            setup_status: deps.setup_status.clone(),
            space_access: deps.security.space_access.clone(),
        })),
        resource: Arc::new(ResourceFacade::new(ResourceFacadeDeps {
            representation_repo: deps.clipboard.representation_repo.clone(),
            thumbnail_repo: deps.storage.thumbnail_repo.clone(),
            blob_store: deps.storage.blob_store.clone(),
        })),
        clipboard_history: Arc::new(ClipboardHistoryFacade::new(ClipboardHistoryFacadeDeps {
            entry_repo: deps.clipboard.clipboard_entry_repo.clone(),
            selection_repo: deps.clipboard.selection_repo.clone(),
            representation_repo: deps.clipboard.representation_repo.clone(),
            event_writer: deps.clipboard.clipboard_event_repo.clone(),
            payload_resolver: deps.clipboard.payload_resolver.clone(),
            blob_store: deps.storage.blob_store.clone(),
            thumbnail_repo: deps.storage.thumbnail_repo.clone(),
            file_transfer_repo: deps.storage.file_transfer_repo.clone(),
            search_index: Some(deps.search.search_index.clone()),
            file_cache_dir: Some(storage_paths.cache_dir.clone()),
            device_identity: deps.device.device_identity.clone(),
            clock: deps.system.clock.clone(),
        })),
        clipboard_sync: options.clipboard_sync,
        blob_transfer: options.blob_transfer,
        clipboard_restore,
        search: Arc::new(SearchFacade::new(SearchFacadeDeps {
            search_index: deps.search.search_index.clone(),
            coordinator: options.search_coordinator,
        })),
        settings: Arc::new(SettingsFacade::new(deps.settings.clone())),
        device: Arc::new(DeviceFacade::new(
            deps.device.device_identity.clone(),
            deps.settings.clone(),
        )),
        storage: Arc::new(StorageFacade::new(StorageFacadeDeps {
            db_path: storage_paths.db_path.clone(),
            vault_dir: storage_paths.vault_dir.clone(),
            cache_dir: storage_paths.cache_dir.clone(),
            logs_dir: storage_paths.logs_dir.clone(),
            app_data_root_dir: storage_paths.app_data_root_dir.clone(),
            cache_fs: deps.system.cache_fs.clone(),
        })),
        upgrade: Arc::new(UpgradeFacade::new(UpgradeFacadeDeps {
            app_version_state: deps.app_version_state.clone(),
            setup_status: deps.setup_status.clone(),
        })),
    }))
}

/// Construct an [`AppFacade`] for CLI entry points.
///
/// CLI commands need a stable application-layer
/// entry point per `uc-application/AGENTS.md` §11.4. This helper assembles
/// the facade subset CLI cares about (encryption / settings / device /
/// clipboard_history / search / lifecycle / storage / resource) and leaves
/// the daemon-only fields (`space_setup`, `member_roster`, `clipboard_restore`)
/// as `None`.
///
/// # Arguments
///
/// * `log_profile` — Log profile override (e.g., `Some(LogProfile::Cli)`).
pub fn build_cli_app_facade(
    log_profile: Option<uc_observability::LogProfile>,
) -> anyhow::Result<Arc<AppFacade>> {
    let ctx = crate::builders::build_cli_context_with_profile(log_profile)?;
    let storage_paths = crate::assembly::get_storage_paths(&ctx.config)?;
    let deps = ctx.deps;
    let lifecycle_status: Arc<dyn LifecycleStatusGateway> =
        Arc::new(InMemoryLifecycleStatus::new());

    let search_coordinator = Arc::new(SearchCoordinator::new(SearchCoordinatorDeps::new(
        deps.search.search_index.clone(),
        deps.search.search_key_derivation.clone(),
        deps.search.search_pipeline.clone(),
        deps.clipboard.clipboard_entry_repo.clone(),
        deps.clipboard.representation_repo.clone(),
        deps.clipboard.selection_repo.clone(),
    )));

    Ok(build_app_facade_from_deps(
        &deps,
        &storage_paths,
        lifecycle_status,
        AppFacadeAssemblyOptions {
            search_coordinator: Some(search_coordinator),
            ..Default::default()
        },
    ))
}

/// CLI 进程内 application runtime。
///
/// 业务命令只通过 `app_facade` 进入 application 层。需要 iroh 网络栈的
/// 命令持有 `space_setup_assembly`,退出前调用 [`Self::shutdown`] 收口。
pub struct CliAppRuntime {
    pub app_facade: Arc<AppFacade>,
    space_setup_assembly: Option<SpaceSetupAssembly>,
}

impl CliAppRuntime {
    pub fn app_facade(&self) -> &Arc<AppFacade> {
        &self.app_facade
    }

    pub async fn shutdown(mut self) {
        if let Some(assembly) = self.space_setup_assembly.take() {
            assembly.shutdown().await;
        }
    }
}

/// 构造完整 CLI runtime。适用于 pairing / roster / send / watch / blob 等
/// 需要 iroh 网络栈的独立 CLI 命令。
pub async fn build_cli_app_runtime(
    log_profile: Option<uc_observability::LogProfile>,
) -> anyhow::Result<CliAppRuntime> {
    let (config, wired) = crate::builders::build_slice1_cli_context(log_profile)?;
    let storage_paths = get_storage_paths(&config)?;
    let assembly = build_space_setup_assembly(&wired, IrohNodeConfig::default())
        .await
        .map_err(|err| anyhow::anyhow!("failed to bind iroh endpoint: {err}"))?;
    let deps = &wired.deps;

    let lifecycle_status: Arc<dyn LifecycleStatusGateway> =
        Arc::new(InMemoryLifecycleStatus::new());
    let search_coordinator = Arc::new(SearchCoordinator::new(SearchCoordinatorDeps::new(
        deps.search.search_index.clone(),
        deps.search.search_key_derivation.clone(),
        deps.search.search_pipeline.clone(),
        deps.clipboard.clipboard_entry_repo.clone(),
        deps.clipboard.representation_repo.clone(),
        deps.clipboard.selection_repo.clone(),
    )));

    let app_facade = build_app_facade_from_deps(
        deps,
        &storage_paths,
        lifecycle_status,
        AppFacadeAssemblyOptions {
            space_setup: Some(assembly.facade.clone()),
            member_roster: Some(assembly.roster.clone()),
            clipboard_sync: Some(assembly.clipboard_sync.clone()),
            blob_transfer: Some(assembly.blob.clone()),
            search_coordinator: Some(search_coordinator),
            ..Default::default()
        },
    );

    Ok(CliAppRuntime {
        app_facade,
        space_setup_assembly: Some(assembly),
    })
}

/// Parse a raw string into a [`ClipboardIntegrationMode`].
///
/// Returns `Full` when `raw` is `None`, empty, or an unrecognized value.
/// Returns `Passive` only when the value is `"passive"` (case-insensitive).
pub fn parse_clipboard_integration_mode(raw: Option<&str>) -> ClipboardIntegrationMode {
    let Some(raw_value) = raw else {
        return ClipboardIntegrationMode::Full;
    };

    let normalized = raw_value.trim();
    if normalized.eq_ignore_ascii_case("passive") {
        return ClipboardIntegrationMode::Passive;
    }
    if normalized.eq_ignore_ascii_case("full") {
        return ClipboardIntegrationMode::Full;
    }

    tracing::warn!(
        uc_clipboard_mode = %raw_value,
        "Invalid UC_CLIPBOARD_MODE value; falling back to full integration"
    );
    ClipboardIntegrationMode::Full
}

/// Resolve the clipboard integration mode from the `UC_CLIPBOARD_MODE` env var.
///
/// Defaults to [`ClipboardIntegrationMode::Full`] when the variable is unset.
/// Used by both GUI and non-GUI runtimes to determine clipboard behavior.
pub fn resolve_clipboard_integration_mode() -> ClipboardIntegrationMode {
    let raw = std::env::var("UC_CLIPBOARD_MODE").ok();
    parse_clipboard_integration_mode(raw.as_deref())
}
