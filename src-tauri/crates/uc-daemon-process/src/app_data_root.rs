//! Self-contained resolution of the application data root.
//!
//! Reproduces the `app_data_root` that
//! `uc_platform::app_dirs::DirsAppDirsAdapter` / `default_app_dirs()` compute,
//! so the daemon PID file (`<app_data_root>/.daemon-pid`) and token file
//! (`<app_data_root>/.daemon-token`) land at the exact same paths as before.
//! The resolved path is **byte-identical on every success path**; the sole
//! divergence is an error path (point 5 below) that a booted daemon cannot reach.
//!
//! This module exists so the process-management helpers (`process_metadata`,
//! `socket`) own **zero** app-stack dependencies (`uc-application`,
//! `uc-platform`) — which is what lets them be extracted into a thin crate that
//! does not drag `iroh`/`diesel` into the CLI client stack (ADR-008 P5).
//!
//! The resolution mirrors three upstream sources verbatim:
//!   - `uc_platform::app_dirs` (`APP_DIR_NAME`, `resolved_app_dir_name`,
//!     `base_data_local_dir`),
//!   - `uc_platform::resolve_profile` / `default_profile`, and
//!   - `uc_platform::portable` (portable-mode detection).
//!
//! ## Why this is identical (zero behavior change)
//!
//! 1. **Same inputs**: reads the same signals — `UC_PROFILE`, `UC_PORTABLE`, a
//!    `portable.dat` marker next to the executable, `std::env::current_exe()`,
//!    and `dirs::data_local_dir()` from the **same** `dirs` 6.0 crate
//!    `uc-platform` pins.
//! 2. **Same composition**: `base.join(app_dir_name)` where
//!    `app_dir_name = "app.uniclipboard.desktop"[-<profile>]`.
//! 3. **Same precedence**: portable redirect > `dirs::data_local_dir()`.
//!    (The `DirsAppDirsAdapter::with_base_data_local_dir` override is only used
//!    by tests, never on the default daemon/CLI path, so it is intentionally
//!    not reproduced here.)
//! 4. **`dev-profile` parity**: `uc_platform::default_profile()` is `Some("dev")`
//!    only when the `dev-profile` feature is enabled, which is enabled **only**
//!    by `uc-tauri` — never by any binary that links these process-management
//!    modules via the CLI/daemon path. So in this graph `default_profile()` is
//!    always `None`, and the profile suffix comes purely from runtime
//!    `UC_PROFILE`. This module deliberately has no such feature, matching that.
//! 5. **Error-path note (the one non-identical case)**: upstream
//!    `DirsAppDirsAdapter::get_app_dirs()` also resolves `dirs::cache_dir()` and
//!    fails with `CacheDirUnavailable` if it is `None`, even though the data root
//!    never needs it. This module consults only `dirs::data_local_dir()`, so in
//!    the (macOS/Linux/Windows-impossible) case where the cache dir is `None` but
//!    the data dir is `Some`, the old path errored while this one succeeds. That
//!    state is unreachable for a *booted* daemon: the daemon itself boots via the
//!    same cache-requiring `get_default_app_dirs()`, so if the dirs truly diverged
//!    it could never have started and there is no PID/token file to find anyway.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Hardcoded application directory name (mirrors
/// `uc_platform::app_dirs::APP_DIR_NAME`).
const APP_DIR_NAME: &str = "app.uniclipboard.desktop";

/// Marker file placed next to the executable inside the portable zip (mirrors
/// `uc_platform::portable::PORTABLE_MARKER`). Its mere presence flips the
/// running binary into portable mode.
const PORTABLE_MARKER: &str = "portable.dat";

/// Subdirectory (relative to the executable) that holds all portable data
/// (mirrors `uc_platform::portable::PORTABLE_DATA_SUBDIR`).
const PORTABLE_DATA_SUBDIR: &str = "data";

/// Failure resolving the application data root.
///
/// Mirrors the two failure modes of `uc_core::ports::AppDirsError` that the
/// data-root path can hit: the base data-local directory being unavailable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppDataRootError {
    /// `dirs::data_local_dir()` returned `None` and portable mode is off.
    DataLocalDirUnavailable,
}

impl fmt::Display for AppDataRootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DataLocalDirUnavailable => {
                write!(f, "the system data-local directory is unavailable")
            }
        }
    }
}

impl std::error::Error for AppDataRootError {}

/// Resolve the active profile name (mirrors `uc_platform::resolve_profile`).
///
/// Runtime `UC_PROFILE` (when set **and** non-empty) wins; otherwise falls back
/// to the compile-time default profile, which is always `None` for this crate
/// (no `dev-profile` feature — see module docs).
fn resolve_profile() -> Option<String> {
    if let Ok(profile) = std::env::var("UC_PROFILE") {
        if !profile.is_empty() {
            return Some(profile);
        }
    }
    // `uc_platform::default_profile()` is `Some("dev")` only under the
    // `dev-profile` feature, which no CLI/daemon binary linking these modules
    // enables. Mirror that as an unconditional `None`.
    None
}

/// Constructs the application directory name, appending `-<profile>` when a
/// profile is resolved (mirrors `uc_platform::app_dirs::resolved_app_dir_name`).
fn resolved_app_dir_name() -> String {
    match resolve_profile() {
        Some(profile) => format!("{APP_DIR_NAME}-{profile}"),
        None => APP_DIR_NAME.to_string(),
    }
}

/// Read `UC_PORTABLE` and decide whether it forces portable mode on (mirrors
/// `uc_platform::portable::env_forces_portable`).
fn env_forces_portable() -> bool {
    match std::env::var("UC_PORTABLE") {
        Ok(value) => {
            let value = value.trim();
            value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")
        }
        Err(_) => false,
    }
}

/// Resolve the portable data root from an executable directory and an explicit
/// env override (mirrors `uc_platform::portable::resolve_portable_root`).
fn resolve_portable_root(exe_dir: &Path, env_forced: bool) -> Option<PathBuf> {
    if env_forced || exe_dir.join(PORTABLE_MARKER).is_file() {
        Some(exe_dir.join(PORTABLE_DATA_SUBDIR))
    } else {
        None
    }
}

/// Detect the portable data root for the running binary (mirrors
/// `uc_platform::portable::detect_portable_root`).
fn detect_portable_root() -> Option<PathBuf> {
    let env_forced = env_forces_portable();
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    resolve_portable_root(exe_dir, env_forced)
}

/// Resolve (and cache) the portable data root for the running binary (mirrors
/// `uc_platform::portable::portable_data_root`).
///
/// Cached after the first call: portable status cannot change during a process
/// lifetime. Uses its own `OnceLock` — `uc-platform` caches separately, but
/// both derive the same value from the same signals, so the cached results are
/// identical.
fn portable_data_root() -> Option<PathBuf> {
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE.get_or_init(detect_portable_root).clone()
}

/// Resolve the base local data directory (mirrors
/// `uc_platform::app_dirs::DirsAppDirsAdapter::base_data_local_dir` on the
/// default, non-override path).
fn base_data_local_dir() -> Option<PathBuf> {
    if let Some(portable_root) = portable_data_root() {
        return Some(portable_root);
    }
    dirs::data_local_dir()
}

/// Resolve the application data root: `base_data_local_dir().join(app_dir_name)`.
///
/// Byte-identical to `uc_platform::app_dirs::default_app_dirs()?.app_data_root`.
pub fn app_data_root() -> Result<PathBuf, AppDataRootError> {
    let base = base_data_local_dir().ok_or(AppDataRootError::DataLocalDirUnavailable)?;
    Ok(base.join(resolved_app_dir_name()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_marker_next_to_exe_redirects_root() {
        let exe_dir = Path::new("/opt/UniClipboard");
        assert_eq!(
            resolve_portable_root(exe_dir, true),
            Some(exe_dir.join(PORTABLE_DATA_SUBDIR))
        );
    }

    #[test]
    fn not_portable_without_marker_or_env() {
        assert_eq!(
            resolve_portable_root(Path::new("/nonexistent/uc"), false),
            None
        );
    }

    #[test]
    fn app_dir_name_has_no_profile_suffix_by_default() {
        // Guard against an ambient UC_PROFILE leaking into the assertion.
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _env = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("UC_PROFILE").ok();
        std::env::remove_var("UC_PROFILE");

        assert_eq!(resolved_app_dir_name(), APP_DIR_NAME);

        std::env::set_var("UC_PROFILE", "team-alpha");
        assert_eq!(
            resolved_app_dir_name(),
            format!("{APP_DIR_NAME}-team-alpha")
        );

        std::env::set_var("UC_PROFILE", "");
        assert_eq!(
            resolved_app_dir_name(),
            APP_DIR_NAME,
            "empty UC_PROFILE must not add a suffix"
        );

        match prev {
            Some(v) => std::env::set_var("UC_PROFILE", v),
            None => std::env::remove_var("UC_PROFILE"),
        }
    }
}
