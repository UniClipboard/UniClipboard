//! Directory-layout resolution.
//!
//! Resolves the platform `AppDirs` and applies config overrides + the
//! `UC_PROFILE` suffix to produce the `AppPaths` the rest of wiring consumes.
//! The authoritative directory layout lives in `uc-app-paths`; this module only
//! adapts it to the composition root's config-override and profile-suffix rules.

use std::path::PathBuf;

use uc_core::config::AppConfig;
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::ports::AppDirsPort;

use crate::wiring::deps::{WiringError, WiringResult};

/// Resolves the application's default directories for storing data and configuration.
pub fn get_default_app_dirs() -> WiringResult<uc_core::app_dirs::AppDirs> {
    let adapter = DirsAppDirsAdapter::new();
    adapter
        .get_app_dirs()
        .map_err(|e| WiringError::ConfigInit(e.to_string()))
}

/// Get resolved storage paths from configuration.
pub fn get_storage_paths(
    config: &uc_core::config::AppConfig,
) -> WiringResult<uc_application::facade::AppPaths> {
    let platform_dirs = get_default_app_dirs()?;
    resolve_app_paths(&platform_dirs, config)
}

/// Build `AppPaths` from platform dirs and config overrides.
pub fn resolve_app_paths(
    platform_dirs: &uc_core::app_dirs::AppDirs,
    config: &AppConfig,
) -> WiringResult<uc_application::facade::AppPaths> {
    let mut paths = uc_application::facade::AppPaths::from_app_dirs(platform_dirs);

    let is_in_memory_db = config.database_path.as_os_str() == ":memory:";

    if is_in_memory_db {
        paths.db_path = config.database_path.clone();
    } else if !config.database_path.as_os_str().is_empty() {
        if config.database_path.is_absolute() {
            // Absolute path: use as-is. In production the path is already inside
            // app_data_root_dir; tests use temp dirs and need the full path respected.
            paths.db_path = config.database_path.clone();
        } else {
            let db_file_name = config
                .database_path
                .file_name()
                .map(|name| name.to_os_string())
                .unwrap_or_else(|| std::ffi::OsString::from("uniclipboard.db"));
            paths.db_path = paths.app_data_root_dir.join(db_file_name);
        }
    }

    if !config.vault_key_path.as_os_str().is_empty() {
        let configured_vault_root = config
            .vault_key_path
            .parent()
            .unwrap_or(&config.vault_key_path)
            .to_path_buf();

        if config.database_path.as_os_str().is_empty() {
            paths.vault_dir = apply_profile_suffix(configured_vault_root);
        } else {
            let configured_db_root = config
                .database_path
                .parent()
                .unwrap_or(&config.database_path)
                .to_path_buf();

            if configured_vault_root.starts_with(&configured_db_root) {
                let relative = configured_vault_root
                    .strip_prefix(&configured_db_root)
                    .unwrap_or(std::path::Path::new(""));
                paths.vault_dir = paths.app_data_root_dir.join(relative);
            } else {
                paths.vault_dir = apply_profile_suffix(configured_vault_root);
            }
        }
    }

    Ok(paths)
}

pub fn apply_profile_suffix(path: PathBuf) -> PathBuf {
    let profile = match std::env::var("UC_PROFILE") {
        Ok(value) if !value.is_empty() => sanitize_profile(&value),
        _ => return path,
    };

    let file_name = match path.file_name().and_then(|name| name.to_str()) {
        Some(name) => name.to_string(),
        None => return path,
    };

    let mut updated = path;
    updated.set_file_name(format!("{file_name}_{profile}"));
    updated
}

/// Normalize a `UC_PROFILE` value into a filesystem-safe suffix.
///
/// Maps every character that is invalid in a Windows filename
/// (`< > : " / \ | ? *` and ASCII control characters) to `_`, so the profile
/// can be safely appended to a file name on any platform. Other platforms only
/// reject `/` (and the NUL byte), so this is a superset of their constraints.
fn sanitize_profile(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}
