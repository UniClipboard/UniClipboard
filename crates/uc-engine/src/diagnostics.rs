use std::fmt;

use serde::{Deserialize, Serialize};

use crate::HostFileHandle;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticsStatusSummary {
    pub debug_mode: bool,
    pub effective_log_profile: String,
    pub restart_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateDebugModeInput {
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DebugModeUpdateSummary {
    pub debug_mode: bool,
    pub restart_required: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ExportDiagnosticLogsInput {
    pub since_hours: Option<u32>,
    pub destination: HostFileHandle,
}

impl fmt::Debug for ExportDiagnosticLogsInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ExportDiagnosticLogsInput")
            .field("since_hours", &self.since_hours)
            .field("destination", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticLogsExportSummary {
    pub included_files: Vec<String>,
    pub since_unix_ms: i64,
}

impl fmt::Debug for DiagnosticLogsExportSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DiagnosticLogsExportSummary")
            .field("included_file_count", &self.included_files.len())
            .field("since_unix_ms", &self.since_unix_ms)
            .finish()
    }
}
