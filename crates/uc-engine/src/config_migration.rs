use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{HostFileHandle, SecretString};

#[derive(Clone, PartialEq, Eq)]
pub struct ExportConfigInput {
    pub destination: HostFileHandle,
}

impl fmt::Debug for ExportConfigInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExportConfigInput")
            .field("destination", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PreviewConfigImportInput {
    pub source: HostFileHandle,
    pub password: SecretString,
}

impl fmt::Debug for PreviewConfigImportInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreviewConfigImportInput")
            .field("source", &"[REDACTED]")
            .field("password", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct StageConfigImportInput {
    pub source: HostFileHandle,
    pub password: SecretString,
}

impl fmt::Debug for StageConfigImportInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StageConfigImportInput")
            .field("source", &"[REDACTED]")
            .field("password", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigSourceModeSummary {
    Portable,
    Installed,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigImportPreviewSummary {
    pub app_version: String,
    pub source_mode: ConfigSourceModeSummary,
    pub created_at_unix_ms: i64,
    pub profile_id: String,
    pub device_fingerprint: String,
}

impl fmt::Debug for ConfigImportPreviewSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ConfigImportPreviewSummary")
            .field("app_version", &self.app_version)
            .field("source_mode", &self.source_mode)
            .field("created_at_unix_ms", &self.created_at_unix_ms)
            .field("profile_id", &"[REDACTED]")
            .field("device_fingerprint", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigExportOutcome {
    Exported,
    Locked,
    NotInitialized,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ConfigImportPreviewOutcome {
    Ready(ConfigImportPreviewSummary),
    InvalidPasswordOrCorrupt,
    Incompatible { reason: String },
}

impl fmt::Debug for ConfigImportPreviewOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready(summary) => formatter
                .debug_tuple("ConfigImportPreviewOutcome::Ready")
                .field(summary)
                .finish(),
            Self::InvalidPasswordOrCorrupt => {
                formatter.write_str("ConfigImportPreviewOutcome::InvalidPasswordOrCorrupt")
            }
            Self::Incompatible { .. } => {
                formatter.write_str("ConfigImportPreviewOutcome::Incompatible")
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ConfigImportStageOutcome {
    Staged { unlock_required_after_apply: bool },
    InvalidPasswordOrCorrupt,
    Incompatible { reason: String },
}

impl fmt::Debug for ConfigImportStageOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Staged {
                unlock_required_after_apply,
            } => formatter
                .debug_struct("ConfigImportStageOutcome::Staged")
                .field("unlock_required_after_apply", unlock_required_after_apply)
                .finish(),
            Self::InvalidPasswordOrCorrupt => {
                formatter.write_str("ConfigImportStageOutcome::InvalidPasswordOrCorrupt")
            }
            Self::Incompatible { .. } => {
                formatter.write_str("ConfigImportStageOutcome::Incompatible")
            }
        }
    }
}
