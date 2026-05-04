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

use std::sync::Arc;

use thiserror::Error;
use tokio::sync::broadcast;

use crate::facade::roster::{MemberSummary, PeerSnapshotView, RosterError};
use crate::facade::settings::{GeneralSettingsPatch, SettingsPatch};
use crate::facade::space_setup::{EnsureReachableAllError, EnsureReachableAllReport};
use crate::facade::space_setup::{
    InitializeSpaceError, InitializeSpaceInput, InitializeSpaceResult, IssuePairingInvitationError,
    IssuePairingInvitationResult, MigrationProgress, PairingOutcome, QueryMigrationProgressError,
    RedeemPairingInvitationError, RedeemPairingInvitationInput, RedeemPairingInvitationResult,
    SwitchSpaceError, SwitchSpaceInput, SwitchSpaceResult, TryResumeSessionError,
};
use crate::facade::upgrade::UpgradeFacade;
use crate::facade::{
    BlobTransferError, BlobTransferFacade, ClipboardHistoryFacade, ClipboardRestoreFacade,
    ClipboardSyncError, ClipboardSyncFacade, DeviceFacade, EncryptionFacade, EncryptionFacadeError,
    EncryptionStateView, FetchBlobCommand, FetchBlobResult, InboundNotice, LifecycleFacade,
    MemberRosterFacade, PublishBlobCommand, PublishBlobResult, ResourceFacade, SearchFacade,
    SearchFacadeError, SearchPageView, SearchQueryInput, SearchRebuildAcceptedView,
    SearchStatusView, SettingsFacade, SettingsFacadeError, SpaceSetupFacade, StorageFacade,
};
use uc_core::ports::{PresenceEvent, ReachabilityState};
use uc_core::ClipboardChangeOrigin;
use uc_core::SystemClipboardSnapshot;

/// 应用层统一入口。
///
/// 新增外部业务调用应优先通过本文件中的顶层方法进入。公开子字段是历史兼容
/// 状态,后续收口 daemon / Tauri 路径时应继续减少直接访问。
pub struct AppFacade {
    pub space_setup: Option<Arc<SpaceSetupFacade>>,
    pub member_roster: Option<Arc<MemberRosterFacade>>,
    pub lifecycle: Arc<LifecycleFacade>,
    pub encryption: Arc<EncryptionFacade>,
    pub resource: Arc<ResourceFacade>,
    pub clipboard_history: Arc<ClipboardHistoryFacade>,
    pub clipboard_sync: Option<Arc<ClipboardSyncFacade>>,
    pub blob_transfer: Option<Arc<BlobTransferFacade>>,
    /// CLI / 仅查询场景下 daemon/Tauri 不构造 restore facade,这里是 None。
    /// daemon API handler 取出前需做存在性检查。
    pub clipboard_restore: Option<Arc<ClipboardRestoreFacade>>,
    pub search: Arc<SearchFacade>,
    pub settings: Arc<SettingsFacade>,
    pub device: Arc<DeviceFacade>,
    pub storage: Arc<StorageFacade>,
    /// 升级检测 facade（P1 thin）。所有桌面入口（GUI / daemon / CLI）共享同
    /// 一份；启动期 host 调一次 `upgrade.detect_on_startup()` 决定是否触发
    /// 重新配对引导等动作。
    pub upgrade: Arc<UpgradeFacade>,
}

impl AppFacade {
    /// Compose from already-constructed sub-facades.
    ///
    /// Bootstrap builds each sub-facade from its own `*Deps` bundle and
    /// hands them here — the aggregator never sees raw ports.
    pub fn new(parts: AppFacadeParts) -> Self {
        Self {
            space_setup: parts.space_setup,
            member_roster: parts.member_roster,
            lifecycle: parts.lifecycle,
            encryption: parts.encryption,
            resource: parts.resource,
            clipboard_history: parts.clipboard_history,
            clipboard_sync: parts.clipboard_sync,
            blob_transfer: parts.blob_transfer,
            clipboard_restore: parts.clipboard_restore,
            search: parts.search,
            settings: parts.settings,
            device: parts.device,
            storage: parts.storage,
            upgrade: parts.upgrade,
        }
    }

    /// A1:初始化空间。外部业务入口从 `AppFacade` 进入,不直接拿 `SpaceSetupFacade`。
    pub async fn initialize_space(
        &self,
        input: InitializeSpaceInput,
    ) -> Result<InitializeSpaceResult, InitializeSpaceError> {
        self.space_setup
            .as_ref()
            .ok_or_else(|| {
                InitializeSpaceError::Internal("space setup facade unavailable".to_string())
            })?
            .initialize_space(input)
            .await
    }

    /// 尝试静默恢复空间会话。
    pub async fn try_resume_session(&self) -> Result<bool, TryResumeSessionError> {
        self.space_setup
            .as_ref()
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
            .as_ref()
            .ok_or(EnsureReachableAllError::Repository(
                "space setup facade unavailable".to_string(),
            ))?
            .refresh_presence()
            .await
    }

    /// B1:签发配对邀请。
    pub async fn issue_pairing_invitation(
        &self,
    ) -> Result<IssuePairingInvitationResult, IssuePairingInvitationError> {
        self.space_setup
            .as_ref()
            .ok_or_else(|| {
                IssuePairingInvitationError::Internal("space setup facade unavailable".to_string())
            })?
            .issue_pairing_invitation()
            .await
    }

    /// B2:兑换配对邀请。
    pub async fn redeem_pairing_invitation(
        &self,
        input: RedeemPairingInvitationInput,
    ) -> Result<RedeemPairingInvitationResult, RedeemPairingInvitationError> {
        self.space_setup
            .as_ref()
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
            .as_ref()
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
            .as_ref()
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
            .as_ref()
            .map(|facade| facade.subscribe_pairing_completion())
            .ok_or_else(|| {
                IssuePairingInvitationError::Internal("space setup facade unavailable".to_string())
            })
    }

    /// 列出对外成员摘要。外部调用只经过 `AppFacade`,不直接依赖 roster 子 facade。
    pub async fn list_members(&self) -> Result<Vec<MemberSummary>, RosterError> {
        self.member_roster
            .as_ref()
            .ok_or(RosterError::Unavailable)?
            .list_members()
            .await
    }

    /// 列出带 presence 的 roster entry。
    pub async fn list_roster_entries(
        &self,
    ) -> Result<Vec<crate::facade::roster::RosterEntry>, RosterError> {
        self.member_roster
            .as_ref()
            .ok_or(RosterError::Unavailable)?
            .list_with_presence()
            .await
    }

    /// 发送一个剪贴板快照到在线 peer。
    pub async fn dispatch_clipboard_snapshot(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
    ) -> Result<crate::facade::DispatchEntryOutcome, ClipboardSyncError> {
        self.clipboard_sync
            .as_ref()
            .ok_or_else(|| {
                ClipboardSyncError::Repository("clipboard sync facade unavailable".to_string())
            })?
            .dispatch_snapshot(snapshot, origin)
            .await
    }

    /// 订阅入站剪贴板通知。
    pub fn subscribe_inbound_clipboard_notices(
        &self,
    ) -> Result<broadcast::Receiver<InboundNotice>, ClipboardSyncError> {
        self.clipboard_sync
            .as_ref()
            .map(|facade| facade.subscribe_inbound_notices())
            .ok_or_else(|| {
                ClipboardSyncError::Repository("clipboard sync facade unavailable".to_string())
            })
    }

    /// 发布 blob。
    pub async fn publish_blob(
        &self,
        command: PublishBlobCommand,
    ) -> Result<PublishBlobResult, BlobTransferError> {
        self.blob_transfer
            .as_ref()
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
            .as_ref()
            .ok_or_else(|| BlobTransferError::Fetch("blob facade unavailable".to_string()))?
            .fetch_blob(command)
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
                    theme: None,
                    theme_color: None,
                    language: None,
                    update_channel: None,
                    telemetry_enabled: None,
                }),
                sync: None,
                retention_policy: None,
                security: None,
                pairing: None,
                keyboard_shortcuts: None,
                file_sync: None,
                network: None,
            })
            .await?;
        Ok(())
    }

    /// 列出对外 peer 快照。外部调用只经过 `AppFacade`,不直接依赖 roster 子 facade。
    pub async fn list_peer_snapshots(&self) -> Result<Vec<PeerSnapshotView>, RosterError> {
        self.member_roster
            .as_ref()
            .ok_or(RosterError::Unavailable)?
            .list_peer_snapshots()
            .await
    }

    /// 订阅成员在线状态变化。外部拿到的是 application 事件,不暴露 core 事件类型。
    pub fn subscribe_peer_presence_events(&self) -> Result<AppPresenceSubscription, RosterError> {
        let inner = self
            .member_roster
            .as_ref()
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
    pub clipboard_restore: Option<Arc<ClipboardRestoreFacade>>,
    pub search: Arc<SearchFacade>,
    pub settings: Arc<SettingsFacade>,
    pub device: Arc<DeviceFacade>,
    pub storage: Arc<StorageFacade>,
    pub upgrade: Arc<UpgradeFacade>,
}
