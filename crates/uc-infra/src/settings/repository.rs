use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::fs;
use tokio::sync::RwLock;
use uc_core::{
    ports::SettingsPort,
    settings::model::{Settings, CURRENT_SCHEMA_VERSION},
};

use crate::settings::migration::SettingsMigrator;

pub struct FileSettingsRepository {
    path: PathBuf,
}

pub struct CachedSettingsRepository {
    inner: Arc<dyn SettingsPort>,
    cache: RwLock<Option<Settings>>,
}

impl FileSettingsRepository {
    /// Creates a FileSettingsRepository configured to use the given file path as the settings file.
    ///
    /// The provided `path` is converted into a `PathBuf` and stored as the repository's settings file location.
    ///
    /// # Examples
    ///
    /// ```
    /// # use uc_infra::settings::repository::FileSettingsRepository;
    /// let _repo = FileSettingsRepository::new("config/settings.json");
    /// ```
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Get the parent directory of the repository's settings file path.
    ///
    /// # Returns
    ///
    /// `Some(&Path)` with the parent directory, or `None` if the path has no parent.
    fn dir(&self) -> Option<&Path> {
        self.path.parent()
    }

    /// Ensure the repository's parent directory exists, creating it if necessary.
    ///
    /// Creates all missing parent directories for the repository's configured settings path.
    /// If the repository has no parent directory (e.g., path is in the filesystem root), this is a no-op.
    ///
    /// # Returns
    ///
    /// `Ok(())` on success; an error with context `create settings dir failed: {dir}` if directory creation fails.
    async fn ensure_parent_dir(&self) -> Result<()> {
        if let Some(dir) = self.dir() {
            fs::create_dir_all(dir)
                .await
                .with_context(|| format!("create settings dir failed: {}", dir.display()))?;
        }
        Ok(())
    }

    /// Atomically writes the given JSON content to the repository's settings file.
    ///
    /// The content is written to a temporary file adjacent to the target file and then
    /// renamed to the target path, ensuring the target is either the previous contents
    /// or the fully written new contents on success.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the write-and-rename completed successfully, `Err` with context if
    /// directory creation, writing the temporary file, or renaming the file failed.
    async fn atomic_write(&self, content: &str) -> Result<()> {
        self.ensure_parent_dir().await?;

        let tmp_path = self.path.with_extension("json.tmp");
        fs::write(&tmp_path, content)
            .await
            .with_context(|| format!("write temp settings failed: {}", tmp_path.display()))?;

        #[cfg(windows)]
        {
            // On Windows, remove target first to ensure rename succeeds
            if self.path.exists() {
                fs::remove_file(&self.path).await.ok(); // Ignore error if file doesn't exist
            }
            fs::rename(&tmp_path, &self.path).await.with_context(|| {
                format!(
                    "rename temp settings to target failed: {} -> {}",
                    tmp_path.display(),
                    self.path.display()
                )
            })?;
        }
        #[cfg(not(windows))]
        {
            fs::rename(&tmp_path, &self.path).await.with_context(|| {
                format!(
                    "rename temp settings to target failed: {} -> {}",
                    tmp_path.display(),
                    self.path.display()
                )
            })?;
        }
        Ok(())
    }
}

fn deserialize_and_migrate_settings(content: &str) -> Result<(Settings, u32)> {
    let settings: Settings = serde_json::from_str(content)?;
    let original_version = settings.schema_version;
    let migrator = SettingsMigrator::new();
    let migrated = migrator
        .migrate_to_latest(settings)
        .map_err(|e| anyhow::anyhow!("settings migration failed: {}", e))?;

    Ok((migrated, original_version))
}

/// Loads a read-only settings snapshot from disk using the canonical repository rules.
///
/// This shares the same deserialization, defaults, and migration logic as
/// [`FileSettingsRepository::load`], but does not persist migrated content
/// back to disk. It is intended for bootstrap-time reads that only need the
/// effective settings value.
pub fn load_settings_snapshot(path: &Path) -> Result<Settings> {
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings::default());
        }
        Err(e) => {
            return Err(e).with_context(|| format!("read settings failed: {}", path.display()))
        }
    };

    let (settings, _) = deserialize_and_migrate_settings(&content)?;
    Ok(settings)
}

#[async_trait]
impl SettingsPort for FileSettingsRepository {
    /// Loads settings from the repository path, migrates them to the latest schema, and persists migrated settings when necessary.
    ///
    /// If the settings file does not exist, returns `Settings::default()`. On success returns the migrated `Settings`. On failure returns an error for I/O or deserialization problems (read errors include context with the settings path).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use uc_infra::settings::repository::FileSettingsRepository;
    /// # use uc_core::ports::SettingsPort;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let repo = FileSettingsRepository::new(std::path::PathBuf::from("/tmp/nonexistent_settings.json"));
    /// let settings = repo.load().await?;
    /// // `settings` is either the migrated contents of the file or `Settings::default()` if the file was missing.
    /// # Ok(())
    /// # }
    /// ```
    async fn load(&self) -> Result<Settings> {
        let content = match fs::read_to_string(&self.path).await {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Settings::default());
            }
            Err(e) => {
                return Err(e)
                    .with_context(|| format!("read settings failed: {}", self.path.display()))
            }
        };

        let (migrated, original_version) = deserialize_and_migrate_settings(&content)?;

        if original_version < CURRENT_SCHEMA_VERSION {
            self.save(&migrated).await?;
        }

        Ok(migrated)
    }

    /// Persist settings as pretty-printed JSON to the repository's configured file.
    ///
    /// On success, the settings are written atomically to the underlying file.
    ///
    /// # Errors
    ///
    /// Returns an error with context `"serialize settings failed"` if JSON serialization fails,
    /// or an I/O error if writing the temporary file or renaming it to the target path fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use uc_infra::settings::repository::FileSettingsRepository;
    /// # use uc_core::settings::model::Settings;
    /// # use uc_core::ports::SettingsPort;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let repo = FileSettingsRepository::new(std::path::PathBuf::from("/tmp/settings.json"));
    /// let settings = Settings::default();
    /// repo.save(&settings).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn save(&self, settings: &Settings) -> Result<()> {
        let content =
            serde_json::to_string_pretty(settings).context("serialize settings failed")?;

        self.atomic_write(&content).await
    }
}

#[async_trait]
impl SettingsPort for CachedSettingsRepository {
    /// Loads settings from the repository path, migrates them to the latest schema, and persists migrated settings when necessary.
    ///
    /// If the settings file does not exist, returns `Settings::default()`. On success returns the migrated `Settings`. On failure returns an error for I/O or deserialization problems (read errors include context with the settings path).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use uc_infra::settings::repository::FileSettingsRepository;
    /// # use uc_core::ports::SettingsPort;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let repo = FileSettingsRepository::new(std::path::PathBuf::from("/tmp/nonexistent_settings.json"));
    /// let settings = repo.load().await?;
    /// // `settings` is either the migrated contents of the file or `Settings::default()` if the file was missing.
    /// # Ok(())
    /// # }
    /// ```
    async fn load(&self) -> Result<Settings> {
        let mut cache = self.cache.write().await;
        if let Some(settings) = cache.as_ref() {
            return Ok(settings.clone());
        }

        let settings = self.inner.load().await?;
        *cache = Some(settings.clone());
        Ok(settings)
    }

    /// Persist settings as pretty-printed JSON to the repository's configured file.
    ///
    /// On success, the settings are written atomically to the underlying file.
    ///
    /// # Errors
    ///
    /// Returns an error with context `"serialize settings failed"` if JSON serialization fails,
    /// or an I/O error if writing the temporary file or renaming it to the target path fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use uc_infra::settings::repository::FileSettingsRepository;
    /// # use uc_core::settings::model::Settings;
    /// # use uc_core::ports::SettingsPort;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let repo = FileSettingsRepository::new(std::path::PathBuf::from("/tmp/settings.json"));
    /// let settings = Settings::default();
    /// repo.save(&settings).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn save(&self, settings: &Settings) -> Result<()> {
        self.inner.save(&settings).await
    }
}
