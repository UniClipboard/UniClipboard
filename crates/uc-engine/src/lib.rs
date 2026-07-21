//! Stable cross-platform interface owned by the UniClipboard engine.

use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

mod config;
mod engine;
mod event_stream;
mod host;

#[doc(hidden)]
pub mod internal;

pub use config::EngineConfig;
pub use engine::Engine;
pub use event_stream::EventStream;
pub use host::{
    HostCapabilities, HostCapabilityError, HostCapabilityErrorCategory, HostClipboard,
    HostClipboardRepresentation, HostClipboardSnapshot, HostDirectories, HostFileAccess,
    HostFileMetadata, HostSecureStorage,
};

#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(Vec<u8>);

impl SecretString {
    pub fn new(value: impl AsRef<str>) -> Self {
        Self(value.as_ref().as_bytes().to_vec())
    }

    pub fn expose(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or_default()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretString([REDACTED])")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HostFileHandle(String);

impl HostFileHandle {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for HostFileHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("HostFileHandle([REDACTED])")
    }
}

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
#[serde(rename_all = "snake_case")]
pub enum EngineState {
    Running,
    Quiescing,
    Quiesced,
    Suspended,
    ShuttingDown,
    Stopped,
}

impl EngineState {
    pub fn accepts_operations(self) -> bool {
        self == Self::Running
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Running, Self::Quiescing)
                | (Self::Quiescing, Self::Quiesced)
                | (Self::Quiesced, Self::Suspended)
                | (Self::Suspended, Self::Running)
                | (Self::Running, Self::ShuttingDown)
                | (Self::Quiescing, Self::ShuttingDown)
                | (Self::Quiesced, Self::ShuttingDown)
                | (Self::Suspended, Self::ShuttingDown)
                | (Self::ShuttingDown, Self::Stopped)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineErrorCategory {
    InvalidInput,
    InvalidState,
    Unauthorized,
    NotFound,
    Conflict,
    Unavailable,
    DeadlineExceeded,
    Internal,
}

impl fmt::Display for EngineErrorCategory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::InvalidInput => "invalid_input",
            Self::InvalidState => "invalid_state",
            Self::Unauthorized => "unauthorized",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Unavailable => "unavailable",
            Self::DeadlineExceeded => "deadline_exceeded",
            Self::Internal => "internal",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineError {
    code: u32,
    category: EngineErrorCategory,
    retryable: bool,
}

impl EngineError {
    pub fn new(code: u32, category: EngineErrorCategory, retryable: bool) -> Self {
        Self {
            code,
            category,
            retryable,
        }
    }

    pub fn code(&self) -> u32 {
        self.code
    }

    pub fn category(&self) -> EngineErrorCategory {
        self.category
    }

    pub fn is_retryable(&self) -> bool {
        self.retryable
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "engine error {} ({})", self.code, self.category)
    }
}

impl std::error::Error for EngineError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshReason {
    ConsumerLagged,
    StateInvalidated,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvent {
    StateChanged {
        state: EngineState,
    },
    IncomingEntry {
        entry: EntrySummary,
    },
    TransferProgress(TransferProgress),
    RefreshRequired {
        reason: RefreshReason,
    },
    OperationFinished {
        operation_id: String,
        terminal: OperationTerminal,
    },
    Fatal {
        error: EngineError,
    },
}

impl EngineEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::StateChanged { .. } => "state_changed",
            Self::IncomingEntry { .. } => "incoming_entry",
            Self::TransferProgress(_) => "transfer_progress",
            Self::RefreshRequired { .. } => "refresh_required",
            Self::OperationFinished { .. } => "operation_finished",
            Self::Fatal { .. } => "fatal",
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferProgress {
    pub transfer_id: String,
    pub completed_bytes: u64,
    pub total_bytes: Option<u64>,
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
    EntrySent {
        entry_id: String,
    },
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
    EntryResent {
        entry_id: String,
    },
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
            Self::EntrySent { .. } => debug.field("kind", &"entry_sent"),
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
            Self::EntryResent { .. } => debug.field("kind", &"entry_resent"),
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
