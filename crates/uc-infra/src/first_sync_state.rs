//! `FileFirstSyncStateRepository` —— `FirstSyncStatePort` 的文件实现。
//!
//! 落地策略：
//! * 独立小文件 `first-sync-state.json`，**不**塞进 `settings.json`。
//!   设置文件由用户偏好驱动，first-sync 标志位由系统驱动；分开避免"用户重置
//!   设置 → 标志位也丢"这类耦合事故。
//! * 父目录（`app_data_root`）由调用方传入，profile 隔离归 platform 层负责。
//! * 三 flag 合一格式 `{ schema_version, attempted, succeeded, file_succeeded }`，
//!   留 `schema_version` 元字段便于未来格式演进。
//! * 写入走 `tempfile + rename` 的"先写临时文件再重命名"模式，避免半写入
//!   留下损坏标志位。
//! * **Race 防护**：内部 `tokio::sync::Mutex` 把 `read → check → write` 包成
//!   critical section。fan-out N 个 peer 同时调 `mark_*` 时全过同一锁，
//!   只有第一个调用置位返回 `true`，其余返回 `false`——避免重复 fire 事件。
//!
//! 错误语义：
//! * 文件不存在 → 视作"全 false 初值"，第一次 mark 即返回 `Ok(true)`。
//! * IO 失败 → `FirstSyncStateError::Read` / `::Write`。
//! * JSON 不合法或 schema 不识别 → `FirstSyncStateError::Corrupt`。

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uc_core::ports::{FirstSyncStateError, FirstSyncStatePort};

/// 默认状态文件名。落点 = `app_data_root.join(DEFAULT_FILE_NAME)`。
pub const DEFAULT_FILE_NAME: &str = "first-sync-state.json";

/// 文件 schema。`schema_version` 防止未来格式演进时把旧文件误读出错值。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct FirstSyncStateFile {
    /// 文件 schema 版本号。当前固定为 1。
    schema_version: u32,
    /// 是否已 fire 过 `first_clipboard_sync_attempted`。
    #[serde(default)]
    attempted: bool,
    /// 是否已 fire 过 `first_clipboard_sync_succeeded`。
    #[serde(default)]
    succeeded: bool,
    /// 是否已 fire 过 `first_file_sync_succeeded`。
    #[serde(default)]
    file_succeeded: bool,
}

const CURRENT_SCHEMA_VERSION: u32 = 1;

pub struct FileFirstSyncStateRepository {
    file_path: PathBuf,
    /// 串行化 `read → check → write` critical section，保证 fan-out 下
    /// 同一 flag 只有一个调用返回 `true`。
    lock: Mutex<()>,
}

impl FileFirstSyncStateRepository {
    /// 直接指定文件绝对路径（测试 / 自定义部署用）。
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            file_path,
            lock: Mutex::new(()),
        }
    }

    /// 标准用法：传入 profile-aware 的 app data root，落点固定为
    /// `app_data_root/first-sync-state.json`。
    pub fn with_defaults(app_data_root: PathBuf) -> Self {
        Self::new(app_data_root.join(DEFAULT_FILE_NAME))
    }

    fn parent_dir(&self) -> Option<&Path> {
        self.file_path.parent()
    }

    async fn ensure_parent(&self) -> Result<(), FirstSyncStateError> {
        if let Some(parent) = self.parent_dir() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| FirstSyncStateError::Write(format!("mkdir {parent:?}: {e}")))?;
        }
        Ok(())
    }

    async fn read_or_default(&self) -> Result<FirstSyncStateFile, FirstSyncStateError> {
        let raw = match fs::read_to_string(&self.file_path).await {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(FirstSyncStateFile {
                    schema_version: CURRENT_SCHEMA_VERSION,
                    ..Default::default()
                });
            }
            Err(e) => {
                return Err(FirstSyncStateError::Read(format!(
                    "read {:?}: {e}",
                    self.file_path
                )))
            }
        };

        if raw.trim().is_empty() {
            return Err(FirstSyncStateError::Corrupt(format!(
                "{:?} is empty",
                self.file_path
            )));
        }

        let parsed: FirstSyncStateFile = serde_json::from_str(&raw).map_err(|e| {
            FirstSyncStateError::Corrupt(format!("parse {:?}: {e}", self.file_path))
        })?;

        if parsed.schema_version != CURRENT_SCHEMA_VERSION {
            // 未来扩 schema 时这里会变成 migrate 分支；当前只接受 v1。
            return Err(FirstSyncStateError::Corrupt(format!(
                "{:?} has unsupported schema_version {}",
                self.file_path, parsed.schema_version
            )));
        }

        Ok(parsed)
    }

    async fn write(&self, state: &FirstSyncStateFile) -> Result<(), FirstSyncStateError> {
        self.ensure_parent().await?;

        let body = serde_json::to_vec_pretty(state)
            .map_err(|e| FirstSyncStateError::Write(format!("serialize state: {e}")))?;

        let tmp_path = self.file_path.with_extension("json.tmp");

        {
            let mut file = fs::File::create(&tmp_path)
                .await
                .map_err(|e| FirstSyncStateError::Write(format!("create {tmp_path:?}: {e}")))?;
            file.write_all(&body)
                .await
                .map_err(|e| FirstSyncStateError::Write(format!("write {tmp_path:?}: {e}")))?;
            file.sync_all()
                .await
                .map_err(|e| FirstSyncStateError::Write(format!("fsync {tmp_path:?}: {e}")))?;
        }

        fs::rename(&tmp_path, &self.file_path).await.map_err(|e| {
            FirstSyncStateError::Write(format!("rename {tmp_path:?} -> {:?}: {e}", self.file_path))
        })?;

        Ok(())
    }

    /// 三个 mark_* 的共用 critical section：在同一锁内 read → check → set → write。
    /// `selector` 选定要置位的字段；返回值 `true` 表示本次为首次置位。
    async fn mark_flag(
        &self,
        selector: fn(&mut FirstSyncStateFile) -> &mut bool,
    ) -> Result<bool, FirstSyncStateError> {
        let _guard = self.lock.lock().await;
        let mut state = self.read_or_default().await?;
        let flag = selector(&mut state);
        if *flag {
            return Ok(false);
        }
        *flag = true;
        // schema_version 在 default 路径上可能为 0（FirstSyncStateFile::default()），
        // 落盘前统一拨正到当前版本。
        state.schema_version = CURRENT_SCHEMA_VERSION;
        self.write(&state).await?;
        Ok(true)
    }
}

#[async_trait]
impl FirstSyncStatePort for FileFirstSyncStateRepository {
    async fn mark_first_sync_attempted(&self) -> Result<bool, FirstSyncStateError> {
        self.mark_flag(|s| &mut s.attempted).await
    }

    async fn mark_first_sync_succeeded(&self) -> Result<bool, FirstSyncStateError> {
        self.mark_flag(|s| &mut s.succeeded).await
    }

    async fn mark_first_file_sync_succeeded(&self) -> Result<bool, FirstSyncStateError> {
        self.mark_flag(|s| &mut s.file_succeeded).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn repo() -> (TempDir, FileFirstSyncStateRepository) {
        let tmp = TempDir::new().unwrap();
        let repo = FileFirstSyncStateRepository::with_defaults(tmp.path().to_path_buf());
        (tmp, repo)
    }

    #[tokio::test]
    async fn mark_attempted_when_file_missing_returns_true_then_false() {
        let (_tmp, repo) = repo();
        assert!(repo.mark_first_sync_attempted().await.unwrap());
        assert!(!repo.mark_first_sync_attempted().await.unwrap());
    }

    #[tokio::test]
    async fn three_flags_round_trip_independently() {
        let (_tmp, repo) = repo();
        assert!(repo.mark_first_sync_attempted().await.unwrap());
        assert!(repo.mark_first_sync_succeeded().await.unwrap());
        assert!(repo.mark_first_file_sync_succeeded().await.unwrap());
        // 第二次全部 false
        assert!(!repo.mark_first_sync_attempted().await.unwrap());
        assert!(!repo.mark_first_sync_succeeded().await.unwrap());
        assert!(!repo.mark_first_file_sync_succeeded().await.unwrap());
    }

    #[tokio::test]
    async fn overwrite_persists_all_set_flags() {
        let (tmp, repo) = repo();
        repo.mark_first_sync_attempted().await.unwrap();
        repo.mark_first_sync_succeeded().await.unwrap();
        // 文件应同时含两个 true
        let raw = fs::read_to_string(tmp.path().join(DEFAULT_FILE_NAME))
            .await
            .unwrap();
        let parsed: FirstSyncStateFile = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(parsed.attempted);
        assert!(parsed.succeeded);
        assert!(!parsed.file_succeeded);
    }

    #[tokio::test]
    async fn corrupt_json_returns_corrupt_error() {
        let (tmp, repo) = repo();
        let path = tmp.path().join(DEFAULT_FILE_NAME);
        fs::write(&path, b"{not valid json").await.unwrap();
        let err = repo.mark_first_sync_attempted().await.unwrap_err();
        assert!(matches!(err, FirstSyncStateError::Corrupt(_)));
    }

    #[tokio::test]
    async fn empty_file_returns_corrupt_error() {
        let (tmp, repo) = repo();
        let path = tmp.path().join(DEFAULT_FILE_NAME);
        fs::write(&path, b"").await.unwrap();
        let err = repo.mark_first_sync_attempted().await.unwrap_err();
        assert!(matches!(err, FirstSyncStateError::Corrupt(_)));
    }

    #[tokio::test]
    async fn unsupported_schema_version_returns_corrupt_error() {
        let (tmp, repo) = repo();
        let path = tmp.path().join(DEFAULT_FILE_NAME);
        fs::write(
            &path,
            br#"{"schema_version":999,"attempted":false,"succeeded":false,"file_succeeded":false}"#,
        )
        .await
        .unwrap();
        let err = repo.mark_first_sync_attempted().await.unwrap_err();
        assert!(matches!(err, FirstSyncStateError::Corrupt(_)));
    }

    #[tokio::test]
    async fn write_creates_parent_dir() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("does/not/exist/yet");
        let repo = FileFirstSyncStateRepository::with_defaults(nested.clone());
        assert!(repo.mark_first_sync_attempted().await.unwrap());
        assert!(nested.join(DEFAULT_FILE_NAME).exists());
    }

    #[tokio::test]
    async fn concurrent_mark_attempted_returns_true_exactly_once() {
        let (_tmp, repo) = repo();
        let repo = Arc::new(repo);
        const N: usize = 8;
        let mut handles = Vec::with_capacity(N);
        for _ in 0..N {
            let r = Arc::clone(&repo);
            handles.push(tokio::spawn(async move {
                r.mark_first_sync_attempted().await.unwrap()
            }));
        }
        let mut true_count = 0;
        for h in handles {
            if h.await.unwrap() {
                true_count += 1;
            }
        }
        assert_eq!(
            true_count, 1,
            "fan-out {N} concurrent marks should yield exactly one `true`",
        );
    }
}
