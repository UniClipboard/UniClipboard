use std::path::PathBuf;

use uc_core::app_dirs::AppDirs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub db_path: PathBuf,
    pub vault_dir: PathBuf,
    pub settings_path: PathBuf,
    pub logs_dir: PathBuf,
    pub cache_dir: PathBuf,
    /// Subdirectory for inbound file transfer cache.
    pub file_cache_dir: PathBuf,
    /// Subdirectory for representation spool (disk staging).
    pub spool_dir: PathBuf,
    /// The resolved application data root directory.
    pub app_data_root_dir: PathBuf,
}

impl AppPaths {
    /// Constructs an AppPaths instance whose file and directory locations are rooted at the provided AppDirs' `app_data_root`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use uc_core::app_dirs::AppDirs;
    /// use uc_app::app_paths::AppPaths;
    ///
    /// let dirs = AppDirs {
    ///     app_data_root: PathBuf::from("/tmp/uniclipboard"),
    ///     app_cache_root: PathBuf::from("/tmp/uniclipboard-cache"),
    /// };
    /// let paths = AppPaths::from_app_dirs(&dirs);
    ///
    /// assert_eq!(paths.db_path, PathBuf::from("/tmp/uniclipboard/uniclipboard.db"));
    /// assert_eq!(paths.vault_dir, PathBuf::from("/tmp/uniclipboard/vault"));
    /// assert_eq!(paths.settings_path, PathBuf::from("/tmp/uniclipboard/settings.json"));
    /// assert_eq!(paths.logs_dir, PathBuf::from("/tmp/uniclipboard/logs"));
    /// ```
    /// Returns the path to the encryption initialization marker file.
    pub fn encryption_marker_path(&self) -> PathBuf {
        self.vault_dir.join(".initialized_encryption")
    }

    /// Returns the path to the persisted local device identifier.
    pub fn device_id_path(&self) -> PathBuf {
        self.vault_dir.join("device_id.txt")
    }

    pub fn daemon_token_path(&self) -> PathBuf {
        self.app_data_root_dir.join(".daemon-token")
    }

    pub fn daemon_pid_path(&self) -> PathBuf {
        self.app_data_root_dir.join(".daemon-pid")
    }

    /// Single source of truth for all application paths.
    /// All subdirectory names are defined here — consumers must use these
    /// fields instead of calling `.join("...")` with hardcoded names.
    pub fn from_app_dirs(dirs: &AppDirs) -> Self {
        Self {
            db_path: dirs.app_data_root.join("uniclipboard.db"),
            vault_dir: dirs.app_data_root.join("vault"),
            settings_path: dirs.app_data_root.join("settings.json"),
            logs_dir: dirs.app_data_root.join("logs"),
            cache_dir: dirs.app_cache_root.clone(),
            file_cache_dir: dirs.app_cache_root.join("file-cache"),
            spool_dir: dirs.app_cache_root.join("spool"),
            app_data_root_dir: dirs.app_data_root.clone(),
        }
    }

    /// Constructs an AppPaths with the given base directory (for testing).
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uc_core::app_dirs::AppDirs;

    #[test]
    fn app_paths_derives_concrete_locations_from_app_data_root() {
        let dirs = AppDirs {
            app_data_root: PathBuf::from("/tmp/uniclipboard"),
            app_cache_root: PathBuf::from("/tmp/uniclipboard-cache"),
        };

        let paths = AppPaths::from_app_dirs(&dirs);

        assert_eq!(
            paths.db_path,
            PathBuf::from("/tmp/uniclipboard/uniclipboard.db")
        );
        assert_eq!(paths.vault_dir, PathBuf::from("/tmp/uniclipboard/vault"));
        assert_eq!(
            paths.settings_path,
            PathBuf::from("/tmp/uniclipboard/settings.json")
        );
        assert_eq!(paths.logs_dir, PathBuf::from("/tmp/uniclipboard/logs"));
        assert_eq!(paths.app_data_root_dir, PathBuf::from("/tmp/uniclipboard"));
        assert_eq!(
            paths.daemon_token_path(),
            PathBuf::from("/tmp/uniclipboard/.daemon-token")
        );
        assert_eq!(
            paths.device_id_path(),
            PathBuf::from("/tmp/uniclipboard/vault/device_id.txt")
        );
    }

    #[test]
    fn app_paths_includes_cache_dir() {
        let dirs = AppDirs {
            app_data_root: PathBuf::from("/tmp/uniclipboard"),
            app_cache_root: PathBuf::from("/tmp/uniclipboard-cache"),
        };
        let paths = AppPaths::from_app_dirs(&dirs);
        assert_eq!(paths.cache_dir, PathBuf::from("/tmp/uniclipboard-cache"));
        assert_eq!(
            paths.file_cache_dir,
            PathBuf::from("/tmp/uniclipboard-cache/file-cache")
        );
        assert_eq!(
            paths.spool_dir,
            PathBuf::from("/tmp/uniclipboard-cache/spool")
        );
        assert_eq!(paths.app_data_root_dir, PathBuf::from("/tmp/uniclipboard"));
    }
}
