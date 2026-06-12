//! File-based setup status repository
//!
//! This module provides a file-based implementation of the SetupStatusPort,
//! persisting setup status to a local JSON file in the application data directory.

use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uc_core::ports::SetupStatusPort;
use uc_core::setup::SetupStatus;

pub const DEFAULT_SETUP_STATUS_FILE: &str = ".setup_status";

pub struct FileSetupStatusRepository {
    status_file_path: PathBuf,
}

impl FileSetupStatusRepository {
    /// Create repository with custom file path
    pub fn new(status_file_path: PathBuf) -> Self {
        Self { status_file_path }
    }

    /// Create repository with base dir and filename
    pub fn with_base_dir(base_dir: PathBuf, filename: impl Into<String>) -> Self {
        Self {
            status_file_path: base_dir.join(filename.into()),
        }
    }

    /// Create repository with defaults
    pub fn with_defaults(base_dir: PathBuf) -> Self {
        Self {
            status_file_path: base_dir.join(DEFAULT_SETUP_STATUS_FILE),
        }
    }

    async fn ensure_parent_dir(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.status_file_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl SetupStatusPort for FileSetupStatusRepository {
    async fn get_status(&self) -> anyhow::Result<SetupStatus> {
        if !self.status_file_path.exists() {
            return Ok(SetupStatus::default());
        }

        self.ensure_parent_dir().await?;
        let content = fs::read_to_string(&self.status_file_path).await?;

        if content.trim().is_empty() {
            return Ok(SetupStatus::default());
        }

        let status: SetupStatus = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse setup status: {e}"))?;

        Ok(status)
    }

    async fn set_status(&self, status: &SetupStatus) -> anyhow::Result<()> {
        self.ensure_parent_dir().await?;

        let json = serde_json::to_string_pretty(status)
            .map_err(|e| anyhow::anyhow!("Failed to serialize setup status: {e}"))?;

        let mut file = fs::File::create(&self.status_file_path)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create status file: {e}"))?;

        file.write_all(json.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write status file: {e}"))?;

        file.sync_all()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to sync status file: {e}"))?;

        Ok(())
    }
}
