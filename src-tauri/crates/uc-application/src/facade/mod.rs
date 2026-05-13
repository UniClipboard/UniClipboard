//! Slice 1 application facade tree.
//!
//! Per `uc-application/AGENTS.md` §11.4 external consumers only see the
//! top-level `AppFacade` and the per-domain sub-facades it aggregates.
//! Use cases live under `crate::usecases::<domain>` and stay `pub(crate)`;
//! sub-facades expose them through domain-scoped methods.

pub mod app_facade;
pub mod app_paths;
pub mod blob_transfer;
pub mod clipboard;
pub mod clipboard_capture;
pub mod clipboard_history;
pub mod clipboard_inbound;
pub mod clipboard_live_index;
pub mod clipboard_outbound;
pub mod clipboard_restore;
pub mod device;
pub mod encryption;
pub mod file_transfer;
pub mod host_event;
pub mod lifecycle;
pub mod mobile_sync;
pub mod resource;
pub mod roster;
pub mod search;
pub mod settings;
pub mod setup_status;
pub mod space_setup;
pub mod storage;
pub mod upgrade;

pub use app_facade::{
    AppFacade, AppFacadeParts, AppPresenceEvent, AppPresenceSubscription,
    AppPresenceSubscriptionError, DaemonLifecycleFacades,
};
pub use app_paths::AppPaths;
pub use blob_transfer::{
    BlobTransferDeps, BlobTransferError, BlobTransferFacade, FetchBlobCommand, FetchBlobResult,
    FetchBlobToPathCommand, FetchBlobToPathResult, PublishBlobCommand, PublishBlobPathCommand,
    PublishBlobResult,
};
pub use clipboard::{
    ClipboardSyncDeps, ClipboardSyncError, ClipboardSyncFacade, DispatchEntryInput,
    DispatchEntryOutcome, DispatchEntryPerTarget, InboundAction, InboundNotice, IngestHandle,
};
pub use clipboard_capture::{
    CapturedClipboardEntryView, ClipboardCaptureFacade, ClipboardCaptureFacadeError,
    ClipboardCapturePort,
};
pub use clipboard_history::{
    CleanupResultView as ClipboardCleanupResultView,
    ClearHistoryResultView as ClipboardClearHistoryResultView, ClipboardHistoryError,
    ClipboardHistoryFacade, ClipboardHistoryFacadeDeps, ClipboardListInput, ClipboardStatsView,
    EntryDetailView, EntryProjectionView, EntryResourceView,
};
pub use clipboard_inbound::{
    InboundClipboardApplyError, InboundClipboardApplyInput, InboundClipboardApplyOutcome,
    InboundClipboardApplyPort, InboundClipboardFacade, InboundClipboardNoticeInput,
};
pub use clipboard_live_index::{
    ClipboardLiveIndexDeps, ClipboardLiveIndexError, ClipboardLiveIndexFacade,
    ClipboardLiveIndexInput, ClipboardLiveIndexOutcome, ClipboardLiveIndexPort,
    ClipboardLiveIndexer,
};
pub use clipboard_outbound::{
    ClipboardOutboundDeps, ClipboardOutboundDispatcher, ClipboardOutboundError,
    ClipboardOutboundFacade, ClipboardOutboundInput, ClipboardOutboundOutcome,
    ClipboardOutboundPort,
};
pub use clipboard_restore::{
    ClipboardRestoreError, ClipboardRestoreFacade, ClipboardRestoreFacadeDeps,
};
pub use device::{DeviceFacade, DeviceFacadeError, LocalDeviceInfoView};
pub use encryption::{
    EncryptionFacade, EncryptionFacadeDeps, EncryptionFacadeError, EncryptionStateView,
};
pub use file_transfer::{
    CancelTransfer, CompleteTransfer, FailTransfer, FileTransferApplicationError,
    FileTransferFacade, FileTransferFacadeDeps, LinkTransferToEntry, ReportTransferProgress,
    SeedReceiverContext, StartTransfer,
};
pub use host_event::{
    ClipboardHostEvent, ClipboardOriginKind, EmitError, FileTransferHostEventPublisher, HostEvent,
    HostEventEmitterPort, OutboundEntryIdCache, TransferHostEvent,
};
pub use lifecycle::{
    InMemoryLifecycleStatus, LifecycleFacade, LifecycleFacadeDeps, LifecycleFacadeError,
    LifecycleStateView, LifecycleStatusGateway,
};
pub use mobile_sync::mobile_sync_streaming_scope_nonce;
pub use mobile_sync::{
    ApplyIncomingMobileClipError, ApplyIncomingMobileClipInput, ApplyIncomingMobileClipOutcome,
    AuthenticateBasicAuthError, AuthenticateBasicAuthInput, AuthenticatedDevice,
    GetLatestMobileSyncDocError, GetMobileSyncFileError, GetMobileSyncFileOutput,
    GetMobileSyncSettingsError, IncomingMobileBuffer, IncomingMobileClipEvent,
    LanInterfaceOption as MobileSyncLanInterfaceOption,
    ListLanInterfacesError as MobileSyncListLanInterfacesError, ListMobileDevicesError,
    MobileDeviceSummary, MobileSyncFacade, MobileSyncFacadeDeps, MobileSyncSettingsView,
    MobileSyncSnapshotPorts, RegisterMobileShortcutDeviceError, RegisterMobileShortcutDeviceInput,
    RegisterMobileShortcutDeviceOutput, RevokeMobileDeviceError, RevokeMobileDeviceInput,
    ShortcutInstallMethod, ShortcutInstallMethodOption, SyncClipboardItemType, SyncClipboardMeta,
    UpdateMobileSyncSettingsError, UpdateMobileSyncSettingsInput, UpdateMobileSyncSettingsOutput,
    SYNC_CLIPBOARD_EX_INSTALL_URL,
};
pub use resource::{BinaryResourceView, ResourceFacade, ResourceFacadeDeps, ResourceFacadeError};
pub use roster::{
    connection_channel_to_wire, ConnectionChannel, ContentTypesPatch, ContentTypesView,
    MemberRosterDeps, MemberRosterFacade, MemberSummary, MemberSyncPreferencesPatch,
    MemberSyncPreferencesView, PeerSnapshotView, PresenceEvent, RosterEntry, RosterError,
};
pub use search::{
    map_search_error, ManualRebuildResult, SearchCoordinator, SearchCoordinatorDeps,
    SearchCoordinatorEvent, SearchFacade, SearchFacadeDeps, SearchFacadeError, SearchPageView,
    SearchProjectionBuilder, SearchQueryInput, SearchRebuildAcceptedView,
    SearchRebuildProgressView, SearchResultView, SearchStatusSnapshot, SearchStatusView,
};
pub use settings::{
    ContentTypesPatch as SettingsContentTypesPatch, ContentTypesView as SettingsContentTypesView,
    FileSyncSettingsPatch, FileSyncSettingsView, GeneralSettingsPatch, GeneralSettingsView,
    PairingSettingsPatch, PairingSettingsView, RetentionPolicyPatch, RetentionPolicyView,
    RetentionRulePatchValue, RetentionRuleView, RuleEvaluationView, SecuritySettingsPatch,
    SecuritySettingsView, SettingsFacade, SettingsFacadeError, SettingsPatch, SettingsView,
    ShortcutKeyView, SyncFrequencyView, SyncSettingsPatch, SyncSettingsView, ThemeView,
    UpdateChannelView,
};
pub use setup_status::SetupStatusFacade;
pub use space_setup::{
    CancelInvitationError, CurrentInvitation, InitializeSpaceError, InitializeSpaceInput,
    InitializeSpaceResult, IssuePairingInvitationError, IssuePairingInvitationResult,
    PairingFailureReason, PairingInvitationAddressCandidate, PairingOutcome, QuerySetupStateError,
    RedeemPairingInvitationError, RedeemPairingInvitationInput, RedeemPairingInvitationResult,
    ResetSpaceError, SetupStateView, SpaceSetupDeps, SpaceSetupFacade, UnlockSpaceError,
    UnlockSpaceInput, UnlockSpaceResult,
};
pub use storage::{
    ClearCacheResultView, StorageFacade, StorageFacadeDeps, StorageFacadeError, StorageStatsView,
};
pub use upgrade::{
    AcknowledgeUpgradeError, DetectUpgradeError, UpgradeFacade, UpgradeFacadeDeps, UpgradeStatus,
};
