use std::fmt;

use serde::{Deserialize, Serialize};

use super::{EngineError, ResendEntryOutcome, SendReportSummary};
use crate::{
    ConfigExportOutcome, ConfigImportPreviewOutcome, ConfigImportStageOutcome,
    DebugModeUpdateSummary, DiagnosticLogsExportSummary, DiagnosticsStatusSummary,
    MobileAuthenticatedSession, MobileAuthenticationOutcome, MobileDeviceRegistrationOutcome,
    MobileDeviceRevokeOutcome, MobileDeviceSummary, MobileDeviceUpdateOutcome,
    MobileFileUploadHandle, MobileLanInterfaceSummary, MobileSyncDocument,
    MobileSyncDocumentApplyOutcome, MobileSyncFileReadOutcome, MobileSyncSettingsSummary,
    MobileSyncSettingsUpdateOutcome, RelayProbeOutcome, SettingsSummary, SettingsUpdateOutcome,
    UpgradeStatusSummary,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationTerminal {
    Succeeded,
    Failed(EngineError),
    Cancelled,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntrySummary {
    pub entry_id: String,
    pub content_type: String,
    pub preview: Option<String>,
    pub created_at_ms: i64,
}

impl fmt::Debug for EntrySummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntrySummary")
            .field("entry_id", &self.entry_id)
            .field("content_type", &self.content_type)
            .field("has_preview", &self.preview.is_some())
            .field("created_at_ms", &self.created_at_ms)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntrySummary {
    pub entry_id: String,
    pub preview: String,
    pub has_detail: bool,
    pub size_bytes: i64,
    pub captured_at_ms: i64,
    pub content_type: String,
    pub thumbnail_url: Option<String>,
    pub is_encrypted: bool,
    pub is_favorited: bool,
    pub updated_at_ms: i64,
    pub active_time_ms: i64,
    pub file_transfer_status: Option<String>,
    pub file_transfer_reason: Option<String>,
    pub content_tags: Vec<String>,
    pub link_urls: Option<Vec<String>>,
    pub link_domains: Option<Vec<String>>,
    pub file_sizes: Option<Vec<i64>>,
    pub image_width: Option<i32>,
    pub image_height: Option<i32>,
    pub payload_state: Option<String>,
}

impl fmt::Debug for HistoryEntrySummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryEntrySummary")
            .field("entry_id", &self.entry_id)
            .field("has_preview", &!self.preview.is_empty())
            .field("has_detail", &self.has_detail)
            .field("size_bytes", &self.size_bytes)
            .field("content_type", &self.content_type)
            .field("has_thumbnail", &self.thumbnail_url.is_some())
            .field("is_encrypted", &self.is_encrypted)
            .field("is_favorited", &self.is_favorited)
            .field("tag_count", &self.content_tags.len())
            .field("link_count", &self.link_urls.as_ref().map_or(0, Vec::len))
            .field("has_payload_state", &self.payload_state.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntryDetailSummary {
    pub entry_id: String,
    pub content: String,
    pub size_bytes: i64,
    pub created_at_ms: i64,
    pub active_time_ms: i64,
    pub mime_type: Option<String>,
}

impl fmt::Debug for HistoryEntryDetailSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryEntryDetailSummary")
            .field("entry_id", &self.entry_id)
            .field("has_content", &!self.content.is_empty())
            .field("size_bytes", &self.size_bytes)
            .field("created_at_ms", &self.created_at_ms)
            .field("active_time_ms", &self.active_time_ms)
            .field("mime_type", &self.mime_type)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntryResourceSummary {
    pub blob_id: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub url: Option<String>,
    pub inline_data: Option<Vec<u8>>,
}

impl fmt::Debug for HistoryEntryResourceSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HistoryEntryResourceSummary")
            .field("has_blob", &self.blob_id.is_some())
            .field("mime_type", &self.mime_type)
            .field("size_bytes", &self.size_bytes)
            .field("has_url", &self.url.is_some())
            .field("inline_byte_len", &self.inline_data.as_ref().map(Vec::len))
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryStatsSummary {
    pub total_items: i64,
    pub total_size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryClearSummary {
    pub deleted_count: u64,
    pub failed_entry_ids: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryDeliveryViewSummary {
    pub entry_id: String,
    pub source: EntrySourceSummary,
    pub deliveries: Vec<EntryDeliveryTargetSummary>,
}

impl fmt::Debug for EntryDeliveryViewSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntryDeliveryViewSummary")
            .field("entry_id", &self.entry_id)
            .field("source", &self.source)
            .field("delivery_count", &self.deliveries.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EntrySourceSummary {
    Local,
    Remote {
        device_id: String,
        device_name: Option<String>,
    },
    Historical,
}

impl fmt::Debug for EntrySourceSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("EntrySourceSummary");
        match self {
            Self::Local => debug.field("kind", &"local"),
            Self::Remote {
                device_id,
                device_name,
            } => debug
                .field("kind", &"remote")
                .field("device_id", device_id)
                .field("has_device_name", &device_name.is_some()),
            Self::Historical => debug.field("kind", &"historical"),
        };
        debug.finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryDeliveryTargetSummary {
    pub target_device_id: String,
    pub target_device_name: Option<String>,
    pub status: EntryDeliveryStatusSummary,
    pub reason_detail: Option<String>,
    pub updated_at_ms: Option<i64>,
}

impl fmt::Debug for EntryDeliveryTargetSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntryDeliveryTargetSummary")
            .field("target_device_id", &self.target_device_id)
            .field("has_target_device_name", &self.target_device_name.is_some())
            .field("status", &self.status)
            .field("has_reason_detail", &self.reason_detail.is_some())
            .field("updated_at_ms", &self.updated_at_ms)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EntryDeliveryStatusSummary {
    Pending,
    Delivered,
    Duplicate,
    Unreachable,
    Failed {
        reason: DeliveryFailureReasonSummary,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryFailureReasonSummary {
    LocalPolicy,
    PeerRejected,
    Io,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiveProgressSummary {
    pub entry_id: String,
    pub attempt_id: String,
    pub state: String,
    pub total_bytes: i64,
    pub completed_bytes: i64,
    pub items_total: u32,
    pub items_completed: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiveReadinessSummary {
    pub ready: bool,
    pub degraded: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryReceiveCancellationOutcome {
    CancellationRequested,
    Cancelled,
    NotReceiving,
    TooLate,
    AlreadyTerminal,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboundTransferCancellationOutcome {
    Cancelled,
    NotInflight,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ClipboardRestoreOutcome {
    Restored,
    PayloadUnavailable {
        entry_id: String,
        representation_id: String,
        state: String,
    },
    NotApplicable {
        reason: String,
    },
}

impl fmt::Debug for ClipboardRestoreOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("ClipboardRestoreOutcome");
        match self {
            Self::Restored => debug.field("kind", &"restored"),
            Self::PayloadUnavailable { state, .. } => debug
                .field("kind", &"payload_unavailable")
                .field("state", state),
            Self::NotApplicable { .. } => debug.field("kind", &"not_applicable"),
        };
        debug.finish()
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinaryResourceSummary {
    pub bytes: Vec<u8>,
    pub media_type: Option<String>,
}

impl fmt::Debug for BinaryResourceSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BinaryResourceSummary")
            .field("byte_len", &self.bytes.len())
            .field("has_media_type", &self.media_type.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryFileResourceSummary {
    pub bytes: Vec<u8>,
    pub media_type: Option<String>,
    pub file_name: String,
}

impl fmt::Debug for EntryFileResourceSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EntryFileResourceSummary")
            .field("byte_len", &self.bytes.len())
            .field("has_media_type", &self.media_type.is_some())
            .field("has_file_name", &!self.file_name.is_empty())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum InvitationAvailability {
    CrossNetwork,
    SameLocalNetwork,
}

impl fmt::Debug for InvitationAvailability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CrossNetwork => "cross_network",
            Self::SameLocalNetwork => "same_local_network",
        })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum OperationResult {
    SpaceCreated {
        space_id: String,
        self_device_id: String,
        identity_fingerprint: String,
    },
    SpaceJoined {
        sponsor_device_id: String,
        sponsor_identity_fingerprint: String,
        space_id: String,
        self_device_id: String,
        self_identity_fingerprint: String,
        migrated_records: Option<u64>,
    },
    SpaceUnlocked {
        space_id: String,
    },
    SessionRecovered {
        unlocked: bool,
        resumed: bool,
    },
    InvitationIssued {
        invitation_code: String,
        expires_at_ms: i64,
        availability: InvitationAvailability,
    },
    InvitationCancelled,
    SpaceReset,
    SpaceFactoryReset,
    SetupState(SetupStateSummary),
    MigrationProgress(MigrationProgressSummary),
    StorageStats(StorageStatsSummary),
    StorageCacheCleared {
        freed_bytes: u64,
    },
    LocalDevice(LocalDeviceSummary),
    PeerConnections(Vec<PeerConnectionSummary>),
    PeerConnectionsRefreshed(PeerConnectionRefreshSummary),
    Settings(Box<SettingsSummary>),
    SettingsUpdated(SettingsUpdateOutcome),
    RelayProbed(RelayProbeOutcome),
    UpgradeStatus(UpgradeStatusSummary),
    UpgradeAcknowledged {
        version: String,
    },
    DiagnosticsStatus(DiagnosticsStatusSummary),
    DebugModeUpdated(DebugModeUpdateSummary),
    DiagnosticLogsExported(DiagnosticLogsExportSummary),
    ConfigExport(ConfigExportOutcome),
    ConfigImportPreview(ConfigImportPreviewOutcome),
    ConfigImportStaged(ConfigImportStageOutcome),
    MobileDevices(Vec<MobileDeviceSummary>),
    MobileDeviceRevoked(MobileDeviceRevokeOutcome),
    MobileRequestAuthenticated(MobileAuthenticatedSession),
    MobileAuthentication(MobileAuthenticationOutcome),
    MobileCredentialCurrent {
        current: bool,
    },
    MobileLanInterfaces(Vec<MobileLanInterfaceSummary>),
    MobileSyncSettings(Box<MobileSyncSettingsSummary>),
    MobileSyncSettingsUpdated(MobileSyncSettingsUpdateOutcome),
    MobileLanEndpointUpdated,
    MobileDeviceRegistered(MobileDeviceRegistrationOutcome),
    MobileDeviceUpdated(MobileDeviceUpdateOutcome),
    MobileContentAvailability {
        available: bool,
    },
    MobileSyncDocument(Option<Box<MobileSyncDocument>>),
    MobileSyncDocumentApplied(MobileSyncDocumentApplyOutcome),
    MobileSyncFile(MobileSyncFileReadOutcome),
    MobileFileUploadStarted(MobileFileUploadHandle),
    MobileFileUploadChunkAppended,
    MobileFileUploadFinished(MobileSyncDocumentApplyOutcome),
    MobileFileUploadAborted {
        existed: bool,
    },
    ReceiveReadiness(ReceiveReadinessSummary),
    EncryptionState(EncryptionStateSummary),
    EncryptionLocked,
    SecureStorageAccess {
        granted: bool,
    },
    Devices(Vec<DeviceSummary>),
    MemberSyncPreferences(MemberSyncPreferencesSummary),
    MemberRemoved,
    SearchPage(SearchPageSummary),
    SearchTags(Vec<SearchTagSummary>),
    SearchStatus(SearchStatusSummary),
    SearchRebuildAccepted {
        accepted: bool,
    },
    EntrySent(SendReportSummary),
    HistoryPage {
        entries: Vec<EntrySummary>,
        next_cursor: Option<String>,
    },
    HistoryEntries(Vec<HistoryEntrySummary>),
    HistoryEntry(HistoryEntryDetailSummary),
    HistoryEntryDeleted,
    HistoryEntryFavoriteSet,
    HistoryStats(HistoryStatsSummary),
    HistoryEntryResource(HistoryEntryResourceSummary),
    BlobRead(BinaryResourceSummary),
    ThumbnailRead(BinaryResourceSummary),
    EntryFileRead(EntryFileResourceSummary),
    EntryDelivery(EntryDeliveryViewSummary),
    HistoryCleared(HistoryClearSummary),
    EntryReceiveProgress(Option<ReceiveProgressSummary>),
    EntryReceiveProgressList(Vec<ReceiveProgressSummary>),
    EntryReceiveCancellation(EntryReceiveCancellationOutcome),
    InboundTransferCancellation(InboundTransferCancellationOutcome),
    ClipboardCaptured {
        entry_id: Option<String>,
    },
    ClipboardRestored(ClipboardRestoreOutcome),
    EntryExported,
    EntryResent(ResendEntryOutcome),
}

impl fmt::Debug for OperationResult {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("OperationResult");
        match self {
            Self::SpaceCreated { .. } => debug.field("kind", &"space_created"),
            Self::SpaceJoined { .. } => debug.field("kind", &"space_joined"),
            Self::SpaceUnlocked { .. } => debug.field("kind", &"space_unlocked"),
            Self::SessionRecovered { unlocked, resumed } => debug
                .field("kind", &"session_recovered")
                .field("unlocked", unlocked)
                .field("resumed", resumed),
            Self::InvitationIssued { .. } => debug.field("kind", &"invitation_issued"),
            Self::InvitationCancelled => debug.field("kind", &"invitation_cancelled"),
            Self::SpaceReset => debug.field("kind", &"space_reset"),
            Self::SpaceFactoryReset => debug.field("kind", &"space_factory_reset"),
            Self::SetupState(state) => debug.field("kind", &"setup_state").field("state", state),
            Self::MigrationProgress(progress) => debug
                .field("kind", &"migration_progress")
                .field("progress", progress),
            Self::StorageStats(stats) => {
                debug.field("kind", &"storage_stats").field("stats", stats)
            }
            Self::StorageCacheCleared { freed_bytes } => debug
                .field("kind", &"storage_cache_cleared")
                .field("freed_bytes", freed_bytes),
            Self::LocalDevice(device) => {
                debug.field("kind", &"local_device").field("device", device)
            }
            Self::PeerConnections(peers) => debug
                .field("kind", &"peer_connections")
                .field("peer_count", &peers.len()),
            Self::PeerConnectionsRefreshed(report) => debug
                .field("kind", &"peer_connections_refreshed")
                .field("report", report),
            Self::Settings(_) => debug.field("kind", &"settings"),
            Self::SettingsUpdated(outcome) => debug
                .field("kind", &"settings_updated")
                .field("outcome", outcome),
            Self::RelayProbed(outcome) => debug
                .field("kind", &"relay_probed")
                .field("outcome", outcome),
            Self::UpgradeStatus(status) => debug
                .field("kind", &"upgrade_status")
                .field("status", status),
            Self::UpgradeAcknowledged { version } => debug
                .field("kind", &"upgrade_acknowledged")
                .field("version", version),
            Self::DiagnosticsStatus(status) => debug
                .field("kind", &"diagnostics_status")
                .field("status", status),
            Self::DebugModeUpdated(result) => debug
                .field("kind", &"debug_mode_updated")
                .field("result", result),
            Self::DiagnosticLogsExported(result) => debug
                .field("kind", &"diagnostic_logs_exported")
                .field("result", result),
            Self::ConfigExport(outcome) => debug
                .field("kind", &"config_export")
                .field("outcome", outcome),
            Self::ConfigImportPreview(outcome) => debug
                .field("kind", &"config_import_preview")
                .field("outcome", outcome),
            Self::ConfigImportStaged(outcome) => debug
                .field("kind", &"config_import_staged")
                .field("outcome", outcome),
            Self::MobileDevices(devices) => debug
                .field("kind", &"mobile_devices")
                .field("device_count", &devices.len()),
            Self::MobileDeviceRevoked(outcome) => debug
                .field("kind", &"mobile_device_revoked")
                .field("outcome", outcome),
            Self::MobileRequestAuthenticated(session) => debug
                .field("kind", &"mobile_request_authenticated")
                .field("session", session),
            Self::MobileAuthentication(outcome) => debug
                .field("kind", &"mobile_authentication")
                .field("outcome", outcome),
            Self::MobileCredentialCurrent { current } => debug
                .field("kind", &"mobile_credential_current")
                .field("current", current),
            Self::MobileLanInterfaces(interfaces) => debug
                .field("kind", &"mobile_lan_interfaces")
                .field("interface_count", &interfaces.len()),
            Self::MobileSyncSettings(settings) => debug
                .field("kind", &"mobile_sync_settings")
                .field("settings", settings),
            Self::MobileSyncSettingsUpdated(outcome) => debug
                .field("kind", &"mobile_sync_settings_updated")
                .field("outcome", outcome),
            Self::MobileLanEndpointUpdated => debug.field("kind", &"mobile_lan_endpoint_updated"),
            Self::MobileDeviceRegistered(outcome) => debug
                .field("kind", &"mobile_device_registered")
                .field("outcome", outcome),
            Self::MobileDeviceUpdated(outcome) => debug
                .field("kind", &"mobile_device_updated")
                .field("outcome", outcome),
            Self::MobileContentAvailability { available } => debug
                .field("kind", &"mobile_content_availability")
                .field("available", available),
            Self::MobileSyncDocument(document) => debug
                .field("kind", &"mobile_sync_document")
                .field("has_document", &document.is_some()),
            Self::MobileSyncDocumentApplied(outcome) => debug
                .field("kind", &"mobile_sync_document_applied")
                .field("outcome", outcome),
            Self::MobileSyncFile(outcome) => debug
                .field("kind", &"mobile_sync_file")
                .field("outcome", outcome),
            Self::MobileFileUploadStarted(_) => debug.field("kind", &"mobile_file_upload_started"),
            Self::MobileFileUploadChunkAppended => {
                debug.field("kind", &"mobile_file_upload_chunk_appended")
            }
            Self::MobileFileUploadFinished(outcome) => debug
                .field("kind", &"mobile_file_upload_finished")
                .field("outcome", outcome),
            Self::MobileFileUploadAborted { existed } => debug
                .field("kind", &"mobile_file_upload_aborted")
                .field("existed", existed),
            Self::ReceiveReadiness(readiness) => debug
                .field("kind", &"receive_readiness")
                .field("readiness", readiness),
            Self::EncryptionState(state) => debug
                .field("kind", &"encryption_state")
                .field("state", state),
            Self::EncryptionLocked => debug.field("kind", &"encryption_locked"),
            Self::SecureStorageAccess { granted } => debug
                .field("kind", &"secure_storage_access")
                .field("granted", granted),
            Self::Devices(devices) => debug
                .field("kind", &"devices")
                .field("device_count", &devices.len()),
            Self::MemberSyncPreferences(preferences) => debug
                .field("kind", &"member_sync_preferences")
                .field("preferences", preferences),
            Self::MemberRemoved => debug.field("kind", &"member_removed"),
            Self::SearchPage(page) => debug.field("kind", &"search_page").field("page", page),
            Self::SearchTags(tags) => debug
                .field("kind", &"search_tags")
                .field("tag_count", &tags.len()),
            Self::SearchStatus(status) => debug
                .field("kind", &"search_status")
                .field("status", status),
            Self::SearchRebuildAccepted { accepted } => debug
                .field("kind", &"search_rebuild_accepted")
                .field("accepted", accepted),
            Self::EntrySent(report) => debug.field("kind", &"entry_sent").field("report", report),
            Self::HistoryPage {
                entries,
                next_cursor,
            } => debug
                .field("kind", &"history_page")
                .field("entry_count", &entries.len())
                .field("has_next_cursor", &next_cursor.is_some()),
            Self::HistoryEntries(entries) => debug
                .field("kind", &"history_entries")
                .field("entry_count", &entries.len()),
            Self::HistoryEntry(_) => debug.field("kind", &"history_entry"),
            Self::HistoryEntryDeleted => debug.field("kind", &"history_entry_deleted"),
            Self::HistoryEntryFavoriteSet => debug.field("kind", &"history_entry_favorite_set"),
            Self::HistoryStats(stats) => {
                debug.field("kind", &"history_stats").field("stats", stats)
            }
            Self::HistoryEntryResource(resource) => debug
                .field("kind", &"history_entry_resource")
                .field("has_blob", &resource.blob_id.is_some())
                .field("has_url", &resource.url.is_some())
                .field("has_inline_data", &resource.inline_data.is_some()),
            Self::BlobRead(resource) => debug
                .field("kind", &"blob_read")
                .field("byte_len", &resource.bytes.len()),
            Self::ThumbnailRead(resource) => debug
                .field("kind", &"thumbnail_read")
                .field("byte_len", &resource.bytes.len()),
            Self::EntryFileRead(resource) => debug
                .field("kind", &"entry_file_read")
                .field("byte_len", &resource.bytes.len())
                .field("has_file_name", &!resource.file_name.is_empty()),
            Self::EntryDelivery(view) => debug.field("kind", &"entry_delivery").field("view", view),
            Self::HistoryCleared(result) => debug
                .field("kind", &"history_cleared")
                .field("deleted_count", &result.deleted_count)
                .field("failed_count", &result.failed_entry_ids.len()),
            Self::EntryReceiveProgress(progress) => debug
                .field("kind", &"entry_receive_progress")
                .field("has_progress", &progress.is_some()),
            Self::EntryReceiveProgressList(progress) => debug
                .field("kind", &"entry_receive_progress_list")
                .field("progress_count", &progress.len()),
            Self::EntryReceiveCancellation(outcome) => debug
                .field("kind", &"entry_receive_cancellation")
                .field("outcome", outcome),
            Self::InboundTransferCancellation(outcome) => debug
                .field("kind", &"inbound_transfer_cancellation")
                .field("outcome", outcome),
            Self::ClipboardCaptured { entry_id } => debug
                .field("kind", &"clipboard_captured")
                .field("has_entry", &entry_id.is_some()),
            Self::ClipboardRestored(outcome) => debug
                .field("kind", &"clipboard_restored")
                .field("outcome", outcome),
            Self::EntryExported => debug.field("kind", &"entry_exported"),
            Self::EntryResent(outcome) => debug
                .field("kind", &"entry_resent")
                .field("outcome", outcome),
        };
        debug.finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SetupInvitationSummary {
    pub invitation_code: String,
    pub expires_at_ms: i64,
}

impl fmt::Debug for SetupInvitationSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SetupInvitationSummary")
            .field("invitation_code", &"[REDACTED]")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SetupStateSummary {
    pub has_completed: bool,
    pub current_invitation: Option<SetupInvitationSummary>,
    pub device_name: Option<String>,
}

impl fmt::Debug for SetupStateSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SetupStateSummary")
            .field("has_completed", &self.has_completed)
            .field("has_current_invitation", &self.current_invitation.is_some())
            .field("has_device_name", &self.device_name.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationPhaseSummary {
    Prepared,
    HandshakeDone,
    Swapped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationProgressSummary {
    pub phase: Option<MigrationPhaseSummary>,
    pub backup_record_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageStatsSummary {
    pub total_bytes: u64,
    pub database_bytes: u64,
    pub vault_bytes: u64,
    pub cache_bytes: u64,
    pub logs_bytes: u64,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalDeviceSummary {
    pub device_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerConnectionChannelSummary {
    Direct,
    Relay,
    Offline,
    Unknown,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerConnectionSummary {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub addresses: Vec<String>,
    pub is_paired: bool,
    pub connected: bool,
    pub pairing_state: String,
    pub channel: PeerConnectionChannelSummary,
    pub connection_address: Option<String>,
}

impl fmt::Debug for PeerConnectionSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PeerConnectionSummary")
            .field("has_device_name", &self.device_name.is_some())
            .field("address_count", &self.addresses.len())
            .field("is_paired", &self.is_paired)
            .field("connected", &self.connected)
            .field("has_pairing_state", &!self.pairing_state.is_empty())
            .field("channel", &self.channel)
            .field("has_connection_address", &self.connection_address.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PeerConnectionRefreshSummary {
    pub total: u32,
    pub online: u32,
    pub offline: u32,
    pub errors: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptionStateSummary {
    pub initialized: bool,
    pub session_ready: bool,
}

impl fmt::Debug for LocalDeviceSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalDeviceSummary")
            .field("device_id", &self.device_id)
            .field("has_display_name", &!self.display_name.is_empty())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSummary {
    pub device_id: String,
    pub display_name: String,
    pub online: bool,
}

impl fmt::Debug for DeviceSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeviceSummary")
            .field("device_id", &self.device_id)
            .field("has_display_name", &!self.display_name.is_empty())
            .field("online", &self.online)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberSyncPreferencesSummary {
    pub send_enabled: bool,
    pub receive_enabled: bool,
    pub send_content_types: ContentTypesSummary,
    pub receive_content_types: ContentTypesSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentTypesSummary {
    pub text: bool,
    pub image: bool,
    pub link: bool,
    pub file: bool,
    pub code_snippet: bool,
    pub rich_text: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPageSummary {
    pub total: u32,
    pub has_more: bool,
    pub items: Vec<SearchResultSummary>,
    pub state: String,
}

impl fmt::Debug for SearchPageSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchPageSummary")
            .field("total", &self.total)
            .field("has_more", &self.has_more)
            .field("item_count", &self.items.len())
            .field("state", &self.state)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResultSummary {
    pub entry_id: String,
    pub content_type: String,
    pub active_time_ms: i64,
    pub tags: Vec<String>,
    pub text_preview: Option<String>,
    pub char_count: Option<i64>,
    pub mime_type: String,
    pub file_extensions: Vec<String>,
    pub file_names: Vec<String>,
    pub file_paths: Vec<String>,
    pub link_urls: Vec<String>,
    pub source_device: Option<String>,
    pub payload_state: Option<String>,
}

impl fmt::Debug for SearchResultSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchResultSummary")
            .field("entry_id", &self.entry_id)
            .field("content_type", &self.content_type)
            .field("active_time_ms", &self.active_time_ms)
            .field("tag_count", &self.tags.len())
            .field("has_text_preview", &self.text_preview.is_some())
            .field("char_count", &self.char_count)
            .field("file_count", &self.file_names.len())
            .field("link_count", &self.link_urls.len())
            .field("has_source_device", &self.source_device.is_some())
            .field("has_payload_state", &self.payload_state.is_some())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchTagSummary {
    pub tag_id: String,
    pub count: u32,
    pub is_builtin: bool,
}

impl fmt::Debug for SearchTagSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchTagSummary")
            .field("count", &self.count)
            .field("is_builtin", &self.is_builtin)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchStatusSummary {
    pub state: String,
    pub reason: Option<String>,
    pub last_rebuild_started_at_ms: Option<i64>,
    pub last_rebuild_completed_at_ms: Option<i64>,
}

impl fmt::Debug for SearchStatusSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchStatusSummary")
            .field("state", &self.state)
            .field("has_reason", &self.reason.is_some())
            .field(
                "last_rebuild_started_at_ms",
                &self.last_rebuild_started_at_ms,
            )
            .field(
                "last_rebuild_completed_at_ms",
                &self.last_rebuild_completed_at_ms,
            )
            .finish()
    }
}
