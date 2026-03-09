use std::path::PathBuf;

use uc_core::app_dirs::AppDirs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub db_path: PathBuf,
    pub vault_dir: PathBuf,
    pub settings_path: PathBuf,
    pub logs_dir: PathBuf,
    pub cache_dir: PathBuf,
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
    pub fn from_app_dirs(dirs: &AppDirs) -> Self {
        Self {
            db_path: dirs.app_data_root.join("uniclipboard.db"),
            vault_dir: dirs.app_data_root.join("vault"),
            settings_path: dirs.app_data_root.join("settings.json"),
            logs_dir: dirs.app_data_root.join("logs"),
            cache_dir: dirs.app_cache_root.clone(),
        }
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
    }

    #[test]
    fn app_paths_includes_cache_dir() {
        let dirs = AppDirs {
            app_data_root: PathBuf::from("/tmp/uniclipboard"),
            app_cache_root: PathBuf::from("/tmp/uniclipboard-cache"),
        };
        let paths = AppPaths::from_app_dirs(&dirs);
        assert_eq!(paths.cache_dir, PathBuf::from("/tmp/uniclipboard-cache"));
    }
}
