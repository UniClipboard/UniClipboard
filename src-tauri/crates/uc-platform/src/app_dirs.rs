use std::path::PathBuf;

use crate::ports::AppDirsPort;
use uc_core::app_dirs::AppDirs;
use uc_core::ports::AppDirsError;

const APP_DIR_NAME: &str = "app.uniclipboard.desktop";

fn resolved_app_dir_name() -> String {
    match std::env::var("UC_PROFILE") {
        Ok(profile) if !profile.is_empty() => format!("{APP_DIR_NAME}-{profile}"),
        _ => APP_DIR_NAME.to_string(),
    }
}

pub struct DirsAppDirsAdapter {
    base_data_local_dir_override: Option<PathBuf>,
    cached_app_dir_name: String,
}

impl DirsAppDirsAdapter {
    /// Creates a new DirsAppDirsAdapter with no base data directory override.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_platform::app_dirs::DirsAppDirsAdapter;
    /// let _ = DirsAppDirsAdapter::new();
    /// ```
    pub fn new() -> Self {
        Self {
            base_data_local_dir_override: None,
            cached_app_dir_name: resolved_app_dir_name(),
        }
    }

    /// Creates an adapter that overrides the base local data directory.
    ///
    /// The provided `base` path will be used instead of the system data local directory
    /// when resolving application directories for this adapter.
    ///
    /// This is useful for testing, where you want to redirect paths to a temp directory.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use uc_platform::app_dirs::DirsAppDirsAdapter;
    ///
    /// let adapter = DirsAppDirsAdapter::with_base_data_local_dir(PathBuf::from("/tmp"));
    /// ```
    pub fn with_base_data_local_dir(base: PathBuf) -> Self {
        Self {
            base_data_local_dir_override: Some(base),
            cached_app_dir_name: resolved_app_dir_name(),
        }
    }

    /// Resolve the base local data directory used for application data.
    ///
    /// Returns `Some(PathBuf)` containing the overridden base directory if one was set when the
    /// adapter was constructed; otherwise returns the system data-local directory from `dirs::data_local_dir()`.
    /// Returns `None` if no override is set and the system data-local directory is unavailable.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_platform::app_dirs::DirsAppDirsAdapter;
    ///
    /// let adapter = DirsAppDirsAdapter::new();
    /// let _ = adapter.base_data_local_dir();
    /// ```
    pub fn base_data_local_dir(&self) -> Option<PathBuf> {
        if let Some(base) = &self.base_data_local_dir_override {
            return Some(base.clone());
        }
        dirs::data_local_dir()
    }

    fn base_cache_dir(&self) -> Option<PathBuf> {
        if let Some(base) = &self.base_data_local_dir_override {
            return Some(base.clone());
        }
        dirs::cache_dir()
    }
}

impl AppDirsPort for DirsAppDirsAdapter {
    /// Constructs the application's directories using the system (or overridden) local data directory.
    ///
    /// # Returns
    ///
    /// `AppDirs` with `app_data_root` set to the base local data directory joined with the
    /// value captured from `resolved_app_dir_name()` when this adapter is created.
    ///
    /// Depending on `UC_PROFILE`, `resolved_app_dir_name()` resolves to `"uniclipboard"`
    /// or `"uniclipboard-{profile}"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_platform::ports::AppDirsPort;
    /// use uc_platform::app_dirs::DirsAppDirsAdapter;
    ///
    /// let adapter = DirsAppDirsAdapter::new();
    /// let _ = adapter.get_app_dirs();
    /// ```
    fn get_app_dirs(&self) -> Result<AppDirs, AppDirsError> {
        let base_data = self
            .base_data_local_dir()
            .ok_or(AppDirsError::DataLocalDirUnavailable)?;
        let base_cache = self
            .base_cache_dir()
            .ok_or(AppDirsError::CacheDirUnavailable)?;
        Ok(AppDirs {
            app_data_root: base_data.join(&self.cached_app_dir_name),
            app_cache_root: base_cache.join(&self.cached_app_dir_name),
        })
    }
}

/// Resolve the application's data and cache directories for the current environment.
///
/// Uses the system's base data/cache directories (or the adapter override) and appends the
/// configured application directory name, which includes the `UC_PROFILE` suffix when set.
///
/// # Examples
///
/// ```
/// # use uc_platform::app_dirs::{default_app_dirs, AppDirs};
/// let dirs = default_app_dirs().expect("failed to resolve app dirs");
/// // `app_data_root` and `app_cache_root` are absolute paths that include the app directory name.
/// assert!(dirs.app_data_root.to_string_lossy().contains("app.uniclipboard.desktop"));
/// ```
pub fn default_app_dirs() -> Result<AppDirs, AppDirsError> {
    DirsAppDirsAdapter::new().get_app_dirs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::AppDirsPort;
    use crate::test_support::with_uc_profile;

    /// Verifies that the adapter appends the `uniclipboard` directory name to the base data directory.
    ///
    /// # Examples
    ///
    /// ```
    /// let adapter = DirsAppDirsAdapter::with_base_data_local_dir(std::path::PathBuf::from("/tmp"));
    /// let dirs = adapter.get_app_dirs().unwrap();
    /// assert_eq!(dirs.app_data_root, std::path::PathBuf::from("/tmp/app.uniclipboard.desktop"));
    /// ```
    #[test]
    fn adapter_appends_uniclipboard_dir_name() {
        with_uc_profile(None, || {
            let adapter =
                DirsAppDirsAdapter::with_base_data_local_dir(std::path::PathBuf::from("/tmp"));
            let dirs = adapter.get_app_dirs().unwrap();
            assert_eq!(
                dirs.app_data_root,
                std::path::PathBuf::from("/tmp/app.uniclipboard.desktop")
            );
        });
    }

    #[test]
    fn adapter_sets_cache_root() {
        with_uc_profile(None, || {
            let adapter = DirsAppDirsAdapter::with_base_data_local_dir(PathBuf::from("/tmp"));
            let dirs = adapter.get_app_dirs().unwrap();
            assert!(dirs.app_cache_root.ends_with("app.uniclipboard.desktop"));
        });
    }

    /// Ensures that the adapter produces distinct app data and cache directories when `UC_PROFILE` differs.
    ///
    /// # Examples
    ///
    /// ```
    /// let dirs_a = with_uc_profile(Some("a"), || {
    ///     let adapter = DirsAppDirsAdapter::with_base_data_local_dir(PathBuf::from("/tmp"));
    ///     adapter.get_app_dirs().unwrap()
    /// });
    /// let dirs_b = with_uc_profile(Some("b"), || {
    ///     let adapter = DirsAppDirsAdapter::with_base_data_local_dir(PathBuf::from("/tmp"));
    ///     adapter.get_app_dirs().unwrap()
    /// });
    ///
    /// assert_eq!(dirs_a.app_data_root, PathBuf::from("/tmp/app.uniclipboard.desktop-a"));
    /// assert_eq!(dirs_b.app_data_root, PathBuf::from("/tmp/app.uniclipboard.desktop-b"));
    /// assert_ne!(dirs_a.app_data_root, dirs_b.app_data_root);
    /// assert_eq!(dirs_a.app_cache_root, PathBuf::from("/tmp/app.uniclipboard.desktop-a"));
    /// assert_eq!(dirs_b.app_cache_root, PathBuf::from("/tmp/app.uniclipboard.desktop-b"));
    /// assert_ne!(dirs_a.app_cache_root, dirs_b.app_cache_root);
    /// ```
    #[test]
    fn adapter_isolates_dirs_for_different_uc_profile_values() {
        let dirs_a = with_uc_profile(Some("a"), || {
            let adapter = DirsAppDirsAdapter::with_base_data_local_dir(PathBuf::from("/tmp"));
            adapter.get_app_dirs().unwrap()
        });
        let dirs_b = with_uc_profile(Some("b"), || {
            let adapter = DirsAppDirsAdapter::with_base_data_local_dir(PathBuf::from("/tmp"));
            adapter.get_app_dirs().unwrap()
        });

        assert_eq!(
            dirs_a.app_data_root,
            PathBuf::from("/tmp/app.uniclipboard.desktop-a")
        );
        assert_eq!(
            dirs_b.app_data_root,
            PathBuf::from("/tmp/app.uniclipboard.desktop-b")
        );
        assert_ne!(dirs_a.app_data_root, dirs_b.app_data_root);
        assert_eq!(
            dirs_a.app_cache_root,
            PathBuf::from("/tmp/app.uniclipboard.desktop-a")
        );
        assert_eq!(
            dirs_b.app_cache_root,
            PathBuf::from("/tmp/app.uniclipboard.desktop-b")
        );
        assert_ne!(dirs_a.app_cache_root, dirs_b.app_cache_root);
    }
}
