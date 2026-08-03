//! Profile isolation for E2E tests.
//!
//! Each test gets a unique profile name so its daemon instance, data directory,
//! and socket are fully independent of other concurrent tests.

use std::path::PathBuf;

/// A unique test profile that isolates daemon data, cache, and socket paths.
pub struct TestProfile {
    pub name: String,
    data_dir: PathBuf,
    cache_dir: PathBuf,
    log_dir: PathBuf,
}

impl TestProfile {
    /// Create a new test profile with a unique name derived from `test_name`.
    pub fn new(test_name: &str) -> Self {
        let unique = format!("e2e-{}-{}", test_name, uuid::Uuid::new_v4().as_simple());
        let data_dir = Self::resolve_data_dir(&unique);
        let cache_dir = Self::resolve_cache_dir(&unique);
        let log_dir = Self::resolve_log_dir(&unique);
        Self {
            name: unique,
            data_dir,
            cache_dir,
            log_dir,
        }
    }

    /// Resolve the data directory for the given profile name.
    fn resolve_data_dir(profile: &str) -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            dirs_next::data_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(format!("app.uniclipboard.desktop-{}", profile))
        }
        #[cfg(target_os = "linux")]
        {
            dirs_next::data_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(format!("app.uniclipboard.desktop-{}", profile))
        }
        #[cfg(target_os = "windows")]
        {
            dirs_next::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("C:\\Temp"))
                .join(format!("app.uniclipboard.desktop-{}", profile))
        }
    }

    /// Path to the data directory for this profile.
    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    pub fn log_dir(&self) -> &PathBuf {
        &self.log_dir
    }

    pub fn process_log_path(&self) -> PathBuf {
        self.data_dir.join("e2e-daemon-process.log")
    }

    /// Resolve the cache directory for the given profile name. The daemon writes
    /// a cache dir (clipboard spool, blobs) separate from the data dir, under
    /// the OS cache root.
    fn resolve_cache_dir(profile: &str) -> PathBuf {
        dirs_next::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(format!("app.uniclipboard.desktop-{}", profile))
    }

    fn resolve_log_dir(profile: &str) -> PathBuf {
        let app_dir = format!("app.uniclipboard.desktop-{profile}");
        #[cfg(target_os = "macos")]
        {
            return dirs_next::home_dir()
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join("Library/Logs")
                .join(app_dir);
        }
        #[cfg(target_os = "linux")]
        {
            let state_root = std::env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    dirs_next::home_dir()
                        .unwrap_or_else(|| PathBuf::from("/tmp"))
                        .join(".local/state")
                });
            return state_root.join(app_dir).join("logs");
        }
        #[cfg(target_os = "windows")]
        {
            return dirs_next::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("C:\\Temp"))
                .join(app_dir)
                .join("logs");
        }
    }

    /// Remove every directory this profile's daemon may have created.
    ///
    /// The daemon writes BOTH a data dir and a separate cache dir (spool /
    /// blobs). Cleaning only the data dir leaked one cache dir per test run
    /// (`~/Library/Caches/...` on macOS), which accumulated unbounded.
    pub fn cleanup(&self) {
        for dir in [&self.data_dir, &self.cache_dir, &self.log_dir] {
            if dir.exists() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }
}

impl Drop for TestProfile {
    fn drop(&mut self) {
        if std::env::var_os("UC_E2E_KEEP_PROFILES").is_none() {
            self.cleanup();
        }
    }
}
