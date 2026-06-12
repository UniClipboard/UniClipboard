//! daemon-lifecycle 子 facade 装配。
//!
//! Phase 4 重构后,进程内只有 GUI shell 启动时装的一份 [`AppFacade`]。
//! daemon 启动时把 5 个 daemon-lifecycle 子 facade
//! ([`DaemonLifecycleFacades`]) 一次性 swap 进 [`AppFacade`] 的对应字段;
//! daemon 停止时清空。
//!
//! 不再装第二份完整 `AppFacade` —— `lifecycle` / `encryption` / `settings` /
//! `device` / `storage` / `clipboard_history` / `search` / `clipboard_restore` /
//! `file_transfer` 都是进程级,GUI 端启动时一次性装入,daemon 启停时不动。

use std::sync::Arc;

use uc_application::deps::AppDeps;
use uc_application::facade::{
    AppPaths, BlobTransferFacade, ClipboardOutboundFacade, ClipboardRestoreFacade,
    ClipboardSyncFacade, DaemonLifecycleFacades, FileTransferFacade,
};
use uc_application::ApplyInboundClipboardUseCase;
use uc_bootstrap::{build_mobile_sync_facade, SpaceSetupAssembly};
use uc_core::ports::MobileLanLifecyclePort;

/// 构造 daemon-lifecycle 装配输入。
pub struct DaemonLifecycleFacadesInput<'a> {
    pub deps: &'a AppDeps,
    pub storage_paths: &'a AppPaths,
    pub space_setup_assembly: &'a SpaceSetupAssembly,
    pub clipboard_sync: Arc<ClipboardSyncFacade>,
    pub blob_transfer: Arc<BlobTransferFacade>,
    /// 进程级 file-transfer facade (来自 `BackgroundRuntimeDeps`)。
    /// daemon 装配 `MobileSyncFacade` 时必传 —— SyncDoc apply 后 link +
    /// complete 让 mobile_lan transfer 在 file_transfer 表里闭环。
    pub file_transfer: Arc<FileTransferFacade>,
    /// daemon worker 装配过程中已构造好的 enhanced
    /// `ApplyInboundClipboardUseCase` (with_blob_materializer +
    /// with_host_event_emitter)。同一份实例同时喂给 mobile_sync facade
    /// (本字段) 与 InboundClipboardFacade (worker 装配),让 LAN PUT 路径
    /// 与 P2P 入站走同一条 ApplyInbound 链 (host event 单一源 / blob 状态共享)。
    pub mobile_sync_apply_inbound: Arc<ApplyInboundClipboardUseCase>,
    /// daemon worker 装配过程中构造的 `ClipboardOutboundFacade`(与
    /// `ClipboardWatcherWorker` 共用一份)。同一份实例喂给 mobile_sync
    /// facade, 让"移动端 PUT 落地本机 → fan-out 给其他桌面"完全走本机捕获
    /// 的同一条出站管线 ——
    /// 文件 blob 发布、大图 V3BlobRef 剥离、`OutboundSyncPlanner` 控制
    /// 都自动适用, 不再重复实现。
    pub clipboard_outbound: Arc<ClipboardOutboundFacade>,
    /// LAN 监听器生命周期 port —— 让 `update_settings` 写盘后立即把
    /// listener 状态对齐到新设置, 无需重启进程。同一份 controller 实例同时
    /// 喂给 `MobileSyncFacade`(本字段) 与 daemon `run()`(`DaemonApp`),
    /// 两条链路共用单点状态机。
    pub lan_lifecycle: Arc<dyn MobileLanLifecyclePort>,
    /// 进程级 `ClipboardRestoreFacade`。移动端 PUT 去重命中时恢复已有
    /// entry 到系统剪贴板。
    pub clipboard_restore: Option<Arc<ClipboardRestoreFacade>>,
}

/// 构造 5 个 daemon-lifecycle 子 facade。返回的 [`DaemonLifecycleFacades`]
/// 由 caller 通过 [`uc_application::facade::AppFacade::install_daemon_lifecycle`]
/// 一次性装入进程级 [`uc_application::facade::AppFacade`]。
pub fn build_daemon_lifecycle_facades(
    input: DaemonLifecycleFacadesInput<'_>,
) -> (DaemonLifecycleFacades, String) {
    let DaemonLifecycleFacadesInput {
        deps,
        storage_paths,
        space_setup_assembly,
        clipboard_sync,
        blob_transfer,
        file_transfer,
        mobile_sync_apply_inbound,
        clipboard_outbound,
        lan_lifecycle,
        clipboard_restore,
    } = input;

    let mobile_sync = build_mobile_sync_facade(
        deps,
        storage_paths,
        mobile_sync_apply_inbound.clone(),
        Some(file_transfer),
        Some(lan_lifecycle),
        // 让 mobile_sync facade 在 PUT 落地本机后自动把同一份 snapshot 走
        // 本机捕获完全相同的出站管线 fan-out 给 Space 内其他已配对设备
        // —— 闭合"手机 → 任一桌面 → 所有桌面"链路, 文件类型走
        // iroh-blobs free-file ref(`publish_blob_path`),接收端拉回并改
        // 写 file-list rep。daemon 这条装配路径必装,CLI fallback 与其他
        // 无 outbound dispatcher 的入口走 `None`。
        Some(Arc::clone(&clipboard_outbound)),
        clipboard_restore,
    );

    let local_device_id = deps.device.device_identity.current_device_id().to_string();

    let facades = DaemonLifecycleFacades {
        space_setup: Arc::clone(&space_setup_assembly.facade),
        member_roster: Arc::clone(&space_setup_assembly.roster),
        clipboard_sync,
        blob_transfer,
        // commit D · 让 GUI / Tauri command / CLI 通过 AppFacade.resend_entry
        // 触达 outbound facade(daemon-lifecycle 单例,跟 mobile_sync 同源)。
        clipboard_outbound: Arc::clone(&clipboard_outbound),
        mobile_sync,
    };

    (facades, local_device_id)
}
