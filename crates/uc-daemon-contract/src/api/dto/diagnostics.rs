use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DebugStatusDto {
    pub debug_mode: bool,
    pub effective_log_profile: String,
    pub restart_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDebugModeRequestDto {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDebugModeResultDto {
    pub debug_mode: bool,
    pub restart_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticCaptureModeDto {
    Standard,
    Detailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticCaptureEndReasonDto {
    Expired,
    Requested,
    SuspensionExpiryUnknown,
    RuntimeShutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticCaptureStopResultDto {
    Stopped,
    AlreadyStopped,
    DifferentCapture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSignalResultDto {
    Completed,
    Failed,
    TimedOut,
    AlreadyShutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSetupStatusDto {
    Disabled,
    Ready,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSourceDto {
    Runtime,
    Connections,
    AddressStorage,
    DnsDiscovery,
    MdnsDiscovery,
    PkarrDiscovery,
    ConnectionPaths,
    RelayRecovery,
    MembershipUpdates,
    Sessions,
    HostApplication,
    HostShareExtension,
    HostKeyboardExtension,
    HostBackgroundService,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSourceCapabilityDto {
    Supported,
    Partial,
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSourceCollectionDto {
    Enabled,
    Disabled,
    Unavailable,
    NotRegistered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCaptureStateDto {
    pub mode: DiagnosticCaptureModeDto,
    pub capture_id: Option<String>,
    pub remaining_ms: u64,
    pub started_at_utc: Option<String>,
    pub end_reason: Option<DiagnosticCaptureEndReasonDto>,
    pub last_capture_id: Option<String>,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticSourceCoverageDto {
    pub source: DiagnosticSourceDto,
    pub capability: DiagnosticSourceCapabilityDto,
    pub collection: DiagnosticSourceCollectionDto,
    pub observed_count: String,
    pub policy_filtered_count: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticStatusDto {
    pub run_id: String,
    pub capture: DiagnosticCaptureStateDto,
    pub observed_records: String,
    pub policy_filtered_records: String,
    pub schema_rejected_records: String,
    pub correlation_limited_records: String,
    pub engine_version: String,
    pub source_commit: String,
    pub counter_scope: String,
    pub sources: Vec<DiagnosticSourceCoverageDto>,
    pub local_file: DiagnosticSetupStatusDto,
    pub closed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticFileSourceCountsDto {
    pub source: DiagnosticSourceDto,
    pub accepted_count: String,
    pub written_count: String,
    pub queue_dropped_count: String,
    pub quota_dropped_count: String,
    pub write_failed_count: String,
    pub last_written_at_ms: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticExportPreparationDto {
    pub flush: DiagnosticSignalResultDto,
    pub status: DiagnosticStatusDto,
    pub requested_at_utc: String,
    pub completed_at_utc: String,
    pub other_processes_flushed: bool,
    pub files: Vec<DiagnosticFileSourceCountsDto>,
    /// The independent network-library log, prepared separately from the
    /// business log above: its success or failure never changes `flush`.
    pub network: NetworkExportPreparationDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticArchiveCollectionDto {
    pub included_files: Vec<String>,
    pub unreadable_files: Vec<String>,
    pub truncated_files: Vec<String>,
    pub concurrent_writes_possible: bool,
    /// Log sources (`gui`, `daemon`, `cli`, `quick-panel`, `engine`) that
    /// contributed no file to the package.
    pub missing_sources: Vec<MissingLogSourceDto>,
    /// What the package holds of the independent network-library log.
    pub network: NetworkArchiveCollectionDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MissingLogSourceDto {
    pub source: String,
    /// `missing` (no file of this source) or `unreadable` (files exist, none
    /// could be exported).
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCaptureStartRequestDto {
    pub duration_seconds: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCaptureStopRequestDto {
    pub capture_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogExportRequestDto {
    #[serde(default)]
    pub since_hours: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogExportResultDto {
    pub path: String,
    pub included_files: Vec<String>,
    pub since: DateTime<Utc>,
    pub engine_preparation: DiagnosticExportPreparationDto,
    pub collection: DiagnosticArchiveCollectionDto,
}

/// Whether this process records the network-library log at all. `NotConfigured`
/// is a statement about configuration, never "no network activity".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NetworkLogAvailabilityDto {
    NotConfigured,
    Configured,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NetworkLogStateDto {
    Starting,
    Ready,
    Unavailable,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NetworkProcessRoleDto {
    Daemon,
    MobileApp,
    KeyboardExtension,
    ShareExtension,
    BackgroundService,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NetworkCaptureModeDto {
    Standard,
    Detailed,
}

/// Capacity the daemon passed to the Engine. Byte and count values are decimal
/// strings, like every other counter on this API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkLogBudgetDto {
    pub record_bytes: String,
    pub queue_records: String,
    pub file_bytes: String,
    pub run_bytes: String,
    pub root_bytes: String,
    pub retention_seconds: String,
}

/// Monotonic counters of this run. A missing or zero counter never proves the
/// network was quiet: compare the drop counters before reading an empty log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkLogCountersDto {
    pub accepted: String,
    pub written: String,
    pub policy_filtered: String,
    pub rate_limited: String,
    pub oversized: String,
    pub queue_dropped: String,
    pub closed_dropped: String,
    pub write_failed: String,
    pub rotated_out_files: String,
    pub path_recovered: String,
    pub unreviewed_callsites: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkLibraryStatusDto {
    pub library: String,
    pub version_dir: Option<String>,
    pub approved_rules: u32,
    pub observed_records: String,
}

/// State, budget and drop information of the network-library log. Only the
/// `availability` field is present when the log is not configured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkLogStatusDto {
    pub availability: NetworkLogAvailabilityDto,
    pub state: Option<NetworkLogStateDto>,
    pub run_id: Option<String>,
    pub role: Option<NetworkProcessRoleDto>,
    pub mode: Option<NetworkCaptureModeDto>,
    /// Identifier of the open detailed window, needed to stop it. The Engine does
    /// not report it; the daemon remembers the one its own start call returned.
    pub capture_id: Option<String>,
    pub detailed_remaining_ms: Option<u64>,
    pub counters: Option<NetworkLogCountersDto>,
    pub libraries: Vec<NetworkLibraryStatusDto>,
    pub budget: Option<NetworkLogBudgetDto>,
    pub sampled_at_utc: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkCaptureStartRequestDto {
    pub duration_seconds: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkCaptureStopRequestDto {
    pub capture_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkCaptureStartResultDto {
    pub capture_id: String,
    pub started_at_utc: String,
    pub duration_ms: u64,
    /// A window was already open; it is returned unchanged and not extended.
    pub already_active: bool,
    pub status: NetworkLogStatusDto,
}

/// One file the Engine listed when it prepared the export, with the digest it
/// computed over the length it observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkLogFileDto {
    pub name: String,
    pub bytes: String,
    pub sha256: String,
}

/// The Engine's network-log export preparation for this process. It says
/// nothing about other processes: `otherProcessesFlushed` is always false.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkExportPreparationDto {
    pub availability: NetworkLogAvailabilityDto,
    pub flush: Option<DiagnosticSignalResultDto>,
    pub status: Option<NetworkLogStatusDto>,
    pub requested_at_utc: Option<String>,
    pub completed_at_utc: Option<String>,
    pub other_processes_flushed: bool,
    pub files: Vec<NetworkLogFileDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NetworkArchiveOutcomeDto {
    /// Every file of the current run was read in full and matches the digest
    /// the Engine computed after flushing.
    Complete,
    /// Some log was collected but `reasons` lists what is not covered.
    Partial,
    /// A configured log produced no file.
    Missing,
    NotConfigured,
    /// The log could not be examined at all.
    Unknown,
}

/// Why a package's network log is not `complete`. Reasons are cumulative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum NetworkArchiveReasonDto {
    /// The package was built without the running daemon: nothing was flushed
    /// and counters are unknown.
    EngineOffline,
    FlushFailed,
    FlushTimedOut,
    /// The network log of this process is not ready.
    StateNotReady,
    /// The Engine counted records it did not write.
    RecordsDropped,
    FileUnreadable,
    FileTruncated,
    /// A file grew after the Engine prepared the export; the digest of the
    /// prepared prefix still matches.
    FileGrewAfterPrepare,
    /// A file differs from what the Engine prepared, or is gone.
    FileChangedAfterPrepare,
    /// Another process may hold unflushed records of its own.
    OtherProcessesNotFlushed,
    /// Entries outside the allowed file set were left out.
    EntriesIgnored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkArchiveFileDto {
    pub name: String,
    /// Bytes copied into the package.
    pub bytes: String,
    /// SHA-256 of exactly the copied bytes.
    pub sha256: String,
    pub truncated: bool,
    /// Whether the copied prefix matches the digest the Engine computed; `None`
    /// when the Engine prepared nothing for this file.
    pub matches_prepared: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkArchiveRunDto {
    pub role: String,
    pub run_id: String,
    /// The run of the exporting daemon, as opposed to a retained earlier run.
    pub current_run: bool,
    pub files: Vec<NetworkArchiveFileDto>,
    pub unreadable_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkArchiveCollectionDto {
    pub outcome: NetworkArchiveOutcomeDto,
    pub reasons: Vec<NetworkArchiveReasonDto>,
    pub runs: Vec<NetworkArchiveRunDto>,
    /// Entries under the network log directory that were not exported.
    pub ignored_entries: u32,
}
