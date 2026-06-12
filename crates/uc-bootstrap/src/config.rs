//! # Configuration Loader
//!
//! ## Responsibilities
//!
//! - Read TOML configuration files
//! - Parse TOML into AppConfig DTO
//! - Report I/O and parsing errors with context
//!
//! ## Prohibited
//!
//! - No validation logic
//! - No default value logic
//! - No business rules
//!
//! ## Iron Rule
//!
//! > **Pure data loading only. Accept whatever is in the file.**

use anyhow::Context;
use std::path::PathBuf;
use uc_core::config::AppConfig;

/// Load configuration from a TOML file
///
/// This function performs pure data loading:
/// - Reads file content
/// - Parses TOML format
/// - Maps to AppConfig DTO
///
/// **NO validation is performed**:
/// - Empty strings are valid (they are facts)
/// - Invalid ports are accepted (they are facts)
/// - Missing sections result in empty values (facts)
///
/// # Errors
///
/// Returns error if:
/// - File cannot be read (I/O error)
/// - Content is not valid TOML (parse error)
/// - TOML structure is malformed (mapping error)
pub fn load_config(config_path: PathBuf) -> anyhow::Result<AppConfig> {
    let content = std::fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;
    let toml_value: toml::Value =
        toml::from_str(&content).context("Failed to parse config as TOML")?;
    AppConfig::from_toml(&toml_value)
}
