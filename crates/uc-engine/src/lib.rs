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
    SendText,
    SendImage,
    SendFiles,
    QueryHistory,
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
            Self::SendText => "send_text",
            Self::SendImage => "send_image",
            Self::SendFiles => "send_files",
            Self::QueryHistory => "query_history",
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
    SendText(SendTextInput),
    SendImage(SendImageInput),
    SendFiles(SendFilesInput),
    QueryHistory(QueryHistoryInput),
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
            Self::SendText(_) => OperationKind::SendText,
            Self::SendImage(_) => OperationKind::SendImage,
            Self::SendFiles(_) => OperationKind::SendFiles,
            Self::QueryHistory(_) => OperationKind::QueryHistory,
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
    EntrySent {
        entry_id: String,
    },
    HistoryPage {
        entries: Vec<EntrySummary>,
        next_cursor: Option<String>,
    },
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
            Self::EntrySent { .. } => debug.field("kind", &"entry_sent"),
            Self::HistoryPage {
                entries,
                next_cursor,
            } => debug
                .field("kind", &"history_page")
                .field("entry_count", &entries.len())
                .field("has_next_cursor", &next_cursor.is_some()),
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
