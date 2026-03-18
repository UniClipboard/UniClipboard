//! # Config Resolution
//!
//! Resolves the application configuration from environment variables, filesystem search,
//! or system-default platform directories.
//!
//! ## Responsibilities
//!
//! - Locate config.toml via `UC_CONFIG_PATH` env var or ancestor directory search
//! - Load the located config file or fall back to system defaults
//! - Surface structured errors for invalid config or platform directory failures

use std::path::PathBuf;
use uc_core::config::AppConfig;
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::ports::app_dirs::AppDirsPort;

use crate::config::load_config;

/// Errors that can occur during application config resolution.
#[derive(Debug)]
pub enum ConfigResolutionError {
    /// A config file was found at `path` but could not be parsed.
    InvalidConfig {
        path: PathBuf,
        source: anyhow::Error,
    },
    /// The platform data directory could not be determined.
    PlatformDirsFailed { source: anyhow::Error },
}

impl std::fmt::Display for ConfigResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigResolutionError::InvalidConfig { path, source } => {
                write!(f, "Config file '{}' is invalid: {}", path.display(), source)
            }
            ConfigResolutionError::PlatformDirsFailed { source } => {
                write!(f, "Platform directory resolution failed: {}", source)
            }
        }
    }
}

impl std::error::Error for ConfigResolutionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        None
    }
}

/// Locate the application config file.
///
/// Search order:
/// 1. `UC_CONFIG_PATH` environment variable (if set and points to an existing file)
/// 2. Walk ancestor directories from `current_dir()` looking for `config.toml` or
///    `src-tauri/config.toml`
/// 3. Returns `None` when no config file is found
pub fn resolve_config_path() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("UC_CONFIG_PATH") {
        let explicit_path = PathBuf::from(explicit);
        if explicit_path.is_file() {
            return Some(explicit_path);
        }
    }

    let current_dir = std::env::current_dir().ok()?;

    for ancestor in current_dir.ancestors() {
        let candidate = ancestor.join("config.toml");
        if candidate.is_file() {
            return Some(candidate);
        }

        let src_tauri_candidate = ancestor.join("src-tauri").join("config.toml");
        if src_tauri_candidate.is_file() {
            return Some(src_tauri_candidate);
        }
    }

    None
}

/// Resolve the application configuration.
///
/// Calls [`resolve_config_path`] to find a config file. If found and valid, returns the loaded
/// [`AppConfig`]. If no file is found, falls back to
/// [`AppConfig::with_system_defaults`] using the platform data directory. If a file is found but
/// malformed, returns [`ConfigResolutionError::InvalidConfig`].
///
/// # Errors
///
/// - [`ConfigResolutionError::InvalidConfig`] — config file exists but is malformed
/// - [`ConfigResolutionError::PlatformDirsFailed`] — platform data directory unavailable
pub fn resolve_app_config() -> Result<AppConfig, ConfigResolutionError> {
    let config_path = resolve_config_path().unwrap_or_else(|| PathBuf::from("config.toml"));

    match load_config(config_path.clone()) {
        Ok(config) => {
            tracing::info!(
                "Loaded config from {} (development mode)",
                config_path.display()
            );
            Ok(config)
        }
        Err(e) => {
            if config_path.is_file() {
                return Err(ConfigResolutionError::InvalidConfig {
                    path: config_path,
                    source: e,
                });
            }
            tracing::debug!("No config.toml found, using system defaults: {}", e);
            let app_dirs = DirsAppDirsAdapter::new().get_app_dirs().map_err(|e| {
                ConfigResolutionError::PlatformDirsFailed {
                    source: anyhow::anyhow!("{}", e),
                }
            })?;
            Ok(AppConfig::with_system_defaults(app_dirs.app_data_root))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs, sync::Mutex};
    use tempfile::TempDir;

    static CWD_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_resolve_config_path_finds_parent_directory() {
        let _guard = CWD_TEST_LOCK.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let root_dir = temp_dir.path();
        let nested_dir = root_dir.join("src-tauri");
        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(root_dir.join("config.toml"), "").unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&nested_dir).unwrap();

        let resolved = resolve_config_path().and_then(|path| fs::canonicalize(path).ok());

        env::set_current_dir(original_dir).unwrap();

        let expected = fs::canonicalize(root_dir.join("config.toml")).unwrap();
        assert_eq!(resolved, Some(expected));
    }

    #[test]
    fn test_resolve_config_path_finds_src_tauri_config_from_repo_root() {
        let _guard = CWD_TEST_LOCK.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        let root_dir = temp_dir.path();
        let src_tauri_dir = root_dir.join("src-tauri");
        fs::create_dir_all(&src_tauri_dir).unwrap();
        fs::write(src_tauri_dir.join("config.toml"), "").unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(root_dir).unwrap();

        let resolved = resolve_config_path().and_then(|path| fs::canonicalize(path).ok());

        env::set_current_dir(original_dir).unwrap();

        let expected = fs::canonicalize(src_tauri_dir.join("config.toml")).unwrap();
        assert_eq!(resolved, Some(expected));
    }

    #[test]
    fn test_resolve_app_config_returns_system_defaults_when_no_config_file() {
        // With no UC_CONFIG_PATH set and a non-existent CWD fallback,
        // resolve_app_config should fall back to system defaults
        let _guard = CWD_TEST_LOCK.lock().unwrap();
        let temp_dir = TempDir::new().unwrap();
        // Set CWD to empty temp dir with no config.toml
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();
        // Clear UC_CONFIG_PATH to ensure no explicit override
        let prev_config_path = env::var("UC_CONFIG_PATH").ok();
        env::remove_var("UC_CONFIG_PATH");

        let result = resolve_app_config();

        env::set_current_dir(original_dir).unwrap();
        if let Some(prev) = prev_config_path {
            env::set_var("UC_CONFIG_PATH", prev);
        }

        let config = result.expect("resolve_app_config should succeed with system defaults");
        // System defaults use platform data dir; key indicator is non-empty database_path
        assert!(
            !config.database_path.as_os_str().is_empty(),
            "System-default config should have non-empty database_path"
        );
    }
}
