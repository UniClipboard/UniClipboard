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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Test that valid TOML is parsed correctly
    #[test]
    fn test_load_config_reads_valid_toml() {
        let toml_content = r#"
            [general]
            device_name = "TestDevice"
            silent_start = true

            [security]
            vault_key_path = "/path/to/key"
            vault_snapshot_path = "/path/to/snapshot"

            [network]
            webserver_port = 8080

            [storage]
            database_path = "/path/to/database"
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();
        let config_path = temp_file.path().to_path_buf();

        let config = load_config(config_path).unwrap();

        assert_eq!(config.device_name, "TestDevice");
        assert_eq!(config.webserver_port, 8080);
        assert_eq!(config.silent_start, true);
        assert_eq!(config.vault_key_path, PathBuf::from("/path/to/key"));
        assert_eq!(
            config.vault_snapshot_path,
            PathBuf::from("/path/to/snapshot")
        );
        assert_eq!(config.database_path, PathBuf::from("/path/to/database"));
    }

    /// Test that missing values result in empty/default values
    #[test]
    fn test_load_config_returns_empty_values_when_missing() {
        let toml_content = r#"
            [general]
            # device_name is missing

            [network]
            # webserver_port is missing

            [security]
            # vault paths are missing

            [storage]
            # database_path is missing
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();
        let config_path = temp_file.path().to_path_buf();

        let config = load_config(config_path).unwrap();

        // Empty values are valid "facts"
        assert_eq!(config.device_name, "");
        assert_eq!(config.webserver_port, 0);
        assert_eq!(config.vault_key_path, PathBuf::new());
        assert_eq!(config.vault_snapshot_path, PathBuf::new());
        assert_eq!(config.database_path, PathBuf::new());
        assert_eq!(config.silent_start, false);
    }

    /// Test that port validation is NOT performed
    #[test]
    fn test_load_config_does_not_validate_port_range() {
        // Port 99999 is out of valid range
        // We should accept it as a "fact" (it will be truncated to u16)
        let toml_content = r#"
            [network]
            webserver_port = 99999
        "#;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(toml_content.as_bytes()).unwrap();
        let config_path = temp_file.path().to_path_buf();

        let config = load_config(config_path).unwrap();

        // We don't validate - the value is truncated (99999 as u16 = 34463)
        // This is the raw "fact" from the TOML data
        assert_eq!(config.webserver_port, 34463);
    }

    /// Test that non-existent files return IO error
    #[test]
    fn test_load_config_returns_io_error_on_file_not_found() {
        let non_existent_path = PathBuf::from("/this/path/does/not/exist/config.toml");

        let result = load_config(non_existent_path);

        assert!(result.is_err());
        let err = result.unwrap_err();
        let err_msg = err.to_string().to_lowercase();

        // Should mention file not found or similar IO error
        assert!(
            err_msg.contains("no such file")
                || err_msg.contains("not found")
                || err_msg.contains("failed to read"),
            "Expected IO error message, got: {}",
            err
        );
    }
}
