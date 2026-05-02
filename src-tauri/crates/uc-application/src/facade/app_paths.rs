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
        // Windows 上 `dirs::cache_dir()` 直接返回 `dirs::data_local_dir()`,
        // 因此 `app_cache_root` 与 `app_data_root` 是同一个目录。若直接把
        // `cache_dir` 设为 `app_cache_root`, `StorageFacade::clear_cache` 在
        // Windows 上会清掉整个数据目录 (`vault/.setup_status`、`uniclipboard.db`、
        // `settings.json` 等), 表现为清缓存后下次启动回到 welcome 页。
        // 重合时退到 `app_data_root/cache` 子目录, 保证 cache 与持久化数据物理隔离。
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
        // Windows: dirs::cache_dir() == dirs::data_local_dir() ⇒ 两个 root 重合
        let root = PathBuf::from("/tmp/uc-test-root");
        let dirs = AppDirs {
            app_data_root: root.clone(),
            app_cache_root: root.clone(),
        };
        let paths = AppPaths::from_app_dirs(&dirs);

        assert_eq!(paths.cache_dir, root.join("cache"));
        assert_eq!(paths.spool_dir, root.join("cache").join("spool"));

        // 关键 invariant: clear_cache 沿 cache_dir 递归删除时不能波及任何持久化数据
        assert_disjoint(&paths.cache_dir, "vault_dir", &paths.vault_dir);
        assert_disjoint(&paths.cache_dir, "db_path", &paths.db_path);
        assert_disjoint(&paths.cache_dir, "settings_path", &paths.settings_path);
        assert_disjoint(&paths.cache_dir, "logs_dir", &paths.logs_dir);
        assert_disjoint(&paths.cache_dir, "file_cache_dir", &paths.file_cache_dir);
    }

    #[test]
    fn cache_dir_uses_dedicated_root_when_provided() {
        // macOS / Linux: cache root 与 data root 是不同目录
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
