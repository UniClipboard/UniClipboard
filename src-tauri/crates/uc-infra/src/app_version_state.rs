//! `FileAppVersionStateRepository` —— `AppVersionStatePort` 的文件实现。
//!
//! 落地策略：
//! * 独立小文件 `upgrade-cursor.json`，**不**塞进 `settings.json`。
//!   设置文件由用户偏好驱动，升级游标由系统驱动，二者生命周期不同；分开避免
//!   "用户重置设置 → 游标也丢" 这类耦合事故。
//! * 父目录（`app_data_root`）由调用方传入，profile 隔离归 platform 层负责。
//! * 单字段格式 `{ "last_seen_version": "x.y.z" }`，留 `version` 元字段
//!   便于未来格式演进。
//! * 写入走 `tempfile + rename` 的"先写临时文件再重命名"模式，避免半写入
//!   留下损坏游标。
//!
//! 错误语义：
//! * 文件不存在 → `Ok(None)`（最常见路径，不打 warn）。
//! * IO 失败 → `AppVersionStateError::Read` / `::Write`。
//! * JSON 不合法或 schema 不识别 → `AppVersionStateError::Corrupt`。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uc_core::ports::{AppVersionStateError, AppVersionStatePort};

/// 默认游标文件名。落点 = `app_data_root.join(DEFAULT_FILE_NAME)`。
pub const DEFAULT_FILE_NAME: &str = "upgrade-cursor.json";

/// 文件 schema。`schema_version` 防止未来格式演进时把旧文件误读出空值。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpgradeCursorFile {
    /// 文件 schema 版本号。当前固定为 1。
    schema_version: u32,
    /// 上次成功启动的应用版本字符串（典型为 semver）。
    last_seen_version: String,
}

const CURRENT_SCHEMA_VERSION: u32 = 1;

pub struct FileAppVersionStateRepository {
    file_path: PathBuf,
}

impl FileAppVersionStateRepository {
    /// 直接指定文件绝对路径（测试 / 自定义部署用）。
    pub fn new(file_path: PathBuf) -> Self {
        Self { file_path }
    }

    /// 标准用法：传入 profile-aware 的 app data root，落点固定为
    /// `app_data_root/upgrade-cursor.json`。
    pub fn with_defaults(app_data_root: PathBuf) -> Self {
        Self {
            file_path: app_data_root.join(DEFAULT_FILE_NAME),
        }
    }

    fn parent_dir(&self) -> Option<&Path> {
        self.file_path.parent()
    }

    async fn ensure_parent(&self) -> Result<(), AppVersionStateError> {
        if let Some(parent) = self.parent_dir() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| AppVersionStateError::Write(format!("mkdir {parent:?}: {e}")))?;
        }
        Ok(())
    }
}

#[async_trait]
impl AppVersionStatePort for FileAppVersionStateRepository {
    async fn read(&self) -> Result<Option<String>, AppVersionStateError> {
        let raw = match fs::read_to_string(&self.file_path).await {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(AppVersionStateError::Read(format!(
                    "read {:?}: {e}",
                    self.file_path
                )))
            }
        };

        if raw.trim().is_empty() {
            return Err(AppVersionStateError::Corrupt(format!(
                "{:?} is empty",
                self.file_path
            )));
        }

        let parsed: UpgradeCursorFile = serde_json::from_str(&raw).map_err(|e| {
            AppVersionStateError::Corrupt(format!("parse {:?}: {e}", self.file_path))
        })?;

        if parsed.schema_version != CURRENT_SCHEMA_VERSION {
            // 未来扩 schema 时这里会变成 migrate 分支；当前只接受 v1。
            return Err(AppVersionStateError::Corrupt(format!(
                "{:?} has unsupported schema_version {}",
                self.file_path, parsed.schema_version
            )));
        }

        Ok(Some(parsed.last_seen_version))
    }

    async fn write(&self, version: &str) -> Result<(), AppVersionStateError> {
        self.ensure_parent().await?;

        let payload = UpgradeCursorFile {
            schema_version: CURRENT_SCHEMA_VERSION,
            last_seen_version: version.to_string(),
        };
        let body = serde_json::to_vec_pretty(&payload)
            .map_err(|e| AppVersionStateError::Write(format!("serialize cursor: {e}")))?;

        // 写临时文件 + 原子 rename，避免崩溃后留下损坏的游标。
        let tmp_path = self.file_path.with_extension("json.tmp");

        // 显式把句柄关闭后再 rename：tokio::fs::File 不暴露同步 close，
        // 这里用 drop 触发后的 rename 是事实上的标准模式。
        {
            let mut file = fs::File::create(&tmp_path)
                .await
                .map_err(|e| AppVersionStateError::Write(format!("create {tmp_path:?}: {e}")))?;
            file.write_all(&body)
                .await
                .map_err(|e| AppVersionStateError::Write(format!("write {tmp_path:?}: {e}")))?;
            file.sync_all()
                .await
                .map_err(|e| AppVersionStateError::Write(format!("fsync {tmp_path:?}: {e}")))?;
        }

        fs::rename(&tmp_path, &self.file_path).await.map_err(|e| {
            AppVersionStateError::Write(format!("rename {tmp_path:?} -> {:?}: {e}", self.file_path))
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn repo() -> (TempDir, FileAppVersionStateRepository) {
        let tmp = TempDir::new().unwrap();
        let repo = FileAppVersionStateRepository::with_defaults(tmp.path().to_path_buf());
        (tmp, repo)
    }

    #[tokio::test]
    async fn read_returns_none_when_file_missing() {
        let (_tmp, repo) = repo();
        assert_eq!(repo.read().await.unwrap(), None);
    }

    #[tokio::test]
    async fn write_then_read_round_trips() {
        let (_tmp, repo) = repo();
        repo.write("1.0.0-alpha.1").await.unwrap();
        assert_eq!(repo.read().await.unwrap().as_deref(), Some("1.0.0-alpha.1"));
    }

    #[tokio::test]
    async fn write_overwrites_existing_value() {
        let (_tmp, repo) = repo();
        repo.write("0.9.0").await.unwrap();
        repo.write("1.0.0").await.unwrap();
        assert_eq!(repo.read().await.unwrap().as_deref(), Some("1.0.0"));
    }

    #[tokio::test]
    async fn corrupt_json_returns_corrupt_error() {
        let (tmp, repo) = repo();
        let path = tmp.path().join(DEFAULT_FILE_NAME);
        fs::write(&path, b"{not valid json").await.unwrap();
        let err = repo.read().await.unwrap_err();
        assert!(matches!(err, AppVersionStateError::Corrupt(_)));
    }

    #[tokio::test]
    async fn empty_file_returns_corrupt_error() {
        let (tmp, repo) = repo();
        let path = tmp.path().join(DEFAULT_FILE_NAME);
        fs::write(&path, b"").await.unwrap();
        let err = repo.read().await.unwrap_err();
        assert!(matches!(err, AppVersionStateError::Corrupt(_)));
    }

    #[tokio::test]
    async fn unsupported_schema_version_returns_corrupt_error() {
        let (tmp, repo) = repo();
        let path = tmp.path().join(DEFAULT_FILE_NAME);
        fs::write(
            &path,
            br#"{"schema_version":999,"last_seen_version":"1.0.0"}"#,
        )
        .await
        .unwrap();
        let err = repo.read().await.unwrap_err();
        assert!(matches!(err, AppVersionStateError::Corrupt(_)));
    }

    #[tokio::test]
    async fn write_creates_parent_dir() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("does/not/exist/yet");
        let repo = FileAppVersionStateRepository::with_defaults(nested);
        repo.write("1.0.0").await.unwrap();
        assert_eq!(repo.read().await.unwrap().as_deref(), Some("1.0.0"));
    }
}
