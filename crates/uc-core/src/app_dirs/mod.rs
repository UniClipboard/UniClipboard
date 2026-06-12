use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    pub app_data_root: PathBuf,
    pub app_cache_root: PathBuf,
}

/// Derived filesystem paths for all application subsystems.
///
/// Constructed from [`AppDirs`] (platform-specific roots) and provides
/// concrete paths for the database, vault, settings, logs, cache, etc.
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

    pub fn last_notified_update_path(&self) -> PathBuf {
        self.app_data_root_dir.join("last_notified_update.json")
    }

    pub fn skipped_version_path(&self) -> PathBuf {
        self.app_data_root_dir.join("skipped_version.json")
    }

    pub fn from_app_dirs(dirs: &AppDirs) -> Self {
        // Windows: `dirs::cache_dir()` returns `dirs::data_local_dir()`,
        // so `app_cache_root` and `app_data_root` collide. If we used
        // `app_cache_root` as `cache_dir`, `clear_cache` would wipe the
        // entire data directory. Fall back to a `cache` subdirectory when
        // the roots coincide.
        let cache_root = if dirs.app_cache_root == dirs.app_data_root {
            dirs.app_data_root.join("cache")
        } else {
            dirs.app_cache_root.clone()
        };

        Self {
            db_path: dirs.app_data_root.join("uniclipboard.db"),
            vault_dir: dirs.app_data_root.join("vault"),
            settings_path: dirs.app_data_root.join("settings.json"),
            logs_dir: dirs.app_data_root.join("logs"),
            cache_dir: cache_root.clone(),
            file_cache_dir: dirs.app_data_root.join("file-cache"),
            spool_dir: cache_root.join("spool"),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_disjoint(parent: &PathBuf, child_label: &str, candidate: &PathBuf) {
        assert!(
            !candidate.starts_with(parent),
            "{child_label} ({candidate:?}) must not live inside cache_dir ({parent:?})"
        );
    }

    #[test]
    fn cache_dir_separated_when_data_and_cache_roots_collide() {
        // Windows: dirs::cache_dir() == dirs::data_local_dir()
        let root = PathBuf::from("/tmp/uc-test-root");
        let dirs = AppDirs {
            app_data_root: root.clone(),
            app_cache_root: root.clone(),
        };
        let paths = AppPaths::from_app_dirs(&dirs);

        assert_eq!(paths.cache_dir, root.join("cache"));
        assert_eq!(paths.spool_dir, root.join("cache").join("spool"));

        assert_disjoint(&paths.cache_dir, "vault_dir", &paths.vault_dir);
        assert_disjoint(&paths.cache_dir, "db_path", &paths.db_path);
        assert_disjoint(&paths.cache_dir, "settings_path", &paths.settings_path);
        assert_disjoint(&paths.cache_dir, "logs_dir", &paths.logs_dir);
        assert_disjoint(&paths.cache_dir, "file_cache_dir", &paths.file_cache_dir);
    }

    #[test]
    fn cache_dir_uses_dedicated_root_when_provided() {
        let data = PathBuf::from("/tmp/uc-data");
        let cache = PathBuf::from("/tmp/uc-cache");
        let dirs = AppDirs {
            app_data_root: data.clone(),
            app_cache_root: cache.clone(),
        };
        let paths = AppPaths::from_app_dirs(&dirs);

        assert_eq!(paths.cache_dir, cache);
        assert_eq!(paths.spool_dir, cache.join("spool"));
        assert_eq!(paths.vault_dir, data.join("vault"));
        assert_eq!(paths.app_data_root_dir, data);
    }
}
