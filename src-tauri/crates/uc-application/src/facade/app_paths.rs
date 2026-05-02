use std::path::PathBuf;

use uc_core::app_dirs::AppDirs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub db_path: PathBuf,
    pub vault_dir: PathBuf,
    pub settings_path: PathBuf,
    pub logs_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub file_cache_dir: PathBuf,
    pub spool_dir: PathBuf,
    pub app_data_root_dir: PathBuf,
}

impl AppPaths {
    pub fn encryption_marker_path(&self) -> PathBuf {
        self.vault_dir.join(".initialized_encryption")
    }

    pub fn device_id_path(&self) -> PathBuf {
        self.vault_dir.join("device_id.txt")
    }

    pub fn daemon_token_path(&self) -> PathBuf {
        self.app_data_root_dir.join(".daemon-token")
    }

    pub fn daemon_pid_path(&self) -> PathBuf {
        self.app_data_root_dir.join(".daemon-pid")
    }

    pub fn from_app_dirs(dirs: &AppDirs) -> Self {
        Self {
            db_path: dirs.app_data_root.join("uniclipboard.db"),
            vault_dir: dirs.app_data_root.join("vault"),
            settings_path: dirs.app_data_root.join("settings.json"),
            logs_dir: dirs.app_data_root.join("logs"),
            cache_dir: dirs.app_cache_root.clone(),
            file_cache_dir: dirs.app_data_root.join("file-cache"),
            spool_dir: dirs.app_cache_root.join("spool"),
            app_data_root_dir: dirs.app_data_root.clone(),
        }
    }

    pub fn with_base_data_local_dir(base: PathBuf) -> Self {
        let cache_base = base
            .parent()
            .map(|p| p.join("cache"))
            .unwrap_or_else(|| base.clone());
        let dirs = AppDirs {
            app_data_root: base.clone(),
            app_cache_root: cache_base,
        };
        Self::from_app_dirs(&dirs)
    }
}
