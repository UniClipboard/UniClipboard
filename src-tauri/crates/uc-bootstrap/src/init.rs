//! Bootstrap initialization functions
//!
//! This module contains initialization functions that run during application startup.

use std::sync::Arc;
use uc_core::ports::SettingsPort;

/// Ensures the device has a valid name by initializing it with the system hostname if empty.
///
/// When the application starts, this function checks if `device_name` is `None` or an empty
/// string. If so, it fetches the system hostname and saves it as the default device name.
///
/// # Arguments
///
/// * `settings` - A reference to the settings port implementation
///
/// # Returns
///
/// * `Result<(), Box<dyn std::error::Error>>` - Ok on success, error on failure
///
/// # Behavior
///
/// - If `device_name` is `None` or empty, fetches system hostname and saves it
/// - If `device_name` already has a value, does nothing
/// - Logs the initialization event when setting hostname
///
/// # Example
///
/// ```no_run
/// use uc_bootstrap::ensure_default_device_name;
/// use uc_core::ports::SettingsPort;
/// use std::sync::Arc;
///
/// # async fn example(settings: Arc<dyn SettingsPort>) -> Result<(), Box<dyn std::error::Error>> {
/// ensure_default_device_name(settings).await?;
/// # Ok(())
/// # }
/// ```
pub async fn ensure_default_device_name(
    settings: Arc<dyn SettingsPort>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut current_settings = settings.load().await?;

    // Check if device_name is None or empty string
    let needs_initialization = current_settings.general.device_name.is_none()
        || current_settings.general.device_name.as_deref() == Some("");

    if needs_initialization {
        let hostname = gethostname::gethostname()
            .to_str()
            .unwrap_or("Uniclipboard Device")
            .to_string();

        tracing::info!("Initializing default device name: {}", hostname);

        current_settings.general.device_name = Some(hostname);
        settings.save(&current_settings).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;
    use uc_core::settings::model::Settings;
    use uc_infra::settings::repository::FileSettingsRepository;

    /// Test that hostname is set when device_name is None
    #[tokio::test]
    async fn sets_hostname_when_none() {
        let temp_dir = tempdir().unwrap();
        let settings_path = temp_dir.path().join("test_settings.json");

        let repo = FileSettingsRepository::new(settings_path);
        let repo_arc: Arc<dyn SettingsPort> = Arc::new(repo);

        // Create settings with None device_name
        let mut settings = Settings::default();
        settings.general.device_name = None;
        repo_arc.save(&settings).await.unwrap();

        // Run ensure_default_device_name
        ensure_default_device_name(repo_arc.clone()).await.unwrap();

        // Verify hostname was set
        let loaded = repo_arc.load().await.unwrap();
        assert!(loaded.general.device_name.is_some());
        // Hostname should not be empty
        assert!(!loaded.general.device_name.unwrap().is_empty());
    }

    /// Test that existing device_name is preserved
    #[tokio::test]
    async fn preserves_existing_name() {
        let temp_dir = tempdir().unwrap();
        let settings_path = temp_dir.path().join("test_settings.json");

        let repo = FileSettingsRepository::new(settings_path);
        let repo_arc: Arc<dyn SettingsPort> = Arc::new(repo);

        // Create settings with existing device_name
        let mut settings = Settings::default();
        settings.general.device_name = Some("My Device".to_string());
        repo_arc.save(&settings).await.unwrap();

        // Run ensure_default_device_name
        ensure_default_device_name(repo_arc.clone()).await.unwrap();

        // Verify existing name is preserved
        let loaded = repo_arc.load().await.unwrap();
        assert_eq!(loaded.general.device_name, Some("My Device".to_string()));
    }

    /// Test that empty string is refilled with hostname
    #[tokio::test]
    async fn refills_empty_string() {
        let temp_dir = tempdir().unwrap();
        let settings_path = temp_dir.path().join("test_settings.json");

        let repo = FileSettingsRepository::new(settings_path);
        let repo_arc: Arc<dyn SettingsPort> = Arc::new(repo);

        // Create settings with empty device_name
        let mut settings = Settings::default();
        settings.general.device_name = Some("".to_string());
        repo_arc.save(&settings).await.unwrap();

        // Run ensure_default_device_name
        ensure_default_device_name(repo_arc.clone()).await.unwrap();

        // Verify hostname was set
        let loaded = repo_arc.load().await.unwrap();
        assert!(loaded.general.device_name.is_some());
        assert!(!loaded.general.device_name.unwrap().is_empty());
    }
}
