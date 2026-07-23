//! Shared application-facade assembly owned by the cross-platform engine.
//!
//! Host entry points prepare platform capabilities and pass the resulting
//! application dependencies into the builders in this module.

use std::sync::Arc;

use async_trait::async_trait;
use uc_application::clipboard_capture::CaptureClipboardUseCase;
use uc_application::deps::AppDeps;
use uc_application::facade::settings::{RelayDiagnosticPort, RelayProbeError, RelayProbeReport};
use uc_application::facade::space_setup::SpaceSetupFacade;
#[cfg(feature = "lan-compat")]
use uc_application::facade::{
    ActiveClipboardFacade, IncomingMobileBuffer, MobileSyncFacade, MobileSyncFacadeDeps,
    MobileSyncSnapshotPorts,
};
use uc_application::facade::{
    AppFacade, AppFacadeParts, AppPaths, BlobTransferFacade, ClipboardCaptureFacade,
    ClipboardHistoryFacade, ClipboardHistoryFacadeDeps, ClipboardOutboundFacade,
    ClipboardRestoreFacade, ClipboardRestoreFacadeDeps, ClipboardSyncFacade, DeviceFacade,
    DiagnosticsFacade, DiagnosticsFacadeDeps, EncryptionFacade, EncryptionFacadeDeps,
    FileTransferFacade, LifecycleFacade, LifecycleFacadeDeps, LifecycleStatusGateway,
    MemberRosterFacade, ResourceFacade, ResourceFacadeDeps, SearchCoordinator, SearchFacade,
    SearchFacadeDeps, SettingsFacade, StorageFacade, StorageFacadeDeps, UpgradeFacade,
    UpgradeFacadeDeps,
};
#[cfg(feature = "lan-compat")]
use uc_application::ApplyInboundClipboardUseCase;
use uc_core::clipboard::ClipboardIntegrationMode;
#[cfg(feature = "lan-compat")]
use uc_infra::fs::FsInboundFileTarget;
#[cfg(feature = "lan-compat")]
use uc_infra::mobile_sync::{
    Argon2idPasswordHasher, FilesystemMobileFileStaging, NetworkInterfaceLanProbe,
    OsRngCredentialsMinter,
};
use uc_infra::network::iroh::{IrohRelayProbeAdapter, IrohRelayProbeError, IrohRelayProbeReport};

// ---------------------------------------------------------------------------
// IrohRelayDiagnosticAdapter
// ---------------------------------------------------------------------------

/// Adapts the infrastructure relay probe to the application diagnostic port.
///
/// The engine owns this adapter because it is the shared composition boundary
/// that can see both contracts without reversing either dependency direction.
struct IrohRelayDiagnosticAdapter {
    inner: Arc<IrohRelayProbeAdapter>,
}

#[async_trait]
impl RelayDiagnosticPort for IrohRelayDiagnosticAdapter {
    async fn probe(&self, url: &str) -> Result<RelayProbeReport, RelayProbeError> {
        self.inner
            .probe(url)
            .await
            .map(map_relay_probe_report)
            .map_err(map_relay_probe_error)
    }
}

fn map_relay_probe_report(report: IrohRelayProbeReport) -> RelayProbeReport {
    RelayProbeReport {
        latency_ms: report.latency_ms,
    }
}

fn map_relay_probe_error(err: IrohRelayProbeError) -> RelayProbeError {
    match err {
        IrohRelayProbeError::InvalidUrl(msg) => RelayProbeError::InvalidUrl(msg),
        IrohRelayProbeError::Dns(msg) => RelayProbeError::Dns(msg),
        IrohRelayProbeError::Tls(msg) => RelayProbeError::Tls(msg),
        IrohRelayProbeError::Handshake(msg) => RelayProbeError::Handshake(msg),
        IrohRelayProbeError::Timeout => RelayProbeError::Timeout,
        IrohRelayProbeError::Other(msg) => RelayProbeError::Other(msg),
    }
}

/// `ClipboardRestoreFacade` 的可选装配输入。
///
/// GUI 和 daemon 需要 restore 能力；部分 CLI 查询入口不需要，因此通过
/// 显式选项传入，避免各入口各自复制 facade 拼装代码。
pub struct ClipboardRestoreAssembly {
    pub write_coordinator: Arc<uc_application::clipboard_write::ClipboardWriteCoordinator>,
    pub integration_mode: ClipboardIntegrationMode,
    /// Optional restore-broadcast trigger (issue #1017). When present, a
    /// successful restore announces the activation to peers (gated). `None`
    /// for entry points without a network broadcast stack (CLI fallback).
    pub restore_broadcast: Option<uc_application::clipboard_write::RestoreBroadcastTrigger>,
}

/// 构造 [`ClipboardCaptureFacade`] —— "立即捕获当前 OS 剪贴板内容"的入口
/// (issue #1169:启动期恢复上次剪贴板记录前,先把当前可能已经变化的剪贴板
/// 内容落一条历史,避免被恢复动作覆盖丢失)。
///
/// 所有桌面入口(daemon / CLI / GUI shell)都用同一份 `AppDeps` 装得起来,
/// 不需要额外的 caller 提供的装配选项,因此 `AppFacade.clipboard_capture`
/// 是非 `Option` 字段。
fn build_clipboard_capture_facade(deps: &AppDeps) -> Arc<ClipboardCaptureFacade> {
    let capture_uc = Arc::new(
        CaptureClipboardUseCase::new(
            deps.clipboard.entry_ports.save.clone(),
            deps.clipboard.entry_ports.touch.clone(),
            deps.clipboard.entry_ports.find_by_snapshot_hash.clone(),
            deps.clipboard.clipboard_event_repo.clone(),
            deps.clipboard.representation_policy.clone(),
            deps.clipboard.representation_normalizer.clone(),
            deps.device.device_identity.clone(),
            deps.clipboard.representation_cache.clone(),
            deps.clipboard.spool_queue.clone(),
            deps.storage.blob_content_ingest.clone(),
            deps.storage.entry_file_set_repo.clone(),
            deps.settings.clone(),
            deps.clipboard.entry_ports.replace_content.clone(),
            deps.analytics.clone(),
        )
        .with_entry_identity_coordinator(deps.clipboard.entry_identity_coordinator.clone()),
    );
    Arc::new(
        ClipboardCaptureFacade::new(capture_uc, deps.clipboard.clipboard.clone())
            .with_entry_file_set_repository(deps.storage.entry_file_set_repo.clone()),
    )
}

/// 构造 [`MobileSyncFacade`] —— 抽出来供 daemon-lifecycle 装配复用。
///
/// `apply_inbound` 由 engine 运行期组装并传入。`endpoint_info`
/// 由 [`AppDeps`] 携带 (单例,daemon LAN listener 与 facade 共享同一份
/// Arc),无需 caller 透传。`file_transfer` 进程级 facade:daemon 装配
/// 必传,SyncDoc apply 后 link + complete 让 mobile_lan transfer 在
/// file_transfer 表里闭环。
#[cfg(feature = "lan-compat")]
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
    // Mobile-activation announce (issue #1017 PR7): the active-clipboard facade
    // (advance register + send-gated 0xC3 fan-out). daemon 装配传 `Some(...)`;
    // CLI fallback / 不接 active-clipboard 的入口传 `None`,移动端上传仅落地
    // 本机, 不向对端收敛。OS 剪贴板由入站管线负责写, 不经过这里。
    active_clipboard: Option<Arc<ActiveClipboardFacade>>,
) -> Arc<MobileSyncFacade> {
    Arc::new(MobileSyncFacade::new(MobileSyncFacadeDeps {
        clock: deps.system.clock.clone(),
        // v3 SyncClipboard 兼容: 单一 minter 一次性出 (username, password,
        // password_hash, device_id), Argon2id 作为口令 hash;无状态 ZST,
        // 装配处直接 new 即可。
        credentials_minter: Arc::new(OsRngCredentialsMinter),
        password_hasher: Arc::new(Argon2idPasswordHasher),
        devices: deps.mobile_sync.devices.clone(),
        endpoint_info: deps.mobile_sync.endpoint_info.clone(),
        lan_interface_probe: Arc::new(NetworkInterfaceLanProbe::new()),
        settings: deps.settings.clone(),
        apply_inbound,
        incoming_buffer: Arc::new(IncomingMobileBuffer::new()),
        file_staging: FilesystemMobileFileStaging::new_with_target_reserver(
            storage_paths.file_cache_dir.clone(),
            FsInboundFileTarget::new(deps.settings.clone()),
        ),
        snapshot_ports: MobileSyncSnapshotPorts {
            mobile_consumable_load: deps.clipboard.mobile_consumable_load.clone(),
            entry_repo: deps.clipboard.entry_ports.get.clone(),
            selection_repo: deps.clipboard.selection_repo.clone(),
            representation_repo: deps.clipboard.representation_ports.get.clone(),
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
        active_clipboard,
        find_entry_by_snapshot_hash: deps.clipboard.entry_ports.find_by_snapshot_hash.clone(),
        check_entry_availability: deps.clipboard.entry_ports.availability.clone(),
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
    /// daemon 启动期构造好的 outbound facade(commit D)。`AppFacade.resend_entry`
    /// 通过它落地 resend。GUI shell / CLI fallback 留 `None`,
    /// daemon 启动后 `install_daemon_lifecycle` 装入。
    pub clipboard_outbound: Option<Arc<ClipboardOutboundFacade>>,
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
            selection_repo: deps.clipboard.selection_repo.clone(),
            entry_ports: deps.clipboard.entry_ports.clone(),
            representation_ports: deps.clipboard.representation_ports.clone(),
            payload_resolver: deps.clipboard.payload_resolver.clone(),
            blob_store: deps.storage.blob_store.clone(),
            clock: deps.system.clock.clone(),
            device_identity: deps.device.device_identity.clone(),
            active_register: deps.clipboard.active_register.clone(),
            mobile_consumability: deps.clipboard.mobile_consumability.clone(),
            restore_broadcast: restore.restore_broadcast,
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
            initialize: deps.security.space_access_ports.initialize.clone(),
            resume_session: deps.security.space_access_ports.resume_session.clone(),
            is_unlocked: deps.security.space_access_ports.is_unlocked.clone(),
            lock: deps.security.space_access_ports.lock.clone(),
            verify_keychain_access: deps
                .security
                .space_access_ports
                .verify_keychain_access
                .clone(),
            mobile_consumable_backfill: deps.clipboard.mobile_consumable_backfill.clone(),
        })),
        resource: Arc::new(ResourceFacade::new(ResourceFacadeDeps {
            representation_by_blob_id: deps.clipboard.representation_ports.get_by_blob_id.clone(),
            representations_for_event: deps.clipboard.representation_ports.list_for_event.clone(),
            thumbnail_repo: deps.storage.thumbnail_repo.clone(),
            blob_store: deps.storage.blob_store.clone(),
            entry_repo: deps.clipboard.entry_ports.get.clone(),
        })),
        clipboard_history: Arc::new(ClipboardHistoryFacade::new(ClipboardHistoryFacadeDeps {
            entry_ports: deps.clipboard.entry_ports.clone(),
            selection_repo: deps.clipboard.selection_repo.clone(),
            representation_ports: deps.clipboard.representation_ports.clone(),
            event_writer: deps.clipboard.clipboard_event_repo.clone(),
            payload_resolver: deps.clipboard.payload_resolver.clone(),
            blob_store: deps.storage.blob_store.clone(),
            thumbnail_repo: deps.storage.thumbnail_repo.clone(),
            file_transfer_repo: deps.storage.file_transfer.entry_summary.clone(),
            entry_file_set_repo: deps.storage.entry_file_set_repo.clone(),
            search_index: Some(deps.search.search_index.clone()),
            file_cache_dir: Some(storage_paths.file_cache_dir.clone()),
            blob_transfer: options.blob_transfer_port.clone(),
            settings: deps.settings.clone(),
            device_identity: deps.device.device_identity.clone(),
            clock: deps.system.clock.clone(),
            cache_fs: deps.system.cache_fs.clone(),
        })),
        clipboard_capture: build_clipboard_capture_facade(deps),
        clipboard_sync: options.clipboard_sync,
        blob_transfer: options.blob_transfer,
        // GUI shell 启动期为空; daemon 起来后由
        // `AppFacade::install_daemon_lifecycle` 装入。
        clipboard_outbound: options.clipboard_outbound,
        file_transfer: options.file_transfer,
        clipboard_restore,
        search: Arc::new(SearchFacade::new(SearchFacadeDeps {
            search_index: deps.search.search_index.clone(),
            coordinator: options.search_coordinator,
        })),
        settings: Arc::new({
            // Relay 诊断 adapter 在 daemon 启动期一次性装配。infra 探测器
            // 初始化失败(TLS provider 缺失等)不应阻断整个 daemon 启动 ——
            // 走"探测能力缺失"路径,前端会得到 RelayProbeUnavailable。
            let mut facade = SettingsFacade::new(deps.settings.clone());
            match IrohRelayProbeAdapter::new() {
                Ok(probe) => {
                    let adapter = IrohRelayDiagnosticAdapter {
                        inner: Arc::new(probe),
                    };
                    facade = facade.with_relay_diagnostic(Arc::new(adapter));
                }
                Err(err) => {
                    tracing::warn!(
                        target: "bootstrap.network",
                        error = %err,
                        "relay probe adapter unavailable; settings.probe_relay_url will reject"
                    );
                }
            }
            facade
        }),
        diagnostics: Arc::new(DiagnosticsFacade::new(DiagnosticsFacadeDeps {
            settings: deps.settings.clone(),
            logs_dir: storage_paths.logs_dir.clone(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
        })),
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
        // Carried through from `wire_dependencies` (its db_pool / local_identity /
        // profile_id materials are only available there); see `AppDeps`.
        config_migration: deps.config_migration.clone(),
        upgrade: Arc::new(UpgradeFacade::new(UpgradeFacadeDeps {
            app_version_state: deps.app_version_state.clone(),
            setup_status: deps.setup_status.clone(),
        })),
        #[cfg(feature = "lan-compat")]
        mobile_sync: None,
    }))
}
