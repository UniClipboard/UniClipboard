//! Inlined file-backed port implementations for the pure-client GUI.
//!
//! These are minimal implementations of [`SettingsPort`], [`SetupStatusPort`],
//! and [`DeviceIdentityPort`] that read/write JSON files — no sqlite, no iroh,
//! no heavyweight infra dependency.  The GUI client only needs these three
//! file-backed ports; all business state lives in the external `uniclipd`
//! daemon.
//!
//! The implementations mirror the uc-infra originals (`FileSettingsRepository`,
//! `FileSetupStatusRepository`, `LocalDeviceIdentity`) but are intentionally
//! self-contained so that `uc-desktop` does not pull in uc-infra's transitive
//! dependency tree (iroh / diesel / sqlite).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uc_core::ids::DeviceId;
use uc_core::ports::{DeviceIdentityPort, SettingsPort, SetupStatusPort};
use uc_core::settings::model::{Settings, CURRENT_SCHEMA_VERSION};
use uc_core::setup::SetupStatus;

// ---------------------------------------------------------------------------
// FileSettingsRepository
// ---------------------------------------------------------------------------

/// File-backed settings repository (inlined for uc-desktop).
///
/// Reads and writes `settings.json` as pretty-printed JSON.  On load, if the
/// persisted `schema_version` is below [`CURRENT_SCHEMA_VERSION`] the settings
/// are re-saved (currently a no-op since CURRENT_SCHEMA_VERSION == 1 and there
/// are zero registered migrations).
pub struct FileSettingsRepository {
    path: PathBuf,
}

impl FileSettingsRepository {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    fn dir(&self) -> Option<&Path> {
        self.path.parent()
    }

    async fn ensure_parent_dir(&self) -> Result<()> {
        if let Some(dir) = self.dir() {
            fs::create_dir_all(dir)
                .await
                .with_context(|| format!("create settings dir failed: {}", dir.display()))?;
        }
        Ok(())
    }

    /// Atomically write content via tmp-file + rename.
    async fn atomic_write(&self, content: &str) -> Result<()> {
        self.ensure_parent_dir().await?;

        let tmp_path = self.path.with_extension("json.tmp");
        fs::write(&tmp_path, content)
            .await
            .with_context(|| format!("write temp settings failed: {}", tmp_path.display()))?;

        #[cfg(windows)]
        {
            if self.path.exists() {
                fs::remove_file(&self.path).await.ok();
            }
        }

        fs::rename(&tmp_path, &self.path).await.with_context(|| {
            format!(
                "rename temp settings to target failed: {} -> {}",
                tmp_path.display(),
                self.path.display()
            )
        })?;

        Ok(())
    }
}

#[async_trait]
impl SettingsPort for FileSettingsRepository {
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

        let settings: Settings =
            serde_json::from_str(&content).context("deserialize settings failed")?;
        let original_version = settings.schema_version;

        // Currently CURRENT_SCHEMA_VERSION == 1 and there are zero migrations,
        // so `settings` is already at the latest version.  If we ever add
        // migrations, this code path will need to apply them inline (or call
        // out to a shared migration helper).  For now just re-save when the
        // version is stale so the file reflects the current schema.
        if original_version < CURRENT_SCHEMA_VERSION {
            self.save(&settings).await?;
        }

        Ok(settings)
    }

    async fn save(&self, settings: &Settings) -> Result<()> {
        let content =
            serde_json::to_string_pretty(settings).context("serialize settings failed")?;
        self.atomic_write(&content).await
    }
}

// ---------------------------------------------------------------------------
// FileSetupStatusRepository
// ---------------------------------------------------------------------------

const DEFAULT_SETUP_STATUS_FILE: &str = ".setup_status";

/// File-backed setup status repository (inlined for uc-desktop).
pub struct FileSetupStatusRepository {
    status_file_path: PathBuf,
}

impl FileSetupStatusRepository {
    /// Create repository with the vault directory's default filename.
    pub fn with_defaults(base_dir: PathBuf) -> Self {
        Self {
            status_file_path: base_dir.join(DEFAULT_SETUP_STATUS_FILE),
        }
    }

    async fn ensure_parent_dir(&self) -> Result<()> {
        if let Some(parent) = self.status_file_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl SetupStatusPort for FileSetupStatusRepository {
    async fn get_status(&self) -> Result<SetupStatus> {
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

    async fn set_status(&self, status: &SetupStatus) -> Result<()> {
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

// ---------------------------------------------------------------------------
// LocalDeviceIdentity
// ---------------------------------------------------------------------------

const DEVICE_ID_FILE: &str = "device_id.txt";

/// Local filesystem-backed device identity (inlined for uc-desktop).
///
/// Stores the device ID as a plain-text UUID in the application data directory.
/// Immutable once created.
pub struct LocalDeviceIdentity {
    device_id: DeviceId,
}

impl LocalDeviceIdentity {
    /// Load existing device ID from disk or create a new one.
    pub fn load_or_create(config_dir: PathBuf) -> Result<Self> {
        if let Some(id) = load_device_id_from_disk(&config_dir)? {
            Ok(Self { device_id: id })
        } else {
            let id = DeviceId::new(uuid::Uuid::new_v4().to_string());
            save_device_id_to_disk(&config_dir, &id)?;
            Ok(Self { device_id: id })
        }
    }
}

impl DeviceIdentityPort for LocalDeviceIdentity {
    fn current_device_id(&self) -> DeviceId {
        self.device_id.clone()
    }
}

fn load_device_id_from_disk(config_dir: &Path) -> Result<Option<DeviceId>> {
    let path = config_dir.join(DEVICE_ID_FILE);

    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("read device_id file failed: {}", path.display()))?;

    let id_str = content.trim();
    if id_str.is_empty() {
        return Ok(None);
    }

    // Validate UUID format
    uuid::Uuid::parse_str(id_str)
        .with_context(|| format!("invalid device_id UUID in file: {}", path.display()))?;

    Ok(Some(DeviceId::new(id_str.to_string())))
}

fn save_device_id_to_disk(config_dir: &Path, id: &DeviceId) -> Result<()> {
    std::fs::create_dir_all(config_dir)
        .with_context(|| format!("create config dir failed: {}", config_dir.display()))?;

    let path = config_dir.join(DEVICE_ID_FILE);

    // Atomic write via temp file + rename, with direct-write fallback.
    let tmp_path = path.with_extension("txt.tmp");
    std::fs::write(&tmp_path, id.as_str())
        .with_context(|| format!("write temp device_id failed: {}", tmp_path.display()))?;

    match std::fs::rename(&tmp_path, &path) {
        Ok(_) => Ok(()),
        Err(rename_err) => {
            std::fs::write(&path, id.as_str()).with_context(|| {
                format!(
                    "direct write device_id failed after rename error ({}): {}",
                    rename_err,
                    path.display()
                )
            })?;
            let _ = std::fs::remove_file(&tmp_path);
            Ok(())
        }
    }
}
