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
pub mod file_sync;
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
pub mod sync_planner;
pub mod update_settings;
pub mod verify_keychain_access;

pub use app_lifecycle::{
    AppLifecycleCoordinator, AppLifecycleCoordinatorDeps, DeviceAnnouncer, DeviceNameAnnouncer,
    InMemoryLifecycleStatus, LifecycleEvent, LifecycleEventEmitter, LifecycleState,
    LifecycleStatusPort, LoggingLifecycleEventEmitter, LoggingSessionReadyEmitter,
    SessionReadyEmitter,
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
    GetP2pPeersSnapshot, ListConnectedPeers, ListDiscoveredPeers, ListPairedDevices,
    LocalDeviceInfo, PairingConfig, PairingOrchestrator, ResolveConnectionPolicy, SetPairingState,
    StagedPairedDeviceStore, UnpairDevice, UpdateDeviceSyncSettings,
};
pub use setup::{MarkSetupComplete, SetupError, SetupOrchestrator, SetupPairingFacadePort};
pub use start_network::StartNetwork;
pub use start_network_after_unlock::StartNetworkAfterUnlock;
pub use update_settings::UpdateSettings;
pub use verify_keychain_access::VerifyKeychainAccess;

pub use clipboard::clipboard_write_coordinator::{ClipboardWriteCoordinator, ClipboardWriteIntent};

pub use file_sync::{
    EarlyCompletionCache, EarlyCompletionInfo, FileTransferOrchestrator, SyncInboundFileUseCase,
    SyncOutboundFileUseCase, TrackInboundTransfersUseCase,
};

pub use sync_planner::{
    ClipboardSyncIntent, FileCandidate, FileSyncIntent, OutboundSyncPlan, OutboundSyncPlanner,
};

use crate::runtime::CoreRuntime;
use std::sync::Arc;

/// Pure domain use case accessors bound to CoreRuntime.
///
/// All methods return use case instances wired with ports from AppDeps.
/// No Tauri dependency. For Tauri-specific accessors (apply_autostart,
/// app_lifecycle_coordinator) and uc-infra-dependent
/// accessors (sync_inbound_clipboard, sync_outbound_clipboard), see AppUseCases
/// in uc-tauri.
pub struct CoreUseCases<'a> {
    pub(crate) runtime: &'a CoreRuntime,
}

impl<'a> CoreUseCases<'a> {
    pub fn new(runtime: &'a CoreRuntime) -> Self {
        Self { runtime }
    }

    /// Accesses the use case for querying clipboard history.
    pub fn list_clipboard_entries(&self) -> crate::usecases::ListClipboardEntries {
        crate::usecases::ListClipboardEntries::from_arc(
            self.runtime.deps.clipboard.clipboard_entry_repo.clone(),
        )
    }

    /// Create a `DeleteClipboardEntry` use case.
    pub fn delete_clipboard_entry(&self) -> crate::usecases::DeleteClipboardEntry {
        crate::usecases::DeleteClipboardEntry::from_ports(
            self.runtime.deps.clipboard.clipboard_entry_repo.clone(),
            self.runtime.deps.clipboard.selection_repo.clone(),
            self.runtime.deps.clipboard.clipboard_event_repo.clone(),
            self.runtime.deps.clipboard.representation_repo.clone(),
        )
        .with_file_cache_dir(self.runtime.storage_paths.file_cache_dir.clone())
    }

    /// Create a `ClearClipboardHistory` use case.
    pub fn clear_clipboard_history(&self) -> crate::usecases::clipboard::ClearClipboardHistory {
        crate::usecases::clipboard::ClearClipboardHistory::from_ports(
            self.runtime.deps.clipboard.clipboard_entry_repo.clone(),
            self.runtime.deps.clipboard.selection_repo.clone(),
            self.runtime.deps.clipboard.clipboard_event_repo.clone(),
            self.runtime.deps.clipboard.representation_repo.clone(),
        )
    }

    /// Get the GetEntryDetail use case.
    pub fn get_entry_detail(
        &self,
    ) -> crate::usecases::clipboard::get_entry_detail::GetEntryDetailUseCase {
        crate::usecases::clipboard::get_entry_detail::GetEntryDetailUseCase::new(
            self.runtime.deps.clipboard.clipboard_entry_repo.clone(),
            self.runtime.deps.clipboard.selection_repo.clone(),
            self.runtime.deps.clipboard.representation_repo.clone(),
            self.runtime.deps.storage.blob_store.clone(),
            self.runtime.deps.clipboard.payload_resolver.clone(),
        )
    }

    /// Get the GetEntryResource use case.
    pub fn get_entry_resource(
        &self,
    ) -> crate::usecases::clipboard::get_entry_resource::GetEntryResourceUseCase {
        crate::usecases::clipboard::get_entry_resource::GetEntryResourceUseCase::new(
            self.runtime.deps.clipboard.clipboard_entry_repo.clone(),
            self.runtime.deps.clipboard.selection_repo.clone(),
            self.runtime.deps.clipboard.representation_repo.clone(),
            self.runtime.deps.clipboard.payload_resolver.clone(),
        )
    }

    /// Resolve blob resource content by blob id.
    pub fn resolve_blob_resource(
        &self,
    ) -> crate::usecases::clipboard::resolve_blob_resource::ResolveBlobResourceUseCase {
        crate::usecases::clipboard::resolve_blob_resource::ResolveBlobResourceUseCase::new(
            self.runtime.deps.clipboard.representation_repo.clone(),
            self.runtime.deps.storage.blob_store.clone(),
        )
    }

    /// Get storage statistics use case.
    pub fn get_storage_stats(&self) -> crate::usecases::storage::GetStorageStats {
        crate::usecases::storage::GetStorageStats::new(self.runtime.storage_paths.clone())
    }

    /// Clear cache use case.
    pub fn clear_cache(&self) -> crate::usecases::storage::ClearCache {
        crate::usecases::storage::ClearCache::new(
            self.runtime.storage_paths.clone(),
            self.runtime.deps.system.cache_fs.clone(),
        )
    }

    /// Open data directory use case.
    pub fn open_data_directory(&self) -> crate::usecases::storage::OpenDataDirectory {
        crate::usecases::storage::OpenDataDirectory::new(
            self.runtime.storage_paths.clone(),
            self.runtime.deps.system.file_manager.clone(),
        )
    }

    /// List paired devices from repository.
    pub fn list_paired_devices(&self) -> crate::usecases::ListPairedDevices {
        crate::usecases::ListPairedDevices::new(self.runtime.deps.device.paired_device_repo.clone())
    }

    /// Get local peer id from network port.
    pub fn get_local_peer_id(&self) -> crate::usecases::GetLocalPeerId {
        crate::usecases::GetLocalPeerId::new(self.runtime.deps.network_ports.peers.clone())
    }

    /// Get local device info (peer id + device name).
    pub fn get_local_device_info(&self) -> crate::usecases::GetLocalDeviceInfo {
        crate::usecases::GetLocalDeviceInfo::new(
            self.runtime.deps.network_ports.peers.clone(),
            self.runtime.deps.settings.clone(),
        )
    }

    /// Announce local device name through the network port.
    pub fn announce_device_name(&self) -> crate::usecases::AnnounceDeviceName {
        crate::usecases::AnnounceDeviceName::new(self.runtime.deps.network_ports.peers.clone())
    }

    /// List discovered peers from network.
    pub fn list_discovered_peers(&self) -> crate::usecases::ListDiscoveredPeers {
        crate::usecases::ListDiscoveredPeers::new(self.runtime.deps.network_ports.peers.clone())
    }

    /// List connected peers from network.
    pub fn list_connected_peers(&self) -> crate::usecases::ListConnectedPeers {
        crate::usecases::ListConnectedPeers::new(self.runtime.deps.network_ports.peers.clone())
    }

    /// Get unified P2P peer snapshot combining discovered, connected, and paired peers.
    pub fn get_p2p_peers_snapshot(&self) -> crate::usecases::GetP2pPeersSnapshot {
        crate::usecases::GetP2pPeersSnapshot::new(
            self.runtime.deps.network_ports.peers.clone(),
            self.runtime.deps.device.paired_device_repo.clone(),
        )
    }

    /// Update pairing state for a peer.
    pub fn set_pairing_state(&self) -> crate::usecases::SetPairingState {
        crate::usecases::SetPairingState::new(self.runtime.deps.device.paired_device_repo.clone())
    }

    /// Get resolved sync settings for a specific device.
    pub fn get_device_sync_settings(&self) -> crate::usecases::GetDeviceSyncSettings {
        crate::usecases::GetDeviceSyncSettings::from_ports(
            self.runtime.deps.device.paired_device_repo.clone(),
            self.runtime.deps.settings.clone(),
        )
    }

    /// Update or clear per-device sync settings.
    pub fn update_device_sync_settings(&self) -> crate::usecases::UpdateDeviceSyncSettings {
        crate::usecases::UpdateDeviceSyncSettings::from_ports(
            self.runtime.deps.device.paired_device_repo.clone(),
        )
    }

    /// Unpair device and remove from repository.
    pub fn unpair_device(&self) -> crate::usecases::UnpairDevice {
        crate::usecases::UnpairDevice::new(
            self.runtime.deps.network_ports.pairing.clone(),
            self.runtime.deps.device.paired_device_repo.clone(),
        )
    }

    /// Resolve thumbnail resource content by representation id.
    pub fn resolve_thumbnail_resource(
        &self,
    ) -> crate::usecases::clipboard::resolve_thumbnail_resource::ResolveThumbnailResourceUseCase
    {
        crate::usecases::clipboard::resolve_thumbnail_resource::ResolveThumbnailResourceUseCase::new(
            self.runtime.deps.storage.thumbnail_repo.clone(),
            self.runtime.deps.storage.blob_store.clone(),
        )
    }

    /// Mark setup as complete (persists `.setup_status` flag).
    pub fn mark_setup_complete(&self) -> crate::usecases::MarkSetupComplete {
        crate::usecases::MarkSetupComplete::from_ports(self.runtime.deps.setup_status.clone())
    }

    /// Get the InitializeEncryption use case.
    pub fn initialize_encryption(&self) -> crate::usecases::InitializeEncryption {
        crate::usecases::InitializeEncryption::from_ports(
            self.runtime.deps.security.encryption.clone(),
            self.runtime.deps.security.key_material.clone(),
            self.runtime.deps.security.key_scope.clone(),
            self.runtime.deps.security.encryption_state.clone(),
            self.runtime.deps.security.encryption_session.clone(),
        )
    }

    /// Get the VerifyKeychainAccess use case.
    pub fn verify_keychain_access(
        &self,
    ) -> crate::usecases::verify_keychain_access::VerifyKeychainAccess {
        crate::usecases::verify_keychain_access::VerifyKeychainAccess::from_ports(
            self.runtime.deps.security.key_scope.clone(),
            self.runtime.deps.security.key_material.clone(),
        )
    }

    /// Get the AutoUnlockEncryptionSession use case.
    pub fn auto_unlock_encryption_session(&self) -> crate::usecases::AutoUnlockEncryptionSession {
        crate::usecases::AutoUnlockEncryptionSession::from_ports(
            self.runtime.deps.security.encryption_state.clone(),
            self.runtime.deps.security.key_scope.clone(),
            self.runtime.deps.security.key_material.clone(),
            self.runtime.deps.security.encryption.clone(),
            self.runtime.deps.security.encryption_session.clone(),
        )
    }

    /// Get the SetupOrchestrator.
    pub fn setup_orchestrator(&self) -> Arc<crate::usecases::SetupOrchestrator> {
        self.runtime.setup_orchestrator().clone()
    }

    /// Get application settings.
    pub fn get_settings(&self) -> crate::usecases::GetSettings {
        crate::usecases::GetSettings::new(self.runtime.deps.settings.clone())
    }

    /// Update application settings.
    pub fn update_settings(&self) -> crate::usecases::UpdateSettings {
        crate::usecases::UpdateSettings::new(self.runtime.deps.settings.clone())
    }

    /// Start the network runtime.
    pub fn start_network(&self) -> crate::usecases::StartNetwork {
        crate::usecases::StartNetwork::from_port(self.runtime.deps.network_control.clone())
    }

    /// Start the network runtime after unlock.
    pub fn start_network_after_unlock(&self) -> crate::usecases::StartNetworkAfterUnlock {
        crate::usecases::StartNetworkAfterUnlock::from_port(
            self.runtime.deps.network_control.clone(),
        )
    }

    /// List clipboard entry projections.
    pub fn list_entry_projections(&self) -> crate::usecases::ListClipboardEntryProjections {
        crate::usecases::ListClipboardEntryProjections::new(
            self.runtime.deps.clipboard.clipboard_entry_repo.clone(),
            self.runtime.deps.clipboard.selection_repo.clone(),
            self.runtime.deps.clipboard.representation_repo.clone(),
            self.runtime.deps.storage.thumbnail_repo.clone(),
            self.runtime.deps.storage.file_transfer_repo.clone(),
        )
    }

    /// Restore clipboard selection to system clipboard.
    pub fn restore_clipboard_selection(
        &self,
    ) -> anyhow::Result<
        crate::usecases::clipboard::restore_clipboard_selection::RestoreClipboardSelectionUseCase,
    > {
        let coordinator = self
            .runtime
            .clipboard_write_coordinator()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("clipboard_write_coordinator not available — clipboard writes require daemon/GUI runtime"))?;
        Ok(crate::usecases::clipboard::restore_clipboard_selection::RestoreClipboardSelectionUseCase::new(
            self.runtime.deps.clipboard.clipboard_entry_repo.clone(),
            coordinator,
            self.runtime.deps.clipboard.selection_repo.clone(),
            self.runtime.deps.clipboard.representation_repo.clone(),
            self.runtime.deps.storage.blob_store.clone(),
            self.runtime.clipboard_integration_mode,
        ))
    }

    /// Touch clipboard entry active time.
    pub fn touch_clipboard_entry(
        &self,
    ) -> crate::usecases::clipboard::touch_clipboard_entry::TouchClipboardEntryUseCase {
        crate::usecases::clipboard::touch_clipboard_entry::TouchClipboardEntryUseCase::new(
            self.runtime.deps.clipboard.clipboard_entry_repo.clone(),
            self.runtime.deps.system.clock.clone(),
        )
    }

    /// Toggle favorite state for a clipboard entry.
    pub fn toggle_favorite_clipboard_entry(
        &self,
    ) -> crate::usecases::clipboard::toggle_favorite_clipboard_entry::ToggleFavoriteClipboardEntryUseCase{
        crate::usecases::clipboard::toggle_favorite_clipboard_entry::ToggleFavoriteClipboardEntryUseCase::new(
            self.runtime.deps.clipboard.clipboard_entry_repo.clone(),
        )
    }

    /// Get the lifecycle status port directly (for status queries).
    pub fn get_lifecycle_status(&self) -> Arc<dyn crate::usecases::LifecycleStatusPort> {
        self.runtime.lifecycle_status().clone()
    }

    /// Create a `TrackInboundTransfersUseCase`.
    pub fn track_inbound_transfers(
        &self,
    ) -> crate::usecases::file_sync::TrackInboundTransfersUseCase {
        crate::usecases::file_sync::TrackInboundTransfersUseCase::new(
            self.runtime.deps.storage.file_transfer_repo.clone(),
        )
    }

    /// Create a `SyncOutboundFileUseCase`.
    pub fn sync_outbound_file(&self) -> crate::usecases::file_sync::SyncOutboundFileUseCase {
        crate::usecases::file_sync::SyncOutboundFileUseCase::new(
            self.runtime.deps.settings.clone(),
            self.runtime.deps.device.paired_device_repo.clone(),
            self.runtime.deps.network_ports.peers.clone(),
            self.runtime.deps.network_ports.file_transfer.clone(),
        )
    }

    /// Create a `SyncInboundFileUseCase`.
    pub fn sync_inbound_file(&self) -> crate::usecases::file_sync::SyncInboundFileUseCase {
        let file_cache_dir = self.runtime.storage_paths.file_cache_dir.clone();
        crate::usecases::file_sync::SyncInboundFileUseCase::new(
            self.runtime.deps.settings.clone(),
            file_cache_dir,
        )
    }

    /// Create a `CopyFileToClipboardUseCase`.
    pub fn copy_file_to_clipboard(
        &self,
    ) -> anyhow::Result<crate::usecases::file_sync::CopyFileToClipboardUseCase> {
        let coordinator = self
            .runtime
            .clipboard_write_coordinator()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("clipboard_write_coordinator not available — clipboard writes require daemon/GUI runtime"))?;
        Ok(crate::usecases::file_sync::CopyFileToClipboardUseCase::new(
            self.runtime.deps.clipboard.clipboard_entry_repo.clone(),
            self.runtime.deps.clipboard.representation_repo.clone(),
            coordinator,
            self.runtime.clipboard_integration_mode,
        ))
    }

    /// Get the `ClipboardWriteCoordinator` (clipboard write boundary).
    ///
    /// Returns `None` for CLI-only runtimes that do not perform clipboard writes.
    pub fn clipboard_write_coordinator(
        &self,
    ) -> Option<Arc<crate::usecases::ClipboardWriteCoordinator>> {
        self.runtime.clipboard_write_coordinator().cloned()
    }

    /// Create a `CleanupExpiredFilesUseCase`.
    pub fn cleanup_expired_files(&self) -> crate::usecases::file_sync::CleanupExpiredFilesUseCase {
        let file_cache_dir = self.runtime.storage_paths.file_cache_dir.clone();
        crate::usecases::file_sync::CleanupExpiredFilesUseCase::new(
            self.runtime.deps.settings.clone(),
            file_cache_dir,
        )
    }
}
