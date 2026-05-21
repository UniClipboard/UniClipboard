//! 持久化"已通知过的版本"，按 update channel 分桶。
//!
//! Update scheduler 在主循环里检测到新版本时，先查这个 store；
//! 若同一 (channel, version) 已通知过则跳过，避免重复打扰用户。
//!
//! 实现细节：
//! - 单文件 `last_notified_update.json`，存放在 `AppPaths::app_data_root_dir`
//! - 内部存为 `HashMap<String, String>`，key 是 channel 的 snake_case 字符串
//!   （与 `UpdateChannel` 的 `serde(rename_all = "snake_case")` wire 形态一致）
//! - 损坏 / 缺失 / 空文件均视为"从未通知过"，自愈式重写
//! - 版本比较走字符串相等（Q8.1：不引入 semver）

use std::{collections::HashMap, io, path::Path};

use serde::{Deserialize, Serialize};
use tokio::fs;
use uc_core::settings::model::UpdateChannel;

/// 已通知过的版本记录。按 channel 维度去重。
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct LastNotifiedUpdateStore {
    entries: HashMap<String, String>,
}

impl LastNotifiedUpdateStore {
    /// 从磁盘读取；文件缺失、为空或解析失败均返回空 store（自愈语义）。
    pub async fn load(path: &Path) -> Self {
        match fs::read(path).await {
            Ok(bytes) if bytes.is_empty() => Self::default(),
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                tracing::warn!(
                    target = "update_scheduler::last_notified",
                    error = %err,
                    path = %path.display(),
                    "corrupted last_notified_update.json; treating as empty"
                );
                Self::default()
            }),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(err) => {
                tracing::warn!(
                    target = "update_scheduler::last_notified",
                    error = %err,
                    path = %path.display(),
                    "failed to read last_notified_update.json; treating as empty"
                );
                Self::default()
            }
        }
    }

    /// 检查给定 channel 上记录的版本是否等于 `version`。
    pub fn contains(&self, channel: &UpdateChannel, version: &str) -> bool {
        self.entries
            .get(channel_key(channel))
            .is_some_and(|v| v == version)
    }

    /// 记录 (channel, version) 并立即持久化。
    ///
    /// 重复写入相同 (channel, version) 是幂等的，但仍会触发一次 disk write
    /// （成本可忽略；调用方一般会先 `contains` 过滤）。
    pub async fn record(
        &mut self,
        channel: UpdateChannel,
        version: String,
        path: &Path,
    ) -> io::Result<()> {
        self.entries
            .insert(channel_key(&channel).to_string(), version);
        self.persist(path).await
    }

    async fn persist(&self, path: &Path) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(path, bytes).await
    }
}

/// 把 `UpdateChannel` 映射成 JSON 上落地的 snake_case key。
///
/// 与 `commands/updater.rs::channel_as_str` 保持一致，避免双源演化。
fn channel_key(channel: &UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Alpha => "alpha",
        UpdateChannel::Beta => "beta",
        UpdateChannel::Rc => "rc",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store_path(dir: &TempDir) -> std::path::PathBuf {
        dir.path().join("last_notified_update.json")
    }

    #[tokio::test]
    async fn load_returns_empty_when_file_missing() {
        let dir = TempDir::new().unwrap();
        let store = LastNotifiedUpdateStore::load(&store_path(&dir)).await;
        assert_eq!(store, LastNotifiedUpdateStore::default());
        assert!(!store.contains(&UpdateChannel::Stable, "0.12.0"));
    }

    #[tokio::test]
    async fn load_returns_empty_when_file_is_empty() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        fs::write(&path, b"").await.unwrap();
        let store = LastNotifiedUpdateStore::load(&path).await;
        assert_eq!(store, LastNotifiedUpdateStore::default());
    }

    #[tokio::test]
    async fn load_returns_empty_when_file_is_corrupted() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);
        fs::write(&path, b"{ this is not json").await.unwrap();
        let store = LastNotifiedUpdateStore::load(&path).await;
        assert_eq!(store, LastNotifiedUpdateStore::default());
    }

    #[tokio::test]
    async fn record_persists_and_load_round_trips() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);

        let mut store = LastNotifiedUpdateStore::load(&path).await;
        store
            .record(UpdateChannel::Stable, "0.12.0".into(), &path)
            .await
            .unwrap();

        let reloaded = LastNotifiedUpdateStore::load(&path).await;
        assert!(reloaded.contains(&UpdateChannel::Stable, "0.12.0"));
        assert!(!reloaded.contains(&UpdateChannel::Stable, "0.13.0"));
    }

    #[tokio::test]
    async fn record_multiple_channels_independent() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);

        let mut store = LastNotifiedUpdateStore::default();
        store
            .record(UpdateChannel::Stable, "0.12.0".into(), &path)
            .await
            .unwrap();
        store
            .record(UpdateChannel::Alpha, "0.13.0-alpha.1".into(), &path)
            .await
            .unwrap();

        let reloaded = LastNotifiedUpdateStore::load(&path).await;
        assert!(reloaded.contains(&UpdateChannel::Stable, "0.12.0"));
        assert!(reloaded.contains(&UpdateChannel::Alpha, "0.13.0-alpha.1"));
        assert!(!reloaded.contains(&UpdateChannel::Beta, "0.12.0"));
    }

    #[tokio::test]
    async fn record_overwrites_same_channel() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);

        let mut store = LastNotifiedUpdateStore::default();
        store
            .record(UpdateChannel::Stable, "0.11.0".into(), &path)
            .await
            .unwrap();
        store
            .record(UpdateChannel::Stable, "0.12.0".into(), &path)
            .await
            .unwrap();

        let reloaded = LastNotifiedUpdateStore::load(&path).await;
        assert!(!reloaded.contains(&UpdateChannel::Stable, "0.11.0"));
        assert!(reloaded.contains(&UpdateChannel::Stable, "0.12.0"));
    }

    #[tokio::test]
    async fn persist_creates_missing_parent_dir() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("nested/sub/last_notified_update.json");

        let mut store = LastNotifiedUpdateStore::default();
        store
            .record(UpdateChannel::Rc, "0.12.0-rc.1".into(), &nested)
            .await
            .unwrap();

        assert!(nested.exists());
        let reloaded = LastNotifiedUpdateStore::load(&nested).await;
        assert!(reloaded.contains(&UpdateChannel::Rc, "0.12.0-rc.1"));
    }

    #[test]
    fn channel_key_uses_snake_case() {
        // 锁住 channel key wire 形态：snake_case，与 uc-core settings model
        // 的 serde rename 保持一致
        assert_eq!(channel_key(&UpdateChannel::Stable), "stable");
        assert_eq!(channel_key(&UpdateChannel::Alpha), "alpha");
        assert_eq!(channel_key(&UpdateChannel::Beta), "beta");
        assert_eq!(channel_key(&UpdateChannel::Rc), "rc");
    }

    #[tokio::test]
    async fn wire_format_uses_flat_map() {
        // 锁住磁盘上 JSON 形态：top-level flat map { "<channel>": "<version>" }
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);

        let mut store = LastNotifiedUpdateStore::default();
        store
            .record(UpdateChannel::Stable, "0.12.0".into(), &path)
            .await
            .unwrap();
        store
            .record(UpdateChannel::Alpha, "0.13.0-alpha.1".into(), &path)
            .await
            .unwrap();

        let raw = fs::read_to_string(&path).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["stable"], "0.12.0");
        assert_eq!(parsed["alpha"], "0.13.0-alpha.1");
    }
}
