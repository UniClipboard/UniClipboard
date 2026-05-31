//! Portable ("green") build detection and data-root resolution.
//!
//! A portable build keeps every piece of its data next to the executable
//! instead of the per-user system data directory, so it can run from a USB
//! stick and leave no traces on the host machine. This is the platform-layer
//! source of truth for "am I portable, and where does my data live": all
//! upper layers keep calling [`crate::app_dirs`] / the secure-storage factory
//! and transparently follow the redirect.
//!
//! Portable mode is enabled when either:
//!   - the `UC_PORTABLE` environment variable is set to a truthy value
//!     (`1` / `true` / `yes`, case-insensitive) — handy for exercising the
//!     behavior from `cargo run` without shipping a marker file, or
//!   - a marker file named [`PORTABLE_MARKER`] sits next to the executable.
//!
//! The portable zip artifact ships that marker file while the NSIS installer
//! does not, so a single compiled binary serves both distribution forms with
//! no separate build.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Marker file placed next to the executable inside the portable zip. Its mere
/// presence flips the running binary into portable mode.
pub const PORTABLE_MARKER: &str = "portable.dat";

/// Subdirectory (relative to the executable) that holds all portable data.
/// Keeping everything under a single `data/` folder gives users a clean
/// "delete this to reset" story and keeps the zip root tidy.
const PORTABLE_DATA_SUBDIR: &str = "data";

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
/// secure storage, process metadata) should not each re-`current_exe()`.
pub fn portable_data_root() -> Option<PathBuf> {
    static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
    CACHE.get_or_init(detect_portable_root).clone()
}

/// Whether the running binary is operating in portable mode.
pub fn is_portable() -> bool {
    portable_data_root().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_portable_without_marker_or_env() {
        let tmp = std::env::temp_dir().join("uc_portable_test_none");
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
        let dir = std::env::temp_dir().join("uc_portable_test_marker");
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
}
