//! 持久化"用户主动跳过的版本"，按 update channel 分桶。
//!
//! 当用户在 updater 窗口点击"Skip This Version"时写入；
//! `notify_if_new_version` 在弹窗前检查——匹配则跳过通知。
//!
//! 实现同 `last_notified.rs`：单文件 JSON、自愈式 load、按 channel 分桶。

use std::{collections::HashMap, io, path::Path};

use serde::{Deserialize, Serialize};
use tokio::fs;
use uc_core::settings::model::UpdateChannel;

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct SkippedVersionStore {
    entries: HashMap<String, String>,
}

impl SkippedVersionStore {
    pub async fn load(path: &Path) -> Self {
        match fs::read(path).await {
            Ok(bytes) if bytes.is_empty() => Self::default(),
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                tracing::warn!(
                    target: "update_scheduler::skipped_version",
                    error = %err,
                    path = %path.display(),
                    "corrupted skipped_version.json; treating as empty"
                );
                Self::default()
            }),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(err) => {
                tracing::warn!(
                    target: "update_scheduler::skipped_version",
                    error = %err,
                    path = %path.display(),
                    "failed to read skipped_version.json; treating as empty"
                );
                Self::default()
            }
        }
    }

    pub fn is_skipped(&self, channel: &UpdateChannel, version: &str) -> bool {
        self.entries
            .get(channel_key(channel))
            .is_some_and(|v| v == version)
    }

    pub async fn skip(
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
        dir.path().join("skipped_version.json")
    }

    #[tokio::test]
    async fn load_returns_empty_when_file_missing() {
        let dir = TempDir::new().unwrap();
        let store = SkippedVersionStore::load(&store_path(&dir)).await;
        assert_eq!(store, SkippedVersionStore::default());
        assert!(!store.is_skipped(&UpdateChannel::Stable, "0.12.0"));
    }

    #[tokio::test]
    async fn skip_persists_and_round_trips() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);

        let mut store = SkippedVersionStore::load(&path).await;
        store
            .skip(UpdateChannel::Stable, "0.15.0".into(), &path)
            .await
            .unwrap();

        let reloaded = SkippedVersionStore::load(&path).await;
        assert!(reloaded.is_skipped(&UpdateChannel::Stable, "0.15.0"));
        assert!(!reloaded.is_skipped(&UpdateChannel::Stable, "0.16.0"));
    }

    #[tokio::test]
    async fn skip_overwrites_previous_for_same_channel() {
        let dir = TempDir::new().unwrap();
        let path = store_path(&dir);

        let mut store = SkippedVersionStore::default();
        store
            .skip(UpdateChannel::Stable, "0.14.0".into(), &path)
            .await
            .unwrap();
        store
            .skip(UpdateChannel::Stable, "0.15.0".into(), &path)
            .await
            .unwrap();

        let reloaded = SkippedVersionStore::load(&path).await;
        assert!(!reloaded.is_skipped(&UpdateChannel::Stable, "0.14.0"));
        assert!(reloaded.is_skipped(&UpdateChannel::Stable, "0.15.0"));
    }
}
