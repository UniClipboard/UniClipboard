//! # uc-app-paths — directory-layout authority
//!
//! This crate is the **single source of truth** for *where* UniClipboard's
//! application data and cache directories live. It owns the path-resolution
//! *policy* — the app directory name, the `UC_PROFILE` suffix, the portable
//! ("green") redirect, and the per-platform base directories — and exposes them
//! as pure functions that depend on **only** [`dirs`] + `std`.
//!
//! ## Why this crate exists
//!
//! Two very different crates need this exact policy:
//!
//!   - [`uc-platform`](../uc_platform/index.html) — the heavyweight platform
//!     layer (keyring / clipboard / objc2 / wayland / tokio-full) that owns the
//!     `AppDirsPort` implementation, and
//!   - `uc-daemon-process` — a deliberately thin, dependency-light crate that
//!     resolves the daemon PID/token paths without dragging the app stack into
//!     the CLI client (ADR-008 P5).
//!
//! Before this crate existed (ADR-008 P5-0), `uc-daemon-process` carried a
//! *byte-identical copy* of the resolution because it could not depend on the
//! heavy `uc-platform`. Two copies = drift risk (daemon writes path X, client
//! reads path Y). ADR-008 P5-0c extracts the policy here so **both** consumers
//! share one implementation, and a future "split cache / log / user-data dirs"
//! change happens in exactly one place.
//!
//! ## What stays out
//!
//! This crate owns the *raw computation*, not the abstraction. The
//! `AppDirs` / `AppDirsPort` / `AppDirsError` types stay in `uc-core` /
//! `uc-platform`; the `dev-profile` compile-time feature stays in `uc-platform`
//! (passed in here as the `compile_default` parameter). This crate has no
//! features and makes no error-mapping decisions — each consumer maps `None`
//! to its own error type.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Application directory name. The data/cache roots are
/// `<base>/app.uniclipboard.desktop[-<profile>]`.
pub const APP_DIR_NAME: &str = "app.uniclipboard.desktop";

/// Marker file placed next to the executable inside the portable zip. Its mere
/// presence flips the running binary into portable mode.
pub const PORTABLE_MARKER: &str = "portable.dat";

/// Subdirectory (relative to the executable) that holds all portable data.
/// Keeping everything under a single `data/` folder gives users a clean
/// "delete this to reset" story and keeps the zip root tidy.
const PORTABLE_DATA_SUBDIR: &str = "data";

/// Resolve the active profile name.
///
/// Runtime `UC_PROFILE` takes precedence over `compile_default`; an empty
/// `UC_PROFILE` is treated as unset and falls through to `compile_default`.
/// Returns `None` when neither is set.
///
/// `compile_default` lets the caller thread in a compile-time fallback (for
/// example `uc-platform`'s `dev-profile` feature → `Some("dev")`); callers with
/// no such fallback pass `None`.
pub fn resolve_profile(compile_default: Option<&str>) -> Option<String> {
    if let Ok(profile) = std::env::var("UC_PROFILE") {
        if !profile.is_empty() {
            return Some(profile);
        }
    }
    compile_default.map(str::to_string)
}

/// Constructs the application directory name, appending `-<profile>` when a
/// profile is resolved (`UC_PROFILE` runtime override, else `compile_default`).
///
/// # Examples
///
/// ```
/// # use uc_app_paths::{resolved_app_dir_name, APP_DIR_NAME};
/// std::env::set_var("UC_PROFILE", "testing");
/// assert_eq!(resolved_app_dir_name(None), format!("{APP_DIR_NAME}-testing"));
/// std::env::remove_var("UC_PROFILE");
/// ```
pub fn resolved_app_dir_name(compile_default: Option<&str>) -> String {
    match resolve_profile(compile_default) {
        Some(profile) => format!("{APP_DIR_NAME}-{profile}"),
        None => APP_DIR_NAME.to_string(),
    }
}

/// Resolve the portable data root from an executable directory and an explicit
/// env override, without touching process-global state.
///
/// Returns `Some(<exe_dir>/data)` when portable mode is active, `None`
/// otherwise. Split out from [`portable_data_root`] so it can be unit-tested
/// against a temp directory instead of the real executable location.
fn resolve_portable_root(exe_dir: &Path, env_forced: bool) -> Option<PathBuf> {
    if env_forced || exe_dir.join(PORTABLE_MARKER).is_file() {
        Some(exe_dir.join(PORTABLE_DATA_SUBDIR))
    } else {
        None
    }
}

/// Read `UC_PORTABLE` and decide whether it forces portable mode on.
fn env_forces_portable() -> bool {
    match std::env::var("UC_PORTABLE") {
        Ok(value) => {
            let value = value.trim();
            value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")
        }
        Err(_) => false,
    }
}

fn detect_portable_root() -> Option<PathBuf> {
    let env_forced = env_forces_portable();
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?;
    resolve_portable_root(exe_dir, env_forced)
}

/// Resolve (and cache) the portable data root for the running binary.
///
/// Returns `Some(<exe_dir>/data)` in portable mode, `None` otherwise. The
/// result is cached after the first call: portable status cannot change during
/// a process lifetime, and the many call sites (app dirs, daemon socket path,
/// secure storage, process metadata) should not each re-`current_exe()`. This
/// is the *single* portable cache shared by every consumer.
pub fn portable_data_root() -> Option<PathBuf> {
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE.get_or_init(detect_portable_root).clone()
}

/// Whether the running binary is operating in portable mode.
pub fn is_portable() -> bool {
    portable_data_root().is_some()
}

/// Resolve the base local data directory: the portable redirect when active,
/// otherwise [`dirs::data_local_dir`].
///
/// This is the *non-override* resolution; the test-only base override lives in
/// `uc-platform`'s adapter and short-circuits before this is consulted.
pub fn base_data_local_dir() -> Option<PathBuf> {
    // Portable ("green") builds keep all data next to the executable so the
    // app leaves no trace in the per-user system data directory. The redirect
    // is resolved here (the lowest common layer) so every call site — daemon
    // socket path, secure storage, process metadata — follows it without
    // knowing portable mode exists.
    if let Some(portable_root) = portable_data_root() {
        return Some(portable_root);
    }
    dirs::data_local_dir()
}

/// Resolve the base cache directory: the portable redirect when active,
/// otherwise [`dirs::cache_dir`].
pub fn base_cache_dir() -> Option<PathBuf> {
    if let Some(portable_root) = portable_data_root() {
        return Some(portable_root);
    }
    dirs::cache_dir()
}

/// Resolve the application data root: `base_data_local_dir().join(app_dir_name)`.
///
/// Convenience for callers with no compile-time profile default (daemon / CLI),
/// so the profile suffix comes purely from runtime `UC_PROFILE`. Returns `None`
/// when the base data-local directory is unavailable; the caller maps that to
/// its own error type. Consumers that carry a compile-time default (for example
/// `uc-platform` under `dev-profile`) must compose via [`base_data_local_dir`] +
/// [`resolved_app_dir_name`] instead so the suffix is preserved.
pub fn app_data_root() -> Option<PathBuf> {
    Some(base_data_local_dir()?.join(resolved_app_dir_name(None)))
}

/// Resolve the application cache root: `base_cache_dir().join(app_dir_name)`.
///
/// Symmetric convenience to [`app_data_root`] for no-compile-default callers.
pub fn app_cache_root() -> Option<PathBuf> {
    Some(base_cache_dir()?.join(resolved_app_dir_name(None)))
}

/// Resolve the platform-conventional **log directory** (the final leaf, not a
/// root). Logs follow each OS's logging convention instead of living under the
/// data root, while still honoring the portable redirect and the `UC_PROFILE`
/// suffix.
///
/// This is the single source of truth for *where logs live*: every consumer
/// (the daemon, the CLI, and the GUI host's pre-wiring tracing init) resolves
/// the log directory through this function. No other code should join `"logs"`
/// onto a base path.
///
/// - macOS:            `~/Library/Logs/<app>`
/// - Linux:            `<XDG_STATE_HOME>/<app>/logs` (falls back to the
///                     data-local root when the state dir is unavailable)
/// - Windows / other:  `<data-local>/<app>/logs`
/// - portable:         `<portable-root>/logs`
///
/// Returns `None` only when the underlying base directory is unavailable.
pub fn app_log_dir() -> Option<PathBuf> {
    // Portable ("green") builds keep logs next to the executable, alongside the
    // rest of the data, so the app leaves no trace in the system log location.
    if let Some(portable_root) = portable_data_root() {
        return Some(portable_root.join("logs"));
    }
    platform_log_dir(&resolved_app_dir_name(None))
}

#[cfg(target_os = "macos")]
fn platform_log_dir(app_dir_name: &str) -> Option<PathBuf> {
    // Apple convention: per-user logs live under `~/Library/Logs/<app>`.
    Some(
        dirs::home_dir()?
            .join("Library")
            .join("Logs")
            .join(app_dir_name),
    )
}

#[cfg(target_os = "linux")]
fn platform_log_dir(app_dir_name: &str) -> Option<PathBuf> {
    // XDG convention groups logs under the state dir; fall back to the
    // data-local root when the state dir is unavailable.
    let base = dirs::state_dir().or_else(dirs::data_local_dir)?;
    Some(base.join(app_dir_name).join("logs"))
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_log_dir(app_dir_name: &str) -> Option<PathBuf> {
    // Windows (and any other platform) has no dedicated OS log directory, so
    // logs stay under the data-local app root, matching the historical layout.
    Some(dirs::data_local_dir()?.join(app_dir_name).join("logs"))
}

/// The pre-split log location `<app_data_root>/logs`, exposed only so callers
/// can clean up the old directory after logs moved to [`app_log_dir`].
///
/// Returns `None` when the data root is unavailable, or when the old and new
/// locations coincide (Windows / portable) — in that case there is nothing to
/// clean up.
pub fn legacy_logs_dir() -> Option<PathBuf> {
    let legacy = app_data_root()?.join("logs");
    match app_log_dir() {
        Some(current) if current == legacy => None,
        _ => Some(legacy),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_portable_without_marker_or_env() {
        let tmp = std::env::temp_dir().join("uc_app_paths_portable_test_none");
        assert_eq!(resolve_portable_root(&tmp, false), None);
    }

    #[test]
    fn env_override_forces_portable_root() {
        let exe_dir = Path::new("/opt/UniClipboard");
        assert_eq!(
            resolve_portable_root(exe_dir, true),
            Some(exe_dir.join(PORTABLE_DATA_SUBDIR))
        );
    }

    #[test]
    fn marker_file_next_to_exe_enables_portable() {
        let dir = std::env::temp_dir().join("uc_app_paths_portable_test_marker");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join(PORTABLE_MARKER);
        std::fs::write(&marker, b"").unwrap();

        let resolved = resolve_portable_root(&dir, false);
        assert_eq!(resolved, Some(dir.join(PORTABLE_DATA_SUBDIR)));

        std::fs::remove_file(&marker).ok();
        std::fs::remove_dir(&dir).ok();
    }

    #[test]
    fn env_truthy_values_are_parsed_case_insensitively() {
        let exe_dir = Path::new("/opt/UniClipboard");
        // env_forced=true short-circuits the marker check regardless of dir.
        for forced in [true] {
            assert!(resolve_portable_root(exe_dir, forced).is_some());
        }
        // env_forced=false + no marker present (temp path) => not portable.
        assert!(resolve_portable_root(Path::new("/nonexistent/uc"), false).is_none());
    }

    #[test]
    fn app_dir_name_has_no_profile_suffix_by_default() {
        // Guard against an ambient UC_PROFILE leaking into the assertion.
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _env = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("UC_PROFILE").ok();
        std::env::remove_var("UC_PROFILE");

        assert_eq!(resolved_app_dir_name(None), APP_DIR_NAME);

        std::env::set_var("UC_PROFILE", "team-alpha");
        assert_eq!(
            resolved_app_dir_name(None),
            format!("{APP_DIR_NAME}-team-alpha")
        );

        std::env::set_var("UC_PROFILE", "");
        assert_eq!(
            resolved_app_dir_name(None),
            APP_DIR_NAME,
            "empty UC_PROFILE must not add a suffix"
        );

        // Empty UC_PROFILE must fall through to the compile-time default.
        assert_eq!(
            resolved_app_dir_name(Some("dev")),
            format!("{APP_DIR_NAME}-dev"),
            "empty UC_PROFILE must fall back to compile_default"
        );

        // Runtime UC_PROFILE wins over the compile-time default.
        std::env::set_var("UC_PROFILE", "staging");
        assert_eq!(
            resolved_app_dir_name(Some("dev")),
            format!("{APP_DIR_NAME}-staging"),
            "runtime UC_PROFILE must override compile_default"
        );

        match prev {
            Some(v) => std::env::set_var("UC_PROFILE", v),
            None => std::env::remove_var("UC_PROFILE"),
        }
    }

    #[test]
    fn app_log_dir_is_absolute_and_carries_app_name() {
        // Resolution can return None in a bare CI sandbox without a home dir;
        // assert the contract only when a directory is available.
        if let Some(dir) = app_log_dir() {
            assert!(dir.is_absolute(), "log dir must be absolute: {dir:?}");
            // Portable builds keep logs under `<portable-root>/logs`, which does
            // not carry the app directory name, so only assert it off the
            // platform-conventional path.
            if portable_data_root().is_none() {
                assert!(
                    dir.to_string_lossy().contains(APP_DIR_NAME),
                    "log dir must include the app directory name: {dir:?}"
                );
            }
        }
    }

    #[test]
    fn legacy_logs_dir_is_none_when_it_equals_current() {
        // On any platform where `app_log_dir()` resolves to the old
        // `<app_data_root>/logs` location (Windows / portable), there is
        // nothing to clean up, so `legacy_logs_dir()` must report `None`.
        if let (Some(current), Some(data_root)) = (app_log_dir(), app_data_root()) {
            if current == data_root.join("logs") {
                assert_eq!(legacy_logs_dir(), None);
            } else {
                assert_eq!(legacy_logs_dir(), Some(data_root.join("logs")));
            }
        }
    }
}
