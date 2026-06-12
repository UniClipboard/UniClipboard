//! Profile isolation for E2E tests.
//!
//! Each test gets a unique profile name so its daemon instance, data directory,
//! and socket are fully independent of other concurrent tests.

use std::path::PathBuf;

/// A unique test profile that isolates daemon data and socket paths.
pub struct TestProfile {
    pub name: String,
    data_dir: PathBuf,
}

impl TestProfile {
    /// Create a new test profile with a unique name derived from `test_name`.
    pub fn new(test_name: &str) -> Self {
        let unique = format!("e2e-{}-{}", test_name, uuid::Uuid::new_v4().as_simple());
        let data_dir = Self::resolve_data_dir(&unique);
        Self {
            name: unique,
            data_dir,
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

    /// Clean up the profile's data directory.
    pub fn cleanup(&self) {
        if self.data_dir.exists() {
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }
}

impl Drop for TestProfile {
    fn drop(&mut self) {
        self.cleanup();
    }
}
