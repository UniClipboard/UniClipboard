//! Lightweight filesystem-only setup-completion check.
//!
//! Replaces the heavy `uc_bootstrap::is_setup_complete()` call which pulls in
//! `uc-infra` (FileSetupStatusRepository), `uc-application` (SetupStatusFacade),
//! and the full assembly stack just to answer a boolean question.
//!
//! The check reads at most two small files:
//!   1. `<vault_dir>/.setup_status` — JSON `{"has_completed": true, ...}`
//!   2. `<vault_dir>/.initialized_encryption` — legacy marker (existence = done)
//!
//! `vault_dir` is `app_data_root / "vault"`, where `app_data_root` is resolved by
//! [`uc_app_paths::app_data_root`] (handles `UC_PROFILE`, portable mode, etc.).

use std::path::PathBuf;

use anyhow::{Context, Result};

/// The JSON schema written by `uc-infra::FileSetupStatusRepository`.
#[derive(serde::Deserialize)]
struct SetupStatus {
    has_completed: bool,
}

const SETUP_STATUS_FILE: &str = ".setup_status";
const LEGACY_ENCRYPTION_MARKER: &str = ".initialized_encryption";

/// Returns `true` when the current profile has completed first-time setup.
///
/// Mirrors the semantics of the heavier `uc_bootstrap::is_setup_complete()`:
///   - Reads `.setup_status` JSON; if `has_completed == true` → done.
///   - Falls back to the legacy `.initialized_encryption` marker file.
///   - Returns `false` when neither indicator is present.
///
/// This is a synchronous filesystem check (two `stat` + one small `read` at
/// most). Callers in async context can `.await` a `spawn_blocking` wrapper or
/// simply call it directly — the I/O is trivially fast on local disk.
pub fn is_setup_complete() -> Result<bool> {
    let vault_dir = resolve_vault_dir()?;

    // Primary: parse .setup_status JSON.
    let status_path = vault_dir.join(SETUP_STATUS_FILE);
    if status_path.is_file() {
        let contents = std::fs::read_to_string(&status_path)
            .with_context(|| format!("read {}", status_path.display()))?;
        if let Ok(status) = serde_json::from_str::<SetupStatus>(&contents) {
            if status.has_completed {
                return Ok(true);
            }
        }
        // File exists but `has_completed` is false (or unparseable) — fall through
        // to legacy check for back-compat with pre-Slice 1 state.
    }

    // Legacy fallback: file existence alone means setup was completed.
    let legacy_path = vault_dir.join(LEGACY_ENCRYPTION_MARKER);
    Ok(legacy_path.exists())
}

/// Derive `vault_dir` from `uc_app_paths::app_data_root()`.
fn resolve_vault_dir() -> Result<PathBuf> {
    let app_data_root = uc_app_paths::app_data_root()
        .context("unable to resolve app data root (data_local_dir unavailable)")?;
    Ok(app_data_root.join("vault"))
}
