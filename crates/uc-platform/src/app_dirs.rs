use std::path::PathBuf;

use crate::ports::AppDirsPort;
use uc_core::app_dirs::AppDirs;
use uc_core::ports::AppDirsError;

/// Constructs the application directory name, appending a profile suffix when a profile is resolved.
///
/// Delegates the raw computation to [`uc_app_paths::resolved_app_dir_name`],
/// threading in `uc-platform`'s compile-time [`crate::default_profile`] (the
/// `dev-profile` feature) as the fallback. The result is
/// `app.uniclipboard.desktop` followed by `-<profile>` when a profile resolves,
/// otherwise the bare app directory name.
fn resolved_app_dir_name() -> String {
    uc_app_paths::resolved_app_dir_name(crate::default_profile())
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
        // Non-override resolution (portable redirect > dirs::data_local_dir())
        // is the directory-layout authority's job, so daemon/CLI and GUI follow
        // one source. The test-only override short-circuits before it.
        uc_app_paths::base_data_local_dir()
    }

    fn base_cache_dir(&self) -> Option<PathBuf> {
        if let Some(base) = &self.base_data_local_dir_override {
            return Some(base.clone());
        }
        uc_app_paths::base_cache_dir()
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
        // "Where logs live" has a single source of truth in `uc-app-paths`.
        // The test override redirects every directory under one base, so logs
        // follow it there (keeping tests off the real system log location);
        // production resolves the platform-conventional location upstream.
        let app_log_dir = match &self.base_data_local_dir_override {
            Some(base) => base.join(&self.cached_app_dir_name).join("logs"),
            None => uc_app_paths::app_log_dir().ok_or(AppDirsError::DataLocalDirUnavailable)?,
        };
        Ok(AppDirs {
            app_data_root: base_data.join(&self.cached_app_dir_name),
            app_cache_root: base_cache.join(&self.cached_app_dir_name),
            app_log_dir,
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
/// # use uc_platform::app_dirs::default_app_dirs;
/// let dirs = default_app_dirs().expect("failed to resolve app dirs");
/// // `app_data_root` and `app_cache_root` are absolute paths that include the app directory name.
/// assert!(dirs.app_data_root.to_string_lossy().contains("app.uniclipboard.desktop"));
/// ```
pub fn default_app_dirs() -> Result<AppDirs, AppDirsError> {
    DirsAppDirsAdapter::new().get_app_dirs()
}
