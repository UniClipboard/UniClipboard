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

use async_trait::async_trait;
use uc_application::clipboard_capture::CaptureClipboardUseCase;
use uc_application::deps::AppDeps;
use uc_application::facade::space_setup::SpaceSetupFacade;
use uc_application::facade::{
    AppFacade, AppFacadeParts, AppPaths, BlobTransferFacade, ClipboardHistoryFacade,
    ClipboardHistoryFacadeDeps, ClipboardOutboundFacade, ClipboardRestoreFacade,
    ClipboardRestoreFacadeDeps, ClipboardSyncFacade, DeviceFacade, EmitError, EncryptionFacade,
    EncryptionFacadeDeps, FileTransferFacade, HostEvent, HostEventEmitterPort,
    InMemoryLifecycleStatus, IncomingMobileBuffer, LifecycleFacade, LifecycleFacadeDeps,
    LifecycleStatusGateway, MemberRosterFacade, MobileSyncFacade, MobileSyncFacadeDeps,
    MobileSyncSnapshotPorts, ResourceFacade, ResourceFacadeDeps, SearchCoordinator,
    SearchCoordinatorDeps, SearchFacade, SearchFacadeDeps, SettingsFacade, StorageFacade,
    StorageFacadeDeps, UpgradeFacade, UpgradeFacadeDeps,
};
use uc_application::{
    ApplyInboundClipboardUseCase, InboundCapture as ApplyInboundCapture,
    InboundWrite as ApplyInboundWrite,
};
use uc_core::clipboard::ClipboardIntegrationMode;
use uc_core::SystemClipboardSnapshot;
use uc_infra::mobile_sync::{
    Argon2idPasswordHasher, FilesystemMobileFileStaging, NetworkInterfaceLanProbe,
    OsRngCredentialsMinter,
};

use crate::assembly::get_storage_paths;
use crate::space_setup::{build_space_setup_assembly, SpaceSetupAssembly};
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

// ── mobile_sync PUT 路径的 fallback adapters ────────────────────────────

/// 当 [`AppFacadeAssemblyOptions::mobile_sync_apply_inbound`] 为 `None` 时
/// (CLI / tauri 等不接 LAN listener 的入口),用此构造一份 lite
/// `ApplyInboundClipboardUseCase`:
///
/// - **capture**:真 `CaptureClipboardUseCase`。写 entry_repo + event_repo +
///   spool_queue,与 daemon 装配的 capture 完全等价。让 P5a.9 引入的
///   `uniclip mobile-sync debug put-*` 子命令真能把数据落库,后续
///   `debug get-doc` / `debug get-file` 直接读得到 ——"完整链路" 验证不再
///   是空壳。
/// - **write**:`NoopInboundWrite`。CLI 进程主动设置
///   `UC_DISABLE_SYSTEM_CLIPBOARD=1`,本就不接系统剪贴板适配器;OS write
///   永远是 daemon 的责任。NoOp 在这里返回 `Ok(())`,让 ApplyInbound 的
///   写回环防御链不报错。
/// - 不挂 `with_blob_materializer`/`with_host_event_emitter`:debug 路径
///   不走 P2P / 不发 host event,跳过两组可选装配减少耦合。
///
/// daemon 入口仍走自己的 enhanced 装配(`runtime_assembly.rs`),不受影响。
fn build_fallback_apply_inbound(deps: &AppDeps) -> Arc<ApplyInboundClipboardUseCase> {
    let capture_uc = Arc::new(CaptureClipboardUseCase::new(
        deps.clipboard.clipboard_entry_repo.clone(),
        deps.clipboard.clipboard_event_repo.clone(),
        deps.clipboard.representation_policy.clone(),
        deps.clipboard.representation_normalizer.clone(),
        deps.device.device_identity.clone(),
        deps.clipboard.representation_cache.clone(),
        deps.clipboard.spool_queue.clone(),
        deps.analytics.clone(),
    ));
    let capture: Arc<dyn ApplyInboundCapture> = capture_uc;
    let write: Arc<dyn ApplyInboundWrite> = Arc::new(NoopInboundWrite);
    Arc::new(ApplyInboundClipboardUseCase::new(
        deps.clipboard.clipboard_entry_repo.clone(),
        capture,
        write,
    ))
}

/// `InboundWrite` 的 NoOp 实装。
///
/// CLI 与 tauri 入口都不持有系统剪贴板适配器(CLI 显式 disable,tauri 不接
/// 这条 PUT 路径),OS write 不能也不应该在这里发生 —— 直接返回 `Ok(())`,
/// 让 ApplyInbound 链路在 daemon 之外仍可正常推进 capture + dedup。
struct NoopInboundWrite;

#[async_trait]
impl ApplyInboundWrite for NoopInboundWrite {
    async fn write(&self, _snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
        Ok(())
    }
}

/// 构造 [`MobileSyncFacade`] —— 抽出来供 daemon-lifecycle 装配复用。
///
/// `apply_inbound` 由调用方决定:GUI/CLI 走 fallback (NoopWrite),daemon
/// 走 enhanced (with_blob_materializer + with_host_event_emitter)。`endpoint_info`
/// 由 [`AppDeps`] 携带 (单例,daemon LAN listener 与 facade 共享同一份
/// Arc),无需 caller 透传。`file_transfer` 进程级 facade:daemon 装配
/// 必传,SyncDoc apply 后 link + complete 让 mobile_lan transfer 在
/// file_transfer 表里闭环;CLI / 不接 LAN listener 的入口可留 `None`。
pub fn build_mobile_sync_facade(
    deps: &AppDeps,
    storage_paths: &AppPaths,
    apply_inbound: Arc<ApplyInboundClipboardUseCase>,
    file_transfer: Option<Arc<FileTransferFacade>>,
    // GUI daemon 装配传 `Some(controller)` —— update_settings 写盘后即时
    // start/stop/rebind listener。CLI fallback 传 `None`,settings 只写盘,
    // 等下次 daemon 进程启动一次性读取(与本字段引入前完全一致的行为)。
    lan_lifecycle: Option<Arc<dyn uc_core::ports::MobileLanLifecyclePort>>,
    // 同进程内已构造好的 `ClipboardOutboundFacade`(daemon 启动时装配)。
    // 装入时,移动端 PUT 落地本机后会异步把同一份 snapshot 走"本机捕获
    // → 出站"完整管线 fan-out 给 Space 内其他已配对设备 ——
    //
    // - 文本 / 小图 inline 进 V3 envelope;
    // - 大图自动剥成 iroh-blobs ref;
    // - **文件**:`publish_blob_path` 流式发布到 iroh-blobs, 构造 free-file
    //   V3BlobRef, 接收端拉回并改写 file-list rep 成本机 URI ——
    //   "手机文件 → 其他桌面"的真正传输靠这条路径成立。
    //
    // CLI fallback / 不接 P2P 出站的入口传 `None`, mobile 上传仅落地本机,
    // 不传播。
    clipboard_outbound: Option<Arc<ClipboardOutboundFacade>>,
) -> Arc<MobileSyncFacade> {
    Arc::new(MobileSyncFacade::new(MobileSyncFacadeDeps {
        clock: deps.system.clock.clone(),
        // v3 SyncClipboard 兼容: 单一 minter 一次性出 (username, password,
        // password_hash, device_id), Argon2id 作为口令 hash;无状态 ZST,
        // 装配处直接 new 即可。
        credentials_minter: Arc::new(OsRngCredentialsMinter),
        password_hasher: Arc::new(Argon2idPasswordHasher),
        device_repo: deps.mobile_sync.device_repo.clone(),
        endpoint_info: deps.mobile_sync.endpoint_info.clone(),
        lan_interface_probe: Arc::new(NetworkInterfaceLanProbe::new()),
        settings: deps.settings.clone(),
        apply_inbound,
        incoming_buffer: Arc::new(IncomingMobileBuffer::new()),
        file_staging: FilesystemMobileFileStaging::new(storage_paths.file_cache_dir.clone()),
        snapshot_ports: MobileSyncSnapshotPorts {
            entry_repo: deps.clipboard.clipboard_entry_repo.clone(),
            selection_repo: deps.clipboard.selection_repo.clone(),
            representation_repo: deps.clipboard.representation_repo.clone(),
            payload_resolver: deps.clipboard.payload_resolver.clone(),
            blob_reader: deps.storage.blob_store.clone(),
        },
        file_transfer,
        clipboard_outbound,
        lan_lifecycle,
        // schema doc §7.6 / §12.2 P1：mobile_sync 域共用 process-wide analytics
        // sink。bootstrap 已把 GatedAnalyticsSink 包好，runtime 切换 noop / 真
        // 实 sink 是 sink 自身职责，不在此装配。
        analytics: deps.analytics.clone(),
    }))
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
    /// 文件传输 lifecycle 入口(5 个动作 + seed + link)。daemon 入口
    /// 必传;CLI / 单元测试可留 `None`。详见
    /// [`AppFacade::file_transfer`](uc_application::facade::AppFacade)。
    pub file_transfer: Option<Arc<FileTransferFacade>>,
    /// 底层 `BlobTransferPort`(`IrohBlobTransferAdapter`)直连引用,供
    /// `ClipboardHistoryFacade` 在 `delete_entry` / `clear_history` 时
    /// 调 `untag` 释放对应 entry 对 iroh-blobs 的引用。与 `blob_transfer`
    /// 字段(承载发布/拉取 use case 的 facade)分开装配:facade 用于
    /// "发布、拉取 blob"业务动作,这个 port 用于"释放 blob 引用"基础
    /// 设施动作,两者共享同一个底层 adapter 实例。
    /// `None` 表示该装配场景不接入 blob 系统(纯文本 / 测试场景),
    /// 此时 untag 直接跳过。
    pub blob_transfer_port: Option<Arc<dyn uc_core::ports::blob::BlobTransferPort>>,
    pub clipboard_restore: Option<ClipboardRestoreAssembly>,
    pub search_coordinator: Option<Arc<SearchCoordinator>>,
    /// 移动端同步 PUT 路径用的 `ApplyInboundClipboardUseCase` 实例。
    ///
    /// daemon 入口在自身装配过程中已经构造一份 enhanced 版本(带
    /// `with_blob_materializer` + `with_host_event_emitter`),并把同一份
    /// 实例同时喂给 `MobileSyncFacade`(本字段)与 `InboundClipboardFacade`
    /// (worker 装配)。GUI 进程内 daemon 也走同一路径。
    ///
    /// CLI / tauri 等不接 LAN listener 的入口可以留 `None`,bootstrap 会
    /// 内置一份 fallback —— 只让 `MobileSyncFacade` 编得过, PUT 路径若
    /// 真的被调用会以 `Internal("mobile_sync PUT path not configured")`
    /// 失败,符合"CLI 不开 LAN 监听因此 PUT 永远不会触发"的实际语义。
    pub mobile_sync_apply_inbound: Option<Arc<ApplyInboundClipboardUseCase>>,
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
    // Mobile-sync facade 自动装配 —— 与 lifecycle / encryption / settings 同
    // 待遇，所有桌面入口（daemon / CLI）都自动带上，不需要 caller 传。
    //
    // Phase 2 适配器形态：4 个 in-memory + 1 个 OS 真实探测
    // (`NetworkInterfaceLanProbe`)。`endpoint_info` 这个 adapter 暂时无人写
    // 入，意味着 `current_lan_endpoint()` 永远返回 `None` —— register flow
    // 会以 `LanListenerDisabled` 失败。Phase 3 接入 daemon LAN listener
    // 时把 listener 启停信号反向喂回 `InMemoryMobileSyncEndpointInfoAdapter`
    // 的 `set` / `clear`，这一处 wiring 即可让 register flow 端到端跑通。
    // mobile_sync facade 装配规则 (Phase 4 + PR #610 合并后):
    //   - daemon 路径不走本函数装 mobile_sync —— 通过 `build_daemon_lifecycle_facades`
    //     单独构造 enhanced 版本, 然后 `install_daemon_lifecycle` swap 进
    //     `AppFacade.mobile_sync` OnceLock。所以 daemon 路径调本函数时
    //     `mobile_sync_apply_inbound` 必须为 `None`, OnceLock 留空待 swap。
    //   - GUI 路径同样不装 (LAN listener 由 daemon 起, GUI 进程内没有自己的
    //     LAN PUT 入口),`mobile_sync_apply_inbound: None` → OnceLock 留空,
    //     daemon 启动时 swap 进 enhanced 版本。
    //   - CLI 路径需要 mobile_sync facade 跑查询命令 (`list_devices` 等),
    //     显式传一份 fallback `Some(build_fallback_apply_inbound(deps))` 即可。
    //
    // `Some` 才装,`None` 留空 —— OnceLock 语义下,留空才能让 daemon-lifecycle
    // swap 不撞已装入的 OnceLock。
    let mobile_sync_facade = options
        .mobile_sync_apply_inbound
        .clone()
        .map(|apply_inbound| {
            build_mobile_sync_facade(
                deps,
                storage_paths,
                apply_inbound,
                options.file_transfer.clone(),
                // CLI fallback 装配:无常驻 daemon, 不需要 in-process hot-swap。
                // settings 改动等下次 daemon 进程启动一次性生效。
                None,
                // CLI fallback 不接 outbound dispatcher(`ClipboardOutboundFacade`
                // 装配链需要 worker 装配过程提供, 见
                // `uc-desktop::daemon::runtime_assembly`),mobile 上传仅落地
                // 本机, 不向其他 paired peers fan-out。daemon 入口走
                // `build_daemon_lifecycle_facades` 那条路径, 在那里以
                // `Some(clipboard_outbound)` 装入完整 fan-out 能力(含文件 blob
                // 发布)。
                None,
            )
        });

    let clipboard_restore = options.clipboard_restore.map(|restore| {
        Arc::new(ClipboardRestoreFacade::new(ClipboardRestoreFacadeDeps {
            entry_repo: deps.clipboard.clipboard_entry_repo.clone(),
            selection_repo: deps.clipboard.selection_repo.clone(),
            representation_repo: deps.clipboard.representation_repo.clone(),
            payload_resolver: deps.clipboard.payload_resolver.clone(),
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
            file_cache_dir: Some(storage_paths.file_cache_dir.clone()),
            blob_transfer: options.blob_transfer_port.clone(),
            settings: deps.settings.clone(),
            device_identity: deps.device.device_identity.clone(),
            clock: deps.system.clock.clone(),
        })),
        clipboard_sync: options.clipboard_sync,
        blob_transfer: options.blob_transfer,
        file_transfer: options.file_transfer,
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
        mobile_sync: mobile_sync_facade,
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
pub async fn build_cli_app_facade(
    log_profile: Option<uc_observability::LogProfile>,
) -> anyhow::Result<Arc<AppFacade>> {
    let ctx = crate::builders::build_cli_context_with_profile(log_profile).await?;
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
    let (config, wired) = crate::builders::build_cli_wiring_context(log_profile).await?;
    let storage_paths = get_storage_paths(&config)?;

    // Phase 94 NETSET-03：与 builders.rs 同模式（D-B1 选项 B 现状决策 — 见
    // 094-CONTEXT.md `<deferred>` 后续 phase 实施 `SettingsLoadError` 偿还）。
    let settings = wired
        .deps
        .settings
        .load()
        .await
        .map_err(|err| anyhow::anyhow!("settings load failed at startup: {err}"))?;
    let allow_relay_fallback = settings.network.allow_relay_fallback;
    let allow_overlay_network_addrs = settings.network.allow_overlay_network_addrs;

    // 【checker BLOCKER 4 — 单一取反点铁律】见 builders.rs 同处注释。
    // 不在此处内联 `let disable_relays = !allow_relay_fallback;`。
    let iroh_config = crate::network_policy::relay_policy_to_iroh_config(
        allow_relay_fallback,
        allow_overlay_network_addrs,
        None,
    );

    tracing::info!(
        target: "settings.network",
        allow_relay_fallback,
        disable_relays = iroh_config.disable_relays,
        allow_overlay_network_addrs = iroh_config.allow_overlay_network_addrs,
        "applying network settings: allow_relay_fallback={} → disable_relays={}, allow_overlay_network_addrs={}",
        allow_relay_fallback,
        iroh_config.disable_relays,
        iroh_config.allow_overlay_network_addrs,
    );

    let assembly = build_space_setup_assembly(&wired, iroh_config)
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

    // CLI 不接 LAN listener,但仍需 `mobile_sync` facade 跑查询命令
    // (`list_devices` / `get_settings` 等)。显式传 fallback apply_inbound,
    // PUT 路径真被调到才报 "not configured" Err —— CLI 场景下不会发生。
    let mobile_sync_apply_inbound = build_fallback_apply_inbound(deps);

    let app_facade = build_app_facade_from_deps(
        deps,
        &storage_paths,
        lifecycle_status,
        AppFacadeAssemblyOptions {
            space_setup: Some(assembly.facade.clone()),
            member_roster: Some(assembly.roster.clone()),
            clipboard_sync: Some(assembly.clipboard_sync.clone()),
            blob_transfer: Some(assembly.blob.clone()),
            blob_transfer_port: Some(Arc::clone(&assembly.blob_transfer)),
            file_transfer: Some(wired.file_transfer_facade.clone()),
            search_coordinator: Some(search_coordinator),
            mobile_sync_apply_inbound: Some(mobile_sync_apply_inbound),
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
