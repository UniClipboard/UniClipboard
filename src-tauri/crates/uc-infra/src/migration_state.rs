//! `MigrationStatePort` 的文件持久化实现。
//!
//! 与 `FileSetupStatusRepository` 同款机制：在应用数据目录下落一份 JSON
//! 文件 `.migration_state`，文件内容是 `Option<MigrationPhase>` 的 serde
//! 表示。daemon 重启时自动从该文件恢复。
//!
//! 文件不存在 / 内容为空时视为 `None`（无在飞迁移），让首次启动 / 全新
//! profile 不需要预创建文件。

use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use uc_core::ports::setup::{MigrationStateError, MigrationStatePort};
use uc_core::setup::MigrationPhase;

pub const DEFAULT_MIGRATION_STATE_FILE: &str = ".migration_state";

pub struct FileMigrationStateRepository {
    state_file_path: PathBuf,
}

impl FileMigrationStateRepository {
    /// 自定义文件路径——多用于测试。
    pub fn new(state_file_path: PathBuf) -> Self {
        Self { state_file_path }
    }

    /// 与 `FileSetupStatusRepository::with_defaults` 对齐：在 `base_dir`
    /// 下用约定文件名 `.migration_state`。
    pub fn with_defaults(base_dir: PathBuf) -> Self {
        Self {
            state_file_path: base_dir.join(DEFAULT_MIGRATION_STATE_FILE),
        }
    }

    async fn ensure_parent_dir(&self) -> Result<(), MigrationStateError> {
        if let Some(parent) = self.state_file_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| MigrationStateError::Storage(e.to_string()))?;
        }
        Ok(())
    }
}

#[async_trait]
impl MigrationStatePort for FileMigrationStateRepository {
    async fn get_current(&self) -> Result<Option<MigrationPhase>, MigrationStateError> {
        if !self.state_file_path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&self.state_file_path)
            .await
            .map_err(|e| MigrationStateError::Storage(e.to_string()))?;
        if content.trim().is_empty() {
            return Ok(None);
        }
        let phase: Option<MigrationPhase> = serde_json::from_str(&content)
            .map_err(|e| MigrationStateError::Internal(format!("parse migration state: {e}")))?;
        Ok(phase)
    }

    async fn set_current(&self, phase: Option<MigrationPhase>) -> Result<(), MigrationStateError> {
        self.ensure_parent_dir().await?;
        let json = serde_json::to_string_pretty(&phase).map_err(|e| {
            MigrationStateError::Internal(format!("serialize migration state: {e}"))
        })?;
        let mut file = fs::File::create(&self.state_file_path)
            .await
            .map_err(|e| MigrationStateError::Storage(e.to_string()))?;
        file.write_all(json.as_bytes())
            .await
            .map_err(|e| MigrationStateError::Storage(e.to_string()))?;
        file.sync_all()
            .await
            .map_err(|e| MigrationStateError::Storage(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ids::SpaceId;
    use uc_core::setup::{MigrationPhase, MigrationRunId};

    #[tokio::test]
    async fn fresh_dir_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileMigrationStateRepository::with_defaults(dir.path().to_path_buf());
        assert_eq!(repo.get_current().await.unwrap(), None);
    }

    #[tokio::test]
    async fn round_trip_prepared_phase() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileMigrationStateRepository::with_defaults(dir.path().to_path_buf());
        let phase = MigrationPhase::Prepared {
            run_id: MigrationRunId::new("mig-test"),
        };
        repo.set_current(Some(phase.clone())).await.unwrap();
        assert_eq!(repo.get_current().await.unwrap(), Some(phase));
    }

    #[tokio::test]
    async fn explicit_clear_writes_null() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileMigrationStateRepository::with_defaults(dir.path().to_path_buf());
        let phase = MigrationPhase::HandshakeDone {
            run_id: MigrationRunId::new("mig-2"),
            target_space_id: SpaceId::from_str("space-2"),
            sponsor_space_person_id: None,
        };
        repo.set_current(Some(phase.clone())).await.unwrap();
        repo.set_current(None).await.unwrap();
        assert_eq!(repo.get_current().await.unwrap(), None);
    }

    #[tokio::test]
    async fn empty_file_treated_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".migration_state");
        // 写一个空文件——历史上偶尔会有这种状态（创建但 sync 前崩了）。
        tokio::fs::write(&path, "").await.unwrap();
        let repo = FileMigrationStateRepository::new(path);
        assert_eq!(repo.get_current().await.unwrap(), None);
    }

    #[tokio::test]
    async fn round_trip_swapped_phase() {
        let dir = tempfile::tempdir().unwrap();
        let repo = FileMigrationStateRepository::with_defaults(dir.path().to_path_buf());
        let phase = MigrationPhase::Swapped {
            run_id: MigrationRunId::new("mig-3"),
            target_space_id: SpaceId::from_str("space-3"),
            sponsor_space_person_id: None,
        };
        repo.set_current(Some(phase.clone())).await.unwrap();
        assert_eq!(repo.get_current().await.unwrap(), Some(phase));
    }
}
