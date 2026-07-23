use std::fmt;

use serde::{Deserialize, Serialize};

use super::{HostFileHandle, SecretString};
use crate::{
    AbortMobileFileUploadInput, AppendMobileFileUploadInput, ApplyMobileSyncDocumentInput,
    AuthenticateMobileRequestInput, BeginMobileFileUploadInput, ExportConfigInput,
    ExportDiagnosticLogsInput, FinishMobileFileUploadInput, MobileContentAvailabilityInput,
    MobileDeviceInput, MobileLanEndpointUpdate, MobileSyncSettingsPatch, PreviewConfigImportInput,
    ReadMobileSyncFileInput, RegisterMobileDeviceInput, RelayProbeInput,
    RevalidateMobileCredentialInput, SettingsPatch, StageConfigImportInput, UpdateDebugModeInput,
    UpdateMobileDeviceInput,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    CreateSpace,
    JoinSpace,
    UnlockSpace,
    RecoverSession,
    IssueInvitation,
    CancelInvitation,
    ResetSpace,
    FactoryResetSpace,
    QuerySetupState,
    QueryMigrationProgress,
    QueryStorageStats,
    ClearStorageCache,
    QueryLocalDevice,
    QueryPeerConnections,
    RefreshPeerConnections,
    QuerySettings,
    UpdateSettings,
    ProbeRelay,
    QueryUpgradeStatus,
    AcknowledgeUpgrade,
    QueryDiagnostics,
    UpdateDebugMode,
    ExportDiagnosticLogs,
    ExportConfig,
    PreviewConfigImport,
    StageConfigImport,
    ListMobileDevices,
    RevokeMobileDevice,
    AuthenticateMobileRequest,
    RevalidateMobileCredential,
    ListMobileLanInterfaces,
    QueryMobileSyncSettings,
    UpdateMobileSyncSettings,
    UpdateMobileLanEndpoint,
    RegisterMobileDevice,
    UpdateMobileDevice,
    CheckMobileContentAvailable,
    QueryLatestMobileSyncDocument,
    ApplyMobileSyncDocument,
    ReadMobileSyncFile,
    BeginMobileFileUpload,
    AppendMobileFileUpload,
    FinishMobileFileUpload,
    AbortMobileFileUpload,
    QueryReceiveReadiness,
    QueryEncryptionState,
    LockEncryption,
    VerifySecureStorageAccess,
    ListDevices,
    QueryMemberSyncPreferences,
    UpdateMemberSyncPreferences,
    RemoveMember,
    SearchEntries,
    QuerySearchTags,
    QuerySearchStatus,
    RebuildSearchIndex,
    SendText,
    SendImage,
    SendFiles,
    QueryHistory,
    ListHistoryEntries,
    GetHistoryEntry,
    DeleteHistoryEntry,
    SetHistoryEntryFavorite,
    QueryHistoryStats,
    GetHistoryEntryResource,
    ReadBlob,
    ReadThumbnail,
    ReadEntryFile,
    QueryEntryDelivery,
    ClearHistory,
    QueryEntryReceiveProgress,
    ListEntryReceiveProgress,
    CancelEntryReceive,
    CancelInboundTransfer,
    CaptureCurrentClipboard,
    RestoreClipboard,
    ExportEntry,
    ResendEntry,
}

impl fmt::Display for OperationKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::CreateSpace => "create_space",
            Self::JoinSpace => "join_space",
            Self::UnlockSpace => "unlock_space",
            Self::RecoverSession => "recover_session",
            Self::IssueInvitation => "issue_invitation",
            Self::CancelInvitation => "cancel_invitation",
            Self::ResetSpace => "reset_space",
            Self::FactoryResetSpace => "factory_reset_space",
            Self::QuerySetupState => "query_setup_state",
            Self::QueryMigrationProgress => "query_migration_progress",
            Self::QueryStorageStats => "query_storage_stats",
            Self::ClearStorageCache => "clear_storage_cache",
            Self::QueryLocalDevice => "query_local_device",
            Self::QueryPeerConnections => "query_peer_connections",
            Self::RefreshPeerConnections => "refresh_peer_connections",
            Self::QuerySettings => "query_settings",
            Self::UpdateSettings => "update_settings",
            Self::ProbeRelay => "probe_relay",
            Self::QueryUpgradeStatus => "query_upgrade_status",
            Self::AcknowledgeUpgrade => "acknowledge_upgrade",
            Self::QueryDiagnostics => "query_diagnostics",
            Self::UpdateDebugMode => "update_debug_mode",
            Self::ExportDiagnosticLogs => "export_diagnostic_logs",
            Self::ExportConfig => "export_config",
            Self::PreviewConfigImport => "preview_config_import",
            Self::StageConfigImport => "stage_config_import",
            Self::ListMobileDevices => "list_mobile_devices",
            Self::RevokeMobileDevice => "revoke_mobile_device",
            Self::AuthenticateMobileRequest => "authenticate_mobile_request",
            Self::RevalidateMobileCredential => "revalidate_mobile_credential",
            Self::ListMobileLanInterfaces => "list_mobile_lan_interfaces",
            Self::QueryMobileSyncSettings => "query_mobile_sync_settings",
            Self::UpdateMobileSyncSettings => "update_mobile_sync_settings",
            Self::UpdateMobileLanEndpoint => "update_mobile_lan_endpoint",
            Self::RegisterMobileDevice => "register_mobile_device",
            Self::UpdateMobileDevice => "update_mobile_device",
            Self::CheckMobileContentAvailable => "check_mobile_content_available",
            Self::QueryLatestMobileSyncDocument => "query_latest_mobile_sync_document",
            Self::ApplyMobileSyncDocument => "apply_mobile_sync_document",
            Self::ReadMobileSyncFile => "read_mobile_sync_file",
            Self::BeginMobileFileUpload => "begin_mobile_file_upload",
            Self::AppendMobileFileUpload => "append_mobile_file_upload",
            Self::FinishMobileFileUpload => "finish_mobile_file_upload",
            Self::AbortMobileFileUpload => "abort_mobile_file_upload",
            Self::QueryReceiveReadiness => "query_receive_readiness",
            Self::QueryEncryptionState => "query_encryption_state",
            Self::LockEncryption => "lock_encryption",
            Self::VerifySecureStorageAccess => "verify_secure_storage_access",
            Self::ListDevices => "list_devices",
            Self::QueryMemberSyncPreferences => "query_member_sync_preferences",
            Self::UpdateMemberSyncPreferences => "update_member_sync_preferences",
            Self::RemoveMember => "remove_member",
            Self::SearchEntries => "search_entries",
            Self::QuerySearchTags => "query_search_tags",
            Self::QuerySearchStatus => "query_search_status",
            Self::RebuildSearchIndex => "rebuild_search_index",
            Self::SendText => "send_text",
            Self::SendImage => "send_image",
            Self::SendFiles => "send_files",
            Self::QueryHistory => "query_history",
            Self::ListHistoryEntries => "list_history_entries",
            Self::GetHistoryEntry => "get_history_entry",
            Self::DeleteHistoryEntry => "delete_history_entry",
            Self::SetHistoryEntryFavorite => "set_history_entry_favorite",
            Self::QueryHistoryStats => "query_history_stats",
            Self::GetHistoryEntryResource => "get_history_entry_resource",
            Self::ReadBlob => "read_blob",
            Self::ReadThumbnail => "read_thumbnail",
            Self::ReadEntryFile => "read_entry_file",
            Self::QueryEntryDelivery => "query_entry_delivery",
            Self::ClearHistory => "clear_history",
            Self::QueryEntryReceiveProgress => "query_entry_receive_progress",
            Self::ListEntryReceiveProgress => "list_entry_receive_progress",
            Self::CancelEntryReceive => "cancel_entry_receive",
            Self::CancelInboundTransfer => "cancel_inbound_transfer",
            Self::CaptureCurrentClipboard => "capture_current_clipboard",
            Self::RestoreClipboard => "restore_clipboard",
            Self::ExportEntry => "export_entry",
            Self::ResendEntry => "resend_entry",
        };
        formatter.write_str(value)
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum Operation {
    CreateSpace(CreateSpaceInput),
    JoinSpace(JoinSpaceInput),
    UnlockSpace(UnlockSpaceInput),
    RecoverSession(RecoverSessionInput),
    IssueInvitation,
    CancelInvitation,
    ResetSpace,
    FactoryResetSpace,
    QuerySetupState,
    QueryMigrationProgress,
    QueryStorageStats,
    ClearStorageCache,
    QueryLocalDevice,
    QueryPeerConnections,
    RefreshPeerConnections,
    QuerySettings,
    UpdateSettings(Box<SettingsPatch>),
    ProbeRelay(RelayProbeInput),
    QueryUpgradeStatus,
    AcknowledgeUpgrade,
    QueryDiagnostics,
    UpdateDebugMode(UpdateDebugModeInput),
    ExportDiagnosticLogs(ExportDiagnosticLogsInput),
    ExportConfig(ExportConfigInput),
    PreviewConfigImport(PreviewConfigImportInput),
    StageConfigImport(StageConfigImportInput),
    ListMobileDevices,
    RevokeMobileDevice(MobileDeviceInput),
    AuthenticateMobileRequest(AuthenticateMobileRequestInput),
    RevalidateMobileCredential(RevalidateMobileCredentialInput),
    ListMobileLanInterfaces,
    QueryMobileSyncSettings,
    UpdateMobileSyncSettings(Box<MobileSyncSettingsPatch>),
    UpdateMobileLanEndpoint(MobileLanEndpointUpdate),
    RegisterMobileDevice(RegisterMobileDeviceInput),
    UpdateMobileDevice(UpdateMobileDeviceInput),
    CheckMobileContentAvailable(MobileContentAvailabilityInput),
    QueryLatestMobileSyncDocument,
    ApplyMobileSyncDocument(Box<ApplyMobileSyncDocumentInput>),
    ReadMobileSyncFile(ReadMobileSyncFileInput),
    BeginMobileFileUpload(BeginMobileFileUploadInput),
    AppendMobileFileUpload(AppendMobileFileUploadInput),
    FinishMobileFileUpload(FinishMobileFileUploadInput),
    AbortMobileFileUpload(AbortMobileFileUploadInput),
    QueryReceiveReadiness,
    QueryEncryptionState,
    LockEncryption,
    VerifySecureStorageAccess,
    ListDevices,
    QueryMemberSyncPreferences(QueryMemberSyncPreferencesInput),
    UpdateMemberSyncPreferences(UpdateMemberSyncPreferencesInput),
    RemoveMember(RemoveMemberInput),
    SearchEntries(SearchEntriesInput),
    QuerySearchTags,
    QuerySearchStatus,
    RebuildSearchIndex,
    SendText(SendTextInput),
    SendImage(SendImageInput),
    SendFiles(SendFilesInput),
    QueryHistory(QueryHistoryInput),
    ListHistoryEntries(ListHistoryEntriesInput),
    GetHistoryEntry(HistoryEntryInput),
    DeleteHistoryEntry(HistoryEntryInput),
    SetHistoryEntryFavorite(SetHistoryEntryFavoriteInput),
    QueryHistoryStats,
    GetHistoryEntryResource(HistoryEntryInput),
    ReadBlob(BlobResourceInput),
    ReadThumbnail(ThumbnailResourceInput),
    ReadEntryFile(HistoryEntryInput),
    QueryEntryDelivery(HistoryEntryInput),
    ClearHistory,
    QueryEntryReceiveProgress(EntryReceiveProgressInput),
    ListEntryReceiveProgress,
    CancelEntryReceive(CancelEntryReceiveInput),
    CancelInboundTransfer(CancelInboundTransferInput),
    CaptureCurrentClipboard,
    RestoreClipboard(RestoreClipboardInput),
    ExportEntry(ExportEntryInput),
    ResendEntry(ResendEntryInput),
}

impl Operation {
    pub fn kind(&self) -> OperationKind {
        match self {
            Self::CreateSpace(_) => OperationKind::CreateSpace,
            Self::JoinSpace(_) => OperationKind::JoinSpace,
            Self::UnlockSpace(_) => OperationKind::UnlockSpace,
            Self::RecoverSession(_) => OperationKind::RecoverSession,
            Self::IssueInvitation => OperationKind::IssueInvitation,
            Self::CancelInvitation => OperationKind::CancelInvitation,
            Self::ResetSpace => OperationKind::ResetSpace,
            Self::FactoryResetSpace => OperationKind::FactoryResetSpace,
            Self::QuerySetupState => OperationKind::QuerySetupState,
            Self::QueryMigrationProgress => OperationKind::QueryMigrationProgress,
            Self::QueryStorageStats => OperationKind::QueryStorageStats,
            Self::ClearStorageCache => OperationKind::ClearStorageCache,
            Self::QueryLocalDevice => OperationKind::QueryLocalDevice,
            Self::QueryPeerConnections => OperationKind::QueryPeerConnections,
            Self::RefreshPeerConnections => OperationKind::RefreshPeerConnections,
            Self::QuerySettings => OperationKind::QuerySettings,
            Self::UpdateSettings(_) => OperationKind::UpdateSettings,
            Self::ProbeRelay(_) => OperationKind::ProbeRelay,
            Self::QueryUpgradeStatus => OperationKind::QueryUpgradeStatus,
            Self::AcknowledgeUpgrade => OperationKind::AcknowledgeUpgrade,
            Self::QueryDiagnostics => OperationKind::QueryDiagnostics,
            Self::UpdateDebugMode(_) => OperationKind::UpdateDebugMode,
            Self::ExportDiagnosticLogs(_) => OperationKind::ExportDiagnosticLogs,
            Self::ExportConfig(_) => OperationKind::ExportConfig,
            Self::PreviewConfigImport(_) => OperationKind::PreviewConfigImport,
            Self::StageConfigImport(_) => OperationKind::StageConfigImport,
            Self::ListMobileDevices => OperationKind::ListMobileDevices,
            Self::RevokeMobileDevice(_) => OperationKind::RevokeMobileDevice,
            Self::AuthenticateMobileRequest(_) => OperationKind::AuthenticateMobileRequest,
            Self::RevalidateMobileCredential(_) => OperationKind::RevalidateMobileCredential,
            Self::ListMobileLanInterfaces => OperationKind::ListMobileLanInterfaces,
            Self::QueryMobileSyncSettings => OperationKind::QueryMobileSyncSettings,
            Self::UpdateMobileSyncSettings(_) => OperationKind::UpdateMobileSyncSettings,
            Self::UpdateMobileLanEndpoint(_) => OperationKind::UpdateMobileLanEndpoint,
            Self::RegisterMobileDevice(_) => OperationKind::RegisterMobileDevice,
            Self::UpdateMobileDevice(_) => OperationKind::UpdateMobileDevice,
            Self::CheckMobileContentAvailable(_) => OperationKind::CheckMobileContentAvailable,
            Self::QueryLatestMobileSyncDocument => OperationKind::QueryLatestMobileSyncDocument,
            Self::ApplyMobileSyncDocument(_) => OperationKind::ApplyMobileSyncDocument,
            Self::ReadMobileSyncFile(_) => OperationKind::ReadMobileSyncFile,
            Self::BeginMobileFileUpload(_) => OperationKind::BeginMobileFileUpload,
            Self::AppendMobileFileUpload(_) => OperationKind::AppendMobileFileUpload,
            Self::FinishMobileFileUpload(_) => OperationKind::FinishMobileFileUpload,
            Self::AbortMobileFileUpload(_) => OperationKind::AbortMobileFileUpload,
            Self::QueryReceiveReadiness => OperationKind::QueryReceiveReadiness,
            Self::QueryEncryptionState => OperationKind::QueryEncryptionState,
            Self::LockEncryption => OperationKind::LockEncryption,
            Self::VerifySecureStorageAccess => OperationKind::VerifySecureStorageAccess,
            Self::ListDevices => OperationKind::ListDevices,
            Self::QueryMemberSyncPreferences(_) => OperationKind::QueryMemberSyncPreferences,
            Self::UpdateMemberSyncPreferences(_) => OperationKind::UpdateMemberSyncPreferences,
            Self::RemoveMember(_) => OperationKind::RemoveMember,
            Self::SearchEntries(_) => OperationKind::SearchEntries,
            Self::QuerySearchTags => OperationKind::QuerySearchTags,
            Self::QuerySearchStatus => OperationKind::QuerySearchStatus,
            Self::RebuildSearchIndex => OperationKind::RebuildSearchIndex,
            Self::SendText(_) => OperationKind::SendText,
            Self::SendImage(_) => OperationKind::SendImage,
            Self::SendFiles(_) => OperationKind::SendFiles,
            Self::QueryHistory(_) => OperationKind::QueryHistory,
            Self::ListHistoryEntries(_) => OperationKind::ListHistoryEntries,
            Self::GetHistoryEntry(_) => OperationKind::GetHistoryEntry,
            Self::DeleteHistoryEntry(_) => OperationKind::DeleteHistoryEntry,
            Self::SetHistoryEntryFavorite(_) => OperationKind::SetHistoryEntryFavorite,
            Self::QueryHistoryStats => OperationKind::QueryHistoryStats,
            Self::GetHistoryEntryResource(_) => OperationKind::GetHistoryEntryResource,
            Self::ReadBlob(_) => OperationKind::ReadBlob,
            Self::ReadThumbnail(_) => OperationKind::ReadThumbnail,
            Self::ReadEntryFile(_) => OperationKind::ReadEntryFile,
            Self::QueryEntryDelivery(_) => OperationKind::QueryEntryDelivery,
            Self::ClearHistory => OperationKind::ClearHistory,
            Self::QueryEntryReceiveProgress(_) => OperationKind::QueryEntryReceiveProgress,
            Self::ListEntryReceiveProgress => OperationKind::ListEntryReceiveProgress,
            Self::CancelEntryReceive(_) => OperationKind::CancelEntryReceive,
            Self::CancelInboundTransfer(_) => OperationKind::CancelInboundTransfer,
            Self::CaptureCurrentClipboard => OperationKind::CaptureCurrentClipboard,
            Self::RestoreClipboard(_) => OperationKind::RestoreClipboard,
            Self::ExportEntry(_) => OperationKind::ExportEntry,
            Self::ResendEntry(_) => OperationKind::ResendEntry,
        }
    }
}

impl fmt::Debug for Operation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Operation")
            .field("kind", &self.kind().to_string())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CreateSpaceInput {
    pub device_name: Option<String>,
    pub passphrase: SecretString,
    pub passphrase_confirmation: SecretString,
}

impl fmt::Debug for CreateSpaceInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CreateSpaceInput")
            .field("device_name", &"[REDACTED]")
            .field("passphrase", &"[REDACTED]")
            .field("passphrase_confirmation", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct JoinSpaceInput {
    pub invitation_code: String,
    pub device_name: Option<String>,
    pub passphrase: SecretString,
}

impl fmt::Debug for JoinSpaceInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JoinSpaceInput")
            .field("invitation_code", &"[REDACTED]")
            .field("device_name", &"[REDACTED]")
            .field("passphrase", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct UnlockSpaceInput {
    pub passphrase: SecretString,
}

impl fmt::Debug for UnlockSpaceInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UnlockSpaceInput")
            .field("passphrase", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoverSessionInput {
    pub allow_secure_storage_unlock: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryMemberSyncPreferencesInput {
    pub device_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateMemberSyncPreferencesInput {
    pub device_id: String,
    pub patch: MemberSyncPreferencesPatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveMemberInput {
    pub device_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberSyncPreferencesPatch {
    pub send_enabled: Option<bool>,
    pub receive_enabled: Option<bool>,
    pub send_content_types: Option<ContentTypesPatch>,
    pub receive_content_types: Option<ContentTypesPatch>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentTypesPatch {
    pub text: Option<bool>,
    pub image: Option<bool>,
    pub link: Option<bool>,
    pub file: Option<bool>,
    pub code_snippet: Option<bool>,
    pub rich_text: Option<bool>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SearchEntriesInput {
    pub query: String,
    pub operator: Option<String>,
    pub time_preset: Option<String>,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
    pub content_types: Option<String>,
    pub extensions: Option<String>,
    pub source_devices: Option<String>,
    pub tags: Option<String>,
    pub limit: u32,
    pub offset: u32,
}

impl fmt::Debug for SearchEntriesInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchEntriesInput")
            .field("has_query", &!self.query.trim().is_empty())
            .field("has_operator", &self.operator.is_some())
            .field("has_time_preset", &self.time_preset.is_some())
            .field(
                "has_time_range",
                &(self.from_ms.is_some() || self.to_ms.is_some()),
            )
            .field("has_content_types", &self.content_types.is_some())
            .field("has_extensions", &self.extensions.is_some())
            .field("has_source_devices", &self.source_devices.is_some())
            .field("has_tags", &self.tags.is_some())
            .field("limit", &self.limit)
            .field("offset", &self.offset)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SendTextInput {
    pub text: String,
    pub target_devices: Vec<String>,
}

impl fmt::Debug for SendTextInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SendTextInput")
            .field("text", &"[REDACTED]")
            .field("target_count", &self.target_devices.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct SendImageInput {
    pub bytes: Vec<u8>,
    pub mime_type: String,
    pub target_devices: Vec<String>,
}

impl fmt::Debug for SendImageInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SendImageInput")
            .field("byte_len", &self.bytes.len())
            .field("mime_type", &self.mime_type)
            .field("target_count", &self.target_devices.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SendTargetOutcome {
    Accepted,
    Duplicate,
    Error { message: String },
}

impl fmt::Debug for SendTargetOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accepted => formatter.write_str("Accepted"),
            Self::Duplicate => formatter.write_str("Duplicate"),
            Self::Error { .. } => formatter
                .debug_struct("Error")
                .field("message", &"[REDACTED]")
                .finish(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendTargetSummary {
    pub device_id: String,
    pub outcome: SendTargetOutcome,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendReportSummary {
    pub entry_id: String,
    pub snapshot_hash: String,
    pub at_ms: i64,
    pub total_accepted: usize,
    pub total_duplicate: usize,
    pub total_offline: usize,
    pub total_errored: usize,
    pub total_pending: usize,
    pub per_target: Vec<SendTargetSummary>,
}

impl fmt::Debug for SendReportSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SendReportSummary")
            .field("total_accepted", &self.total_accepted)
            .field("total_duplicate", &self.total_duplicate)
            .field("total_offline", &self.total_offline)
            .field("total_errored", &self.total_errored)
            .field("total_pending", &self.total_pending)
            .field("target_count", &self.per_target.len())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendFilesInput {
    pub files: Vec<HostFileHandle>,
    pub target_devices: Vec<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct QueryHistoryInput {
    pub cursor: Option<String>,
    pub limit: u32,
    pub query: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListHistoryEntriesInput {
    pub limit: u32,
    pub offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntryInput {
    pub entry_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobResourceInput {
    pub blob_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailResourceInput {
    pub representation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetHistoryEntryFavoriteInput {
    pub entry_id: String,
    pub is_favorited: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryReceiveProgressInput {
    pub entry_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelEntryReceiveInput {
    pub entry_id: String,
    pub attempt_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancelInboundTransferInput {
    pub transfer_id: String,
    pub reason: TransferCancellationReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreClipboardInput {
    pub entry_id: String,
    pub mode: ClipboardRestoreMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardRestoreMode {
    Standard,
    PlainText,
    FilePaths,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferCancellationReason {
    LocalUser,
    RemotePeer,
    Replaced,
    Timeout,
    Unknown,
}

impl fmt::Debug for QueryHistoryInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QueryHistoryInput")
            .field("has_cursor", &self.cursor.is_some())
            .field("limit", &self.limit)
            .field("has_query", &self.query.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportEntryInput {
    pub entry_id: String,
    pub destination: HostFileHandle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResendEntryInput {
    pub entry_id: String,
    pub target_devices: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResendReportSummary {
    pub accepted: usize,
    pub duplicate: usize,
    pub offline: usize,
    pub errored: usize,
    pub pending: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ResendEntryOutcome {
    Completed(ResendReportSummary),
    EntryNotFound {
        entry_id: String,
    },
    EntryNotResendable {
        entry_id: String,
        reason: EntryNotResendableReason,
    },
    TargetNotTrusted {
        device_id: String,
    },
    NoEligibleTargets,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryNotResendableReason {
    RemoteOrigin,
    PayloadLost,
}
