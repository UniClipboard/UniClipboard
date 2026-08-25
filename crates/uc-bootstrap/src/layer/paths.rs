//! Desktop host path resolution.

use std::path::PathBuf;

use uc_app_paths::DesktopRuntimeProfileConfig;
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::ports::{AppDirs, AppDirsProvider};

use crate::wiring::error::{WiringError, WiringResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DesktopHostPaths {
    pub(crate) db_path: PathBuf,
    pub(crate) vault_dir: PathBuf,
    pub(crate) settings_path: PathBuf,
    pub(crate) logs_dir: PathBuf,
    pub(crate) cache_dir: PathBuf,
    pub(crate) app_data_root_dir: PathBuf,
}

impl DesktopHostPaths {
    fn from_app_dirs(dirs: AppDirs) -> Self {
        let cache_dir = if dirs.app_cache_root == dirs.app_data_root {
            dirs.app_data_root.join("cache")
        } else {
            dirs.app_cache_root
        };

        Self {
            db_path: dirs.app_data_root.join("uniclipboard.db"),
            vault_dir: dirs.app_data_root.join("vault"),
            settings_path: dirs.app_data_root.join("settings.json"),
            logs_dir: dirs.app_log_dir,
            cache_dir,
            app_data_root_dir: dirs.app_data_root,
        }
    }

    pub(crate) fn from_profile_config(config: &DesktopRuntimeProfileConfig) -> Self {
        Self {
            db_path: config.data_root().join("uniclipboard.db"),
            vault_dir: config.data_root().join("vault"),
            settings_path: config.data_root().join("settings.json"),
            logs_dir: config.log_dir().to_path_buf(),
            cache_dir: config.cache_root().to_path_buf(),
            app_data_root_dir: config.data_root().to_path_buf(),
        }
    }

    pub(crate) fn device_id_path(&self) -> PathBuf {
        self.vault_dir.join("device_id.txt")
    }
}

pub(crate) fn resolve_desktop_host_paths() -> WiringResult<DesktopHostPaths> {
    DirsAppDirsAdapter::new()
        .get_app_dirs()
        .map(DesktopHostPaths::from_app_dirs)
        .map_err(|error| WiringError::ConfigInit(error.to_string()))
}
