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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticArchiveCollectionDto {
    pub included_files: Vec<String>,
    pub unreadable_files: Vec<String>,
    pub truncated_files: Vec<String>,
    pub concurrent_writes_possible: bool,
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
