//! Public startup product state. Independent of Engine implementation types.
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StartupStateDto {
    Preparing,
    Upgrading,
    StartingServices,
    Ready,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StartupStepDto {
    Checking,
    ConvertingContents,
    ConvertingLargeContents,
    ConvertingRelatedRecords,
    Verifying,
    Preparing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StartupUnitDto {
    ContentRepresentations,
    LargeContents,
    RelatedRecords,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StartupFailureReasonDto {
    StorageFull,
    PermissionDenied,
    StorageUnavailable,
    ProtectionUnavailable,
    CorruptData,
    SourceChanged,
    AlreadyRunning,
    StartupFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StartupStepProgressDto {
    pub step: StartupStepDto,
    #[specta(type = specta_typescript::Number<u64>)]
    pub processed: u64,
    #[specta(type = Option<specta_typescript::Number<u64>>)]
    pub total: Option<u64>,
    pub unit: Option<StartupUnitDto>,
    #[specta(type = Option<specta_typescript::Number<u64>>)]
    pub warning_count: Option<u64>,
    pub completed: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StartupUpgradeDto {
    pub required: bool,
    pub recovering: bool,
    pub completed: bool,
    pub current_step: Option<StartupStepDto>,
    pub steps: Vec<StartupStepProgressDto>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StartupFailureDto {
    pub reason: StartupFailureReasonDto,
    pub retryable: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StartupActionsDto {
    pub retry: bool,
    pub export_diagnostics: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct StartupSnapshotDto {
    pub attempt_id: String,
    #[specta(type = specta_typescript::Number<u64>)]
    pub sequence: u64,
    pub state: StartupStateDto,
    #[specta(type = specta_typescript::Number<u64>)]
    pub elapsed_ms: u64,
    pub upgrade: Option<StartupUpgradeDto>,
    pub failure: Option<StartupFailureDto>,
    pub allowed_actions: StartupActionsDto,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DaemonStartupStatus {
    pub package_version: String,
    pub service_ready: bool,
    pub service_failed: bool,
    pub progress: StartupSnapshotDto,
}
impl DaemonStartupStatus {
    pub fn is_failed(&self) -> bool {
        self.service_failed
            || matches!(
                self.progress.state,
                StartupStateDto::Failed | StartupStateDto::Interrupted
            )
    }
}
