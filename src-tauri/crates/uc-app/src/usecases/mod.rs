//! Business logic use cases
//! 是否是独立 Use Case，
//! 取决于"是否需要用户 / 系统再次做出决策"
//!
//! [ClipboardWatcher]
//        ↓
// CaptureClipboardUseCase
//         ↓
// ---------------------------------
//         ↓
// ListClipboardEntryPreviewsUseCase  → UI 列表
// GetClipboardEntryPreviewUseCase    → UI hover / detail
// ---------------------------------
//         ↓
// MaterializeClipboardSelectionUseCase → 粘贴 / 恢复 / 同步
pub mod app_lifecycle;
pub mod auto_unlock_encryption_session;
pub mod change_passphrase;
pub mod clipboard;
pub mod delete_clipboard_entry;
pub mod get_settings;
pub mod initialize_encryption;
pub mod internal;
pub mod list_clipboard_entries;
pub mod pairing;
pub mod settings;
pub mod setup;
pub mod space_access;
pub mod start_network;
pub mod start_network_after_unlock;
pub mod storage;
pub mod update_settings;
pub mod verify_keychain_access;

pub use app_lifecycle::{
    AppLifecycleCoordinator, AppLifecycleCoordinatorDeps, DeviceAnnouncer, LifecycleEvent,
    LifecycleEventEmitter, LifecycleState, LifecycleStatusPort, SessionReadyEmitter,
};
pub use auto_unlock_encryption_session::AutoUnlockEncryptionSession;
pub use clipboard::list_entry_projections::{
    EntryProjectionDto, ListClipboardEntryProjections, ListProjectionsError,
};
pub use delete_clipboard_entry::DeleteClipboardEntry;
pub use get_settings::GetSettings;
pub use initialize_encryption::InitializeEncryption;
pub use list_clipboard_entries::ListClipboardEntries;
pub use pairing::{
    AnnounceDeviceName, GetDeviceSyncSettings, GetLocalDeviceInfo, GetLocalPeerId,
    ListConnectedPeers, ListDiscoveredPeers, ListPairedDevices, LocalDeviceInfo, PairingConfig,
    PairingOrchestrator, ResolveConnectionPolicy, SetPairingState, StagedPairedDeviceStore,
    UnpairDevice, UpdateDeviceSyncSettings,
};
pub use setup::{MarkSetupComplete, SetupError, SetupOrchestrator};
pub use start_network::StartNetwork;
pub use start_network_after_unlock::StartNetworkAfterUnlock;
pub use uc_core::ports::{StartClipboardWatcherError, StartClipboardWatcherPort};
pub use update_settings::UpdateSettings;
pub use verify_keychain_access::VerifyKeychainAccess;
