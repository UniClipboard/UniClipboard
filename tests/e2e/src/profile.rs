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
        Self::from_unique_name(unique)
    }

    /// Create a fresh development profile for v0.19.1 upgrade compatibility tests.
    pub fn new_v0_19_1_upgrade(test_name: &str) -> Self {
        let unique = format!(
            "dev-upgrade-v0191-{}-{}",
            test_name,
            uuid::Uuid::new_v4().as_simple()
        );
        Self::from_unique_name(unique)
    }

    pub fn for_upgrade_fixture(profile_name: &str) -> Result<Self, String> {
        let development_prefix = ["dev-", "e2e-", "wdio-"]
            .into_iter()
            .any(|prefix| profile_name.starts_with(prefix));
        if !development_prefix
            || profile_name.is_empty()
            || profile_name.starts_with('.')
            || !profile_name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err("upgrade fixture profile name is invalid".to_string());
        }
        Ok(Self::from_unique_name(profile_name.to_string()))
    }

    fn from_unique_name(unique: String) -> Self {
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

    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
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
        uc_app_paths::app_log_dir_for_profile(Some(profile))
            .expect("the platform must provide an E2E log directory")
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

#[cfg(test)]
mod tests {
    use super::TestProfile;

    #[test]
    fn upgrade_fixture_restore_rejects_the_default_profile() {
        match TestProfile::for_upgrade_fixture("default") {
            Err(_) => {}
            Ok(profile) => {
                std::mem::forget(profile);
                panic!("default profile must never be a fixture restore target");
            }
        }
    }
}
