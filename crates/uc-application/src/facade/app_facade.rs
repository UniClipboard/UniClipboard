//! `AppFacade` — Slice 1 cross-domain aggregator.
//!
//! Per `uc-application/AGENTS.md` §11.4 external consumers reach the
//! application layer exclusively through a facade. `AppFacade` is the
//! single outward-facing type; internally it just groups sub-facades,
//! each constructed from its own `*Deps` bundle, so adding a new
//! domain does not cascade into a constructor explosion.
//!
//! # Current scope (Slice 1 · P4)
//!
//! * [`SpaceSetupFacade`] — A1 `initialize_space`, A2 `unlock_space`
//!
//! # Deferred
//!
//! * `PairingFacade` (B1 / B2) → P7+
//! * `SyncFacade` (C1 / C2 / C3) → Slice 2
//! * F1 `on_startup` / F2 `on_shutdown` → P6 (lives inside the
//!   sub-facades once `StartNetwork` plumbing exists)
//! * Daemon / tauri / CLI switching from the legacy sub-facades
//!   (`SetupFacade`, `PairingFacade`) to `AppFacade` → Slice 1.5 or
//!   later. Those sub-facades remain `pub` this slice to keep existing
//!   entry points working. D18 retired the legacy access facade because
//!   its state machine had no dispatcher, while the real admit path runs
//!   through `PairingInboundOrchestrator`.

use std::net::IpAddr;
use std::sync::{Arc, OnceLock};

use thiserror::Error;
use tokio::sync::broadcast;

use crate::facade::config_migration::ConfigMigrationFacade;
use crate::facade::file_transfer::FileTransferFacade;
use crate::facade::mobile_sync::MobileSyncFacade;
use crate::facade::roster::{MemberSummary, PeerSnapshotView, RosterError};
use crate::facade::settings::{GeneralSettingsPatch, SettingsPatch};
use crate::facade::space_setup::{EnsureReachableAllError, EnsureReachableAllReport};
use crate::facade::space_setup::{
    InitializeSpaceError, InitializeSpaceInput, InitializeSpaceResult, IssuePairingInvitationError,
    IssuePairingInvitationResult, MigrationProgress, PairingInvitationAddressCandidate,
    PairingOutcome, QueryMigrationProgressError, RedeemPairingInvitationError,
    RedeemPairingInvitationInput, RedeemPairingInvitationResult, SwitchSpaceError,
    SwitchSpaceInput, SwitchSpaceResult, TryResumeSessionError,
};
use crate::facade::upgrade::UpgradeFacade;
use crate::facade::{
    BlobTransferError, BlobTransferFacade, ClipboardHistoryFacade, ClipboardOutboundFacade,
    ClipboardRestoreFacade, ClipboardSyncError, ClipboardSyncFacade, DeviceFacade,
    DiagnosticsFacade, DispatchEntryOutcome, EncryptionFacade, EncryptionFacadeError,
    EncryptionStateView, FetchBlobCommand, FetchBlobResult, FetchBlobToPathCommand,
    FetchBlobToPathResult, InboundNotice, LifecycleFacade, MemberRosterFacade, PublishBlobCommand,
    PublishBlobPathCommand, PublishBlobResult, ResendEntryCommand, ResendEntryError, ResendReport,
    ResourceFacade, SearchFacade, SearchFacadeError, SearchPageView, SearchQueryInput,
    SearchRebuildAcceptedView, SearchStatusView, SettingsFacade, SettingsFacadeError,
    SpaceSetupFacade, StorageFacade,
};
use crate::usecases::clipboard_sync::V3BlobRef;
use uc_core::ids::DeviceId;
use uc_core::ports::{PresenceError, PresenceEvent, ReachabilityState};
use uc_core::ClipboardChangeOrigin;
use uc_core::SystemClipboardSnapshot;

/// 应用层统一入口。
///
/// 新增外部业务调用应优先通过本文件中的顶层方法进入。公开子字段是历史兼容
/// 状态,后续收口 daemon / Tauri 路径时应继续减少直接访问。
///
/// # daemon-lifecycle 字段(启动期一次性装入)
///
/// 下面 6 个字段绑定 daemon-lifecycle 资源(iroh node、clipboard_sync 链、
/// LAN PUT 入站等)。方案 C (2026-05-11) 取消 in-process daemon reload 后,
/// daemon 在进程内只起一次, 这 6 个字段也只装入一次 —— GUI shell 启动期
/// 为空, daemon 启动时由 [`Self::install_daemon_lifecycle`] set 进
/// [`OnceLock`], daemon (= 进程) 退出时由 Arc drop 自然回收。
///
/// - `space_setup`、`member_roster` —— iroh 网络栈相关
/// - `clipboard_sync`、`blob_transfer` —— iroh 上的同步业务
/// - `clipboard_outbound` —— 用户主动 resend 入口 (ADR-005 §2.5)
/// - `mobile_sync` —— 因绑 enhanced apply_inbound (带 blob_materializer +
///   host_event_emitter) 也是 daemon-lifecycle
///
/// 用 [`OnceLock`] 而非 `RwLock<Option<Arc<X>>>`:读路径 GUI command +
/// daemon worker 高频访问, set-once 语义恰好匹配 "启动期装入, 进程内不再
/// 切换" 的真实生命周期, 比 `RwLock` 省一次锁且语义更紧。
pub struct AppFacade {
    pub space_setup: OnceLock<Arc<SpaceSetupFacade>>,
    pub member_roster: OnceLock<Arc<MemberRosterFacade>>,
    pub lifecycle: Arc<LifecycleFacade>,
    pub encryption: Arc<EncryptionFacade>,
    pub resource: Arc<ResourceFacade>,
    pub clipboard_history: Arc<ClipboardHistoryFacade>,
    pub clipboard_sync: OnceLock<Arc<ClipboardSyncFacade>>,
    pub blob_transfer: OnceLock<Arc<BlobTransferFacade>>,
    /// 用户主动 resend 的入口(对应 commit B3 的 [`ResendEntryUseCase`])。
    /// daemon-lifecycle 字段:GUI shell 启动期为空, daemon 启动时由
    /// [`Self::install_daemon_lifecycle`] 装入。GUI / Tauri command /
    /// CLI `uniclip send --resend` 都从这一份读;未装入(daemon 未启)
    /// 场景下调用方拿到 None 应给"功能未启用"反馈。
    pub clipboard_outbound: OnceLock<Arc<ClipboardOutboundFacade>>,
    /// 文件传输 lifecycle 入口 —— 5 个动作 + seed_receiver_context +
    /// link_transfer_to_entry。`None` 表示当前装配场景未接入 lifecycle
    /// (典型:仅查询的 CLI / 单元测试)。进程级单例(在
    /// `BackgroundRuntimeDeps` 里构造,GUI shell 启动期通过
    /// `AppFacadeAssemblyOptions::file_transfer` 一次性装入),不在
    /// daemon-lifecycle OnceLock swap 范围内。
    pub file_transfer: Option<Arc<FileTransferFacade>>,
    /// CLI / 仅查询场景下 daemon/Tauri 不构造 restore facade,这里是 None。
    /// daemon API handler 取出前需做存在性检查。
    ///
    /// 不在 daemon-lifecycle 范围 —— 它绑 ClipboardWriteCoordinator
    /// (进程级) 与 integration_mode (静态),GUI shell 启动期一次性装入。
    pub clipboard_restore: Option<Arc<ClipboardRestoreFacade>>,
    pub search: Arc<SearchFacade>,
    pub settings: Arc<SettingsFacade>,
    pub diagnostics: Arc<DiagnosticsFacade>,
    pub device: Arc<DeviceFacade>,
    pub storage: Arc<StorageFacade>,
    /// 整机配置迁移 facade（导出 / 导入预览 / 暂存导入）。所有桌面入口共享
    /// 同一份;daemon HTTP `/config/*` 端点经它执行。装配在
    /// `wire_dependencies`,与 `encryption` / `storage` 同流向。
    pub config_migration: Arc<ConfigMigrationFacade>,
    /// 升级检测 facade（P1 thin）。所有桌面入口（GUI / daemon / CLI）共享同
    /// 一份；启动期 host 调一次 `upgrade.detect_on_startup()` 决定是否触发
    /// 重新配对引导等动作。
    pub upgrade: Arc<UpgradeFacade>,
    /// 移动端同步 facade（v1：iOS Shortcut）。
    ///
    /// daemon-lifecycle 字段:GUI shell 启动期为空, daemon 启动时由
    /// [`Self::install_daemon_lifecycle`] 装入 enhanced 版本 (绑 daemon
    /// worker apply_inbound)。daemon 未启动场景下调用方拿到 `None` 应
    /// 直接给用户报"功能未启用 / daemon 未就绪"。
    pub mobile_sync: OnceLock<Arc<MobileSyncFacade>>,
}

/// 一次性把 daemon-lifecycle 资源装进 [`AppFacade`] 的 6 个 OnceLock 字段。
///
/// 由 daemon 启动装配在 daemon 启动时调用 [`AppFacade::install_daemon_lifecycle`]
/// 触发。daemon 是进程级单例 (没有 in-process reload), 整个进程生命周期里
/// install 只会被调一次。ADR-008 P3-3 (B2'-3) 后 `AppFacade` 只存在于 daemon
/// 进程 (GUI 已是纯客户端,不再持有进程内 facade);CLI 的 in-process 置备
/// 同样走这条 path。
pub struct DaemonLifecycleFacades {
    pub space_setup: Arc<SpaceSetupFacade>,
    pub member_roster: Arc<MemberRosterFacade>,
    pub clipboard_sync: Arc<ClipboardSyncFacade>,
    pub blob_transfer: Arc<BlobTransferFacade>,
    pub clipboard_outbound: Arc<ClipboardOutboundFacade>,
    pub mobile_sync: Arc<MobileSyncFacade>,
}

impl AppFacade {
    /// Compose from already-constructed sub-facades.
    ///
    /// Bootstrap builds each sub-facade from its own `*Deps` bundle and
    /// hands them here — the aggregator never sees raw ports.
    pub fn new(parts: AppFacadeParts) -> Self {
        Self {
            space_setup: once_lock_from(parts.space_setup),
            member_roster: once_lock_from(parts.member_roster),
            lifecycle: parts.lifecycle,
            encryption: parts.encryption,
            resource: parts.resource,
            clipboard_history: parts.clipboard_history,
            clipboard_sync: once_lock_from(parts.clipboard_sync),
            blob_transfer: once_lock_from(parts.blob_transfer),
            clipboard_outbound: once_lock_from(parts.clipboard_outbound),
            file_transfer: parts.file_transfer,
            clipboard_restore: parts.clipboard_restore,
            search: parts.search,
            settings: parts.settings,
            diagnostics: parts.diagnostics,
            device: parts.device,
            storage: parts.storage,
            config_migration: parts.config_migration,
            upgrade: parts.upgrade,
            mobile_sync: once_lock_from(parts.mobile_sync),
        }
    }

    /// 把 daemon 启动时构造好的 6 份 lifecycle facade 一次性装入 AppFacade。
    ///
    /// 方案 C 后 daemon 进程内只起一次, 这条 path 每进程调一次。同一份
    /// `AppFacade` 整个进程生命周期共享; GUI command 与 daemon worker 都
    /// 从这一份读 —— LAN listener 写入的 endpoint_info、daemon 端 dispatch
    /// 的事件, GUI 都能立刻读到, 不再依赖跨多份 deps 共享 Arc。
    ///
    /// # Panics
    ///
    /// 重复调用 (6 个 OnceLock 中任一已被装入) panic, 视为编程错误 ——
    /// daemon 没有 reload 路径, 不该有第二次装入。
    pub fn install_daemon_lifecycle(&self, facades: DaemonLifecycleFacades) {
        self.space_setup
            .set(facades.space_setup)
            .map_err(|_| ())
            .expect("space_setup facade already installed; daemon is process-singleton");
        self.member_roster
            .set(facades.member_roster)
            .map_err(|_| ())
            .expect("member_roster facade already installed; daemon is process-singleton");
        self.clipboard_sync
            .set(facades.clipboard_sync)
            .map_err(|_| ())
            .expect("clipboard_sync facade already installed; daemon is process-singleton");
        self.blob_transfer
            .set(facades.blob_transfer)
            .map_err(|_| ())
            .expect("blob_transfer facade already installed; daemon is process-singleton");
        self.clipboard_outbound
            .set(facades.clipboard_outbound)
            .map_err(|_| ())
            .expect("clipboard_outbound facade already installed; daemon is process-singleton");
        self.mobile_sync
            .set(facades.mobile_sync)
            .map_err(|_| ())
            .expect("mobile_sync facade already installed; daemon is process-singleton");
    }

    /// A1:初始化空间。外部业务入口从 `AppFacade` 进入,不直接拿 `SpaceSetupFacade`。
    pub async fn initialize_space(
        &self,
        input: InitializeSpaceInput,
    ) -> Result<InitializeSpaceResult, InitializeSpaceError> {
        self.space_setup
            .get()
            .cloned()
            .ok_or_else(|| {
                InitializeSpaceError::Internal("space setup facade unavailable".to_string())
            })?
            .initialize_space(input)
            .await
    }

    /// 尝试静默恢复空间会话。
    pub async fn try_resume_session(&self) -> Result<bool, TryResumeSessionError> {
        self.space_setup
            .get()
            .cloned()
            .ok_or_else(|| {
                TryResumeSessionError::Internal("space setup facade unavailable".to_string())
            })?
            .try_resume_session()
            .await
    }

    /// 刷新成员在线状态。
    pub async fn refresh_presence(
        &self,
    ) -> Result<EnsureReachableAllReport, EnsureReachableAllError> {
        self.space_setup
            .get()
            .cloned()
            .ok_or(EnsureReachableAllError::Repository(
                "space setup facade unavailable".to_string(),
            ))?
            .refresh_presence()
            .await
    }

    /// 列出已配对 peer 的 `DeviceId`(本机已过滤)。供 desktop keepalive
    /// 调度器用来发现新 peer / 收回已删除 peer。Thin wrapper over
    /// [`SpaceSetupFacade::list_paired_peer_device_ids`].
    pub async fn list_paired_peer_device_ids(
        &self,
    ) -> Result<Vec<DeviceId>, EnsureReachableAllError> {
        self.space_setup
            .get()
            .cloned()
            .ok_or(EnsureReachableAllError::Repository(
                "space setup facade unavailable".to_string(),
            ))?
            .list_paired_peer_device_ids()
            .await
    }

    /// 对单个 peer 触发一次 `ensure_reachable`。供 desktop keepalive 调度
    /// 器在退避到期时按需拨号。Thin wrapper over
    /// [`SpaceSetupFacade::ensure_reachable_one`].
    pub async fn ensure_reachable_one(
        &self,
        device: &DeviceId,
    ) -> Result<ReachabilityState, PresenceError> {
        self.space_setup
            .get()
            .cloned()
            .ok_or_else(|| PresenceError::Internal("space setup facade unavailable".to_string()))?
            .ensure_reachable_one(device)
            .await
    }

    /// B1:签发配对邀请。
    pub async fn issue_pairing_invitation(
        &self,
    ) -> Result<IssuePairingInvitationResult, IssuePairingInvitationError> {
        self.space_setup
            .get()
            .cloned()
            .ok_or_else(|| {
                IssuePairingInvitationError::Internal("space setup facade unavailable".to_string())
            })?
            .issue_pairing_invitation()
            .await
    }

    /// 按指定本机地址签发配对邀请。
    pub async fn issue_pairing_invitation_for_address(
        &self,
        selected_ip: IpAddr,
    ) -> Result<IssuePairingInvitationResult, IssuePairingInvitationError> {
        self.space_setup
            .get()
            .cloned()
            .ok_or_else(|| {
                IssuePairingInvitationError::Internal("space setup facade unavailable".to_string())
            })?
            .issue_pairing_invitation_for_address(selected_ip)
            .await
    }

    /// 列出当前可用于配对邀请的本机地址。
    pub async fn list_pairing_invitation_addresses(
        &self,
    ) -> Result<Vec<PairingInvitationAddressCandidate>, IssuePairingInvitationError> {
        self.space_setup
            .get()
            .cloned()
            .ok_or_else(|| {
                IssuePairingInvitationError::Internal("space setup facade unavailable".to_string())
            })?
            .list_pairing_invitation_addresses()
            .await
    }

    /// B2:兑换配对邀请。
    pub async fn redeem_pairing_invitation(
        &self,
        input: RedeemPairingInvitationInput,
    ) -> Result<RedeemPairingInvitationResult, RedeemPairingInvitationError> {
        self.space_setup
            .get()
            .cloned()
            .ok_or_else(|| {
                RedeemPairingInvitationError::Internal("space setup facade unavailable".to_string())
            })?
            .redeem_pairing_invitation(input)
            .await
    }

    /// 已 setup 设备加入另一个 sponsor 空间，4 阶段重加密迁移。详见
    /// `usecases::setup::switch_space` 模块文档。
    pub async fn switch_space(
        &self,
        input: SwitchSpaceInput,
    ) -> Result<SwitchSpaceResult, SwitchSpaceError> {
        self.space_setup
            .get()
            .cloned()
            .ok_or_else(|| {
                SwitchSpaceError::Internal("space setup facade unavailable".to_string())
            })?
            .switch_space(input)
            .await
    }

    /// 查询当前 switch-space 迁移进度（粗粒度——只返回阶段和备份表条目数）。
    pub async fn query_migration_progress(
        &self,
    ) -> Result<MigrationProgress, QueryMigrationProgressError> {
        self.space_setup
            .get()
            .cloned()
            .ok_or_else(|| {
                QueryMigrationProgressError::Internal("space setup facade unavailable".to_string())
            })?
            .query_migration_progress()
            .await
    }

    /// 订阅配对完成事件。
    pub fn subscribe_pairing_completion(
        &self,
    ) -> Result<broadcast::Receiver<PairingOutcome>, IssuePairingInvitationError> {
        self.space_setup
            .get()
            .cloned()
            .map(|facade| facade.subscribe_pairing_completion())
            .ok_or_else(|| {
                IssuePairingInvitationError::Internal("space setup facade unavailable".to_string())
            })
    }

    /// 列出对外成员摘要。外部调用只经过 `AppFacade`,不直接依赖 roster 子 facade。
    pub async fn list_members(&self) -> Result<Vec<MemberSummary>, RosterError> {
        self.member_roster
            .get()
            .cloned()
            .ok_or(RosterError::Unavailable)?
            .list_members()
            .await
    }

    /// 列出带 presence 的 roster entry。
    pub async fn list_roster_entries(
        &self,
    ) -> Result<Vec<crate::facade::roster::RosterEntry>, RosterError> {
        self.member_roster
            .get()
            .cloned()
            .ok_or(RosterError::Unavailable)?
            .list_with_presence()
            .await
    }

    /// 发送一个剪贴板快照到在线 peer。
    ///
    /// `target_filter`:
    /// - `None` —— 全 fan-out（向所有 trusted online peer）;
    /// - `Some(list)` —— 仅向指定 device 集合 fan-out;空列表合法,表示零目标。
    ///
    /// 不绕过 `is_send_allowed` / member gating / presence 这三层 use case
    /// 内部检查,filter 在它们之后生效。
    pub async fn dispatch_clipboard_snapshot(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        target_filter: Option<Vec<DeviceId>>,
    ) -> Result<crate::facade::DispatchEntryOutcome, ClipboardSyncError> {
        self.clipboard_sync
            .get()
            .cloned()
            .ok_or_else(|| {
                ClipboardSyncError::Repository("clipboard sync facade unavailable".to_string())
            })?
            // CLI / 直接调用方不与某条 entry 绑定,跳过 delivery 落盘;
            // target_filter 透传到下层 dispatch_entry。
            .dispatch_snapshot(snapshot, origin, None, target_filter)
            .await
    }

    /// 订阅入站剪贴板通知。
    pub fn subscribe_inbound_clipboard_notices(
        &self,
    ) -> Result<broadcast::Receiver<InboundNotice>, ClipboardSyncError> {
        self.clipboard_sync
            .get()
            .cloned()
            .map(|facade| facade.subscribe_inbound_notices())
            .ok_or_else(|| {
                ClipboardSyncError::Repository("clipboard sync facade unavailable".to_string())
            })
    }

    /// 取一条 entry 的"来源 + 每个对端同步状态"完整视图。GUI detail
    /// 面板用它渲染"来自哪台设备 / 同步到了哪些设备 / 哪台失败"。
    pub async fn get_entry_delivery_view(
        &self,
        entry_id: &uc_core::ids::EntryId,
    ) -> Result<crate::facade::EntryDeliveryView, crate::facade::GetEntryDeliveryViewError> {
        let facade = self.clipboard_sync.get().cloned().ok_or_else(|| {
            crate::facade::GetEntryDeliveryViewError::Storage(
                "clipboard sync facade unavailable".to_string(),
            )
        })?;
        facade.get_entry_delivery_view(entry_id).await
    }

    /// 用户主动 resend 一条本机来源的 entry。GUI / Tauri command / CLI
    /// `uniclip send --resend` 都从这里进。详细语义见
    /// [`ClipboardOutboundFacade::resend_entry`]。
    ///
    /// daemon 未启动场景下 `clipboard_outbound` OnceLock 为空, 返回
    /// `ResendEntryError::Dispatch("clipboard outbound facade unavailable")`,
    /// 调用方应给"daemon 未就绪"反馈。
    pub async fn resend_entry(
        &self,
        cmd: ResendEntryCommand,
    ) -> Result<ResendReport, ResendEntryError> {
        let facade = self.clipboard_outbound.get().cloned().ok_or_else(|| {
            ResendEntryError::Dispatch("clipboard outbound facade unavailable".to_string())
        })?;
        facade.resend_entry(cmd).await
    }

    /// 发布 blob。
    pub async fn publish_blob(
        &self,
        command: PublishBlobCommand,
    ) -> Result<PublishBlobResult, BlobTransferError> {
        self.blob_transfer
            .get()
            .cloned()
            .ok_or_else(|| BlobTransferError::Publish("blob facade unavailable".to_string()))?
            .publish_blob(command)
            .await
    }

    /// 拉取 blob。
    pub async fn fetch_blob(
        &self,
        command: FetchBlobCommand,
    ) -> Result<FetchBlobResult, BlobTransferError> {
        self.blob_transfer
            .get()
            .cloned()
            .ok_or_else(|| BlobTransferError::Fetch("blob facade unavailable".to_string()))?
            .fetch_blob(command)
            .await
    }

    /// 流式 publish 一个磁盘文件作为 blob。
    ///
    /// 内存峰值与文件大小解耦(走 iroh-blobs `add_path` + reflink_or_copy);
    /// 适合 CLI / GUI 的 user-facing 大文件发送入口。
    pub async fn publish_blob_path(
        &self,
        command: PublishBlobPathCommand,
    ) -> Result<PublishBlobResult, BlobTransferError> {
        self.blob_transfer
            .get()
            .cloned()
            .ok_or_else(|| BlobTransferError::Publish("blob facade unavailable".to_string()))?
            .publish_blob_path(command)
            .await
    }

    /// 流式 fetch 一个 blob 到指定本地文件。
    ///
    /// 与 [`Self::fetch_blob`] 的差别:bytes 落在 `target_path`,不返回内存。
    /// 当 `command.transfer_context` 提供时,fetch 会被注册到 inflight
    /// registry 上;之后调 [`Self::cancel_inbound_transfer`] 可以中断它。
    pub async fn fetch_blob_to_path(
        &self,
        command: FetchBlobToPathCommand,
    ) -> Result<FetchBlobToPathResult, BlobTransferError> {
        self.blob_transfer
            .get()
            .cloned()
            .ok_or_else(|| BlobTransferError::Fetch("blob facade unavailable".to_string()))?
            .fetch_blob_to_path(command)
            .await
    }

    /// 把一个剪贴板快照连同已 publish 的 blob 引用一起 dispatch。
    ///
    /// 与 [`Self::dispatch_clipboard_snapshot`] 区别:本方法适用于 sender
    /// 已经把文件 publish 成 blob 的场景,blob_refs 会被编码进 V3 envelope
    /// 尾部扩展,接收端 inbound materializer 通过 ticket 拉取。
    pub async fn dispatch_clipboard_snapshot_with_blob_refs(
        &self,
        snapshot: SystemClipboardSnapshot,
        blob_refs: Vec<V3BlobRef>,
        origin: ClipboardChangeOrigin,
    ) -> Result<DispatchEntryOutcome, ClipboardSyncError> {
        self.clipboard_sync
            .get()
            .cloned()
            .ok_or_else(|| {
                ClipboardSyncError::Repository("clipboard sync facade unavailable".to_string())
            })?
            .dispatch_snapshot_with_blob_refs(snapshot, blob_refs, origin, None, None)
            .await
    }

    /// 取消一次进行中的 inbound 文件传输。
    ///
    /// 接收方主动撤回 fetch:trigger 内部 cancellation token + 撕掉
    /// iroh-blobs Downloader 用的 QUIC connection + 落 `Cancelled`
    /// domain event。幂等:同一 `transfer_id` 不在 inflight registry
    /// 时(没有进行中的 fetch / 已经被取消过)返回 `Ok(NotInflight)`,
    /// 实际撤回则返回 `Ok(Cancelled)` —— timeout sweep / 删除流程靠这个
    /// 区分来决定是否要走 fallback 终结(例如 `mark_failed` pending 行)。
    pub async fn cancel_inbound_transfer(
        &self,
        transfer_id: &str,
        reason: uc_core::FileTransferCancellationReason,
    ) -> Result<crate::facade::InboundCancelOutcome, BlobTransferError> {
        self.blob_transfer
            .get()
            .cloned()
            .ok_or_else(|| BlobTransferError::Fetch("blob facade unavailable".to_string()))?
            .cancel_inbound_transfer(transfer_id, reason)
            .await
    }

    /// 查询本地搜索索引。
    pub async fn search_query(
        &self,
        input: SearchQueryInput,
    ) -> Result<SearchPageView, SearchFacadeError> {
        self.search.query(input).await
    }

    /// 查询本地搜索状态。
    pub async fn search_status(&self) -> Result<SearchStatusView, SearchFacadeError> {
        self.search.status().await
    }

    /// 在当前进程内同步重建搜索索引。
    pub async fn rebuild_search_now(&self) -> Result<SearchRebuildAcceptedView, SearchFacadeError> {
        self.search.rebuild_now().await
    }

    /// 查询加密/初始化状态。
    pub async fn encryption_state(&self) -> Result<EncryptionStateView, EncryptionFacadeError> {
        self.encryption.state().await
    }

    /// 更新本机设备名。
    pub async fn set_device_name(&self, device_name: String) -> Result<(), SettingsFacadeError> {
        let current = self.settings.get().await?;
        if current.general.device_name.as_deref() == Some(device_name.as_str()) {
            return Ok(());
        }

        self.settings
            .update(SettingsPatch {
                general: Some(GeneralSettingsPatch {
                    device_name: Some(Some(device_name)),
                    auto_start: None,
                    silent_start: None,
                    auto_check_update: None,
                    auto_download_update: None,
                    theme: None,
                    theme_color: None,
                    theme_color_light: None,
                    theme_color_dark: None,
                    theme_overrides_light: None,
                    theme_overrides_dark: None,
                    language: None,
                    update_channel: None,
                    telemetry_enabled: None,
                    usage_analytics_enabled: None,
                    debug_mode: None,
                }),
                sync: None,
                retention_policy: None,
                security: None,
                pairing: None,
                keyboard_shortcuts: None,
                file_sync: None,
                network: None,
                quick_panel: None,
            })
            .await?;
        Ok(())
    }

    /// 列出对外 peer 快照。外部调用只经过 `AppFacade`,不直接依赖 roster 子 facade。
    pub async fn list_peer_snapshots(&self) -> Result<Vec<PeerSnapshotView>, RosterError> {
        self.member_roster
            .get()
            .cloned()
            .ok_or(RosterError::Unavailable)?
            .list_peer_snapshots()
            .await
    }

    /// 订阅成员在线状态变化。外部拿到的是 application 事件,不暴露 core 事件类型。
    pub fn subscribe_peer_presence_events(&self) -> Result<AppPresenceSubscription, RosterError> {
        let inner = self
            .member_roster
            .get()
            .cloned()
            .ok_or(RosterError::Unavailable)?
            .subscribe_presence_events();
        Ok(AppPresenceSubscription { inner })
    }
}

/// application 层 presence 事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPresenceEvent {
    pub device_id: String,
    pub state: String,
    pub at_ms: i64,
}

/// application 层 presence 订阅错误。
#[derive(Debug, Error)]
pub enum AppPresenceSubscriptionError {
    #[error("presence event receiver lagged by {0} messages")]
    Lagged(u64),
    #[error("presence event receiver closed")]
    Closed,
}

/// application 层 presence 订阅句柄。
pub struct AppPresenceSubscription {
    inner: broadcast::Receiver<PresenceEvent>,
}

impl AppPresenceSubscription {
    pub async fn recv(&mut self) -> Result<AppPresenceEvent, AppPresenceSubscriptionError> {
        self.inner
            .recv()
            .await
            .map(presence_event_to_app)
            .map_err(|err| match err {
                broadcast::error::RecvError::Lagged(skipped) => {
                    AppPresenceSubscriptionError::Lagged(skipped)
                }
                broadcast::error::RecvError::Closed => AppPresenceSubscriptionError::Closed,
            })
    }
}

fn once_lock_from<T>(value: Option<T>) -> OnceLock<T> {
    let cell = OnceLock::new();
    if let Some(v) = value {
        let _ = cell.set(v);
    }
    cell
}

fn presence_event_to_app(event: PresenceEvent) -> AppPresenceEvent {
    AppPresenceEvent {
        device_id: event.device_id.as_str().to_string(),
        state: reachability_state_to_string(event.state),
        at_ms: event.at.timestamp_millis(),
    }
}

fn reachability_state_to_string(state: ReachabilityState) -> String {
    match state {
        ReachabilityState::Online => "online",
        ReachabilityState::Offline => "offline",
        ReachabilityState::Unknown => "unknown",
    }
    .to_string()
}

pub struct AppFacadeParts {
    pub space_setup: Option<Arc<SpaceSetupFacade>>,
    pub member_roster: Option<Arc<MemberRosterFacade>>,
    pub lifecycle: Arc<LifecycleFacade>,
    pub encryption: Arc<EncryptionFacade>,
    pub resource: Arc<ResourceFacade>,
    pub clipboard_history: Arc<ClipboardHistoryFacade>,
    pub clipboard_sync: Option<Arc<ClipboardSyncFacade>>,
    pub blob_transfer: Option<Arc<BlobTransferFacade>>,
    pub clipboard_outbound: Option<Arc<ClipboardOutboundFacade>>,
    pub file_transfer: Option<Arc<FileTransferFacade>>,
    pub clipboard_restore: Option<Arc<ClipboardRestoreFacade>>,
    pub search: Arc<SearchFacade>,
    pub settings: Arc<SettingsFacade>,
    pub diagnostics: Arc<DiagnosticsFacade>,
    pub device: Arc<DeviceFacade>,
    pub storage: Arc<StorageFacade>,
    pub config_migration: Arc<ConfigMigrationFacade>,
    pub upgrade: Arc<UpgradeFacade>,
    pub mobile_sync: Option<Arc<MobileSyncFacade>>,
}
