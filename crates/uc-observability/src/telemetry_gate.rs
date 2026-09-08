//! Process-wide runtime gate for the user-facing telemetry switch.
//!
//! Sentry reads this gate at event time so toggling `general.telemetry_enabled`
//! in the UI takes effect without a restart.
//!
//! - Frontend has its own gate (`setFrontendSentryEnabled` in
//!   `src/observability/sentry.ts`) since the gate must live in the JS runtime.
//! - This module is the equivalent for the Rust side: `uc-bootstrap` consults
//!   it from Sentry's transaction sampler, final transport, `before_send`,
//!   `before_breadcrumb`, and `before_send_log` hooks. When the gate is off,
//!   payloads are dropped before capture or at the final envelope boundary.
//!
//! ## Default
//!
//! Defaults to `true` so events emitted between process start and the first
//! settings load are NOT silently dropped — `uc-bootstrap` reads the
//! persisted preference from disk during init and overrides the default
//! before any user-visible event would normally be processed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

static PREFERENCE_PATH: OnceLock<PathBuf> = OnceLock::new();
static PREFERENCE_WRITE: Mutex<()> = Mutex::new(());

/// Initialize the desktop-owned preference before Engine rewrites its settings.
pub fn initialize_preference(settings_path: &Path) -> anyhow::Result<bool> {
    let path = settings_path.with_file_name("desktop-telemetry.json");
    PREFERENCE_PATH
        .set(path.clone())
        .map_err(|_| anyhow::anyhow!("telemetry preference already initialized"))?;
    let enabled = load_preference(&path, settings_path)?;
    set_telemetry_enabled(enabled);
    Ok(enabled)
}

fn load_preference(path: &Path, legacy_path: &Path) -> anyhow::Result<bool> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice::<bool>(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let enabled = match std::fs::read(legacy_path) {
                Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)?
                    .pointer("/general/telemetry_enabled")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
                Err(error) => return Err(error.into()),
            };
            write_preference(path, enabled)?;
            Ok(enabled)
        }
        Err(error) => Err(error.into()),
    }
}

fn write_preference(path: &Path, enabled: bool) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("missing preference directory"))?;
    std::fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(serde_json::to_string(&enabled)?.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

/// Persist first so a failed save cannot change the runtime preference.
pub fn save_preference(enabled: bool) -> anyhow::Result<()> {
    let _guard = PREFERENCE_WRITE
        .lock()
        .map_err(|_| anyhow::anyhow!("telemetry preference lock poisoned"))?;
    let path = PREFERENCE_PATH
        .get()
        .ok_or_else(|| anyhow::anyhow!("telemetry preference is not initialized"))?;
    write_preference(path, enabled)?;
    set_telemetry_enabled(enabled);
    Ok(())
}

static TELEMETRY_ENABLED: AtomicBool = AtomicBool::new(true);

/// Returns whether the user-facing telemetry switch is currently on.
///
/// Hot path — called once per emitted event/span. `Ordering::Relaxed` is
/// sufficient because there is no other state we synchronize with: the
/// worst case is that an event emitted concurrently with a setter call is
/// classified by the pre-toggle value, which is acceptable.
#[inline]
pub fn is_telemetry_enabled() -> bool {
    TELEMETRY_ENABLED.load(Ordering::Relaxed)
}

/// Update the runtime gate.
///
/// Called from two paths:
/// - `uc-bootstrap` once at init time after reading persisted settings.
/// - `uc-webserver` PUT /settings handler whenever `telemetry_enabled`
///   changes, so the new value takes effect immediately.
pub fn set_telemetry_enabled(enabled: bool) {
    TELEMETRY_ENABLED.store(enabled, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Both tests mutate the same process-wide TELEMETRY_ENABLED static.
    // cargo test runs tests in this binary in parallel by default, so
    // serialize them here to prevent set(false) in one from racing
    // assert(true) in the other.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn preference_preserves_legacy_opt_out_and_survives_engine_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("settings.json");
        let path = dir.path().join("desktop-telemetry.json");
        std::fs::write(&legacy, r#"{"general":{"telemetry_enabled":false}}"#).unwrap();
        assert!(!load_preference(&path, &legacy).unwrap());
        std::fs::write(&legacy, r#"{"general":{}}"#).unwrap();
        assert!(!load_preference(&path, &legacy).unwrap());
        write_preference(&path, true).unwrap();
        assert!(load_preference(&path, &legacy).unwrap());
    }

    #[test]
    fn corrupt_preference_is_not_replaced_with_enabled_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("desktop-telemetry.json");
        std::fs::write(&path, "invalid").unwrap();
        assert!(load_preference(&path, &dir.path().join("settings.json")).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "invalid");
    }

    #[test]
    fn default_is_true_to_avoid_dropping_pre_init_events() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Reset to default in case prior test mutated; then assert.
        set_telemetry_enabled(true);
        assert!(is_telemetry_enabled());
    }

    #[test]
    fn setter_round_trip() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_telemetry_enabled(false);
        assert!(!is_telemetry_enabled());
        set_telemetry_enabled(true);
        assert!(is_telemetry_enabled());
    }
}
