//! Use case for updating application settings
//! 更新应用设置的用例

use anyhow::Result;
use tracing::{info, info_span, Instrument};
use uc_core::ports::SettingsPort;
use uc_core::settings::model::{
    ContentTypes, GeneralSettings, RetentionPolicy, SecuritySettings, Settings, SyncSettings,
};

/// Use case for updating application settings.
///
/// ## Behavior / 行为
/// - Loads current settings for comparison
/// - Validates settings (basic validation)
/// - Logs changed fields with old/new values
/// - Persists settings through the settings port
///
/// ## English
/// Updates the application settings by validating and persisting
/// the provided settings through the configured settings repository.
pub struct UpdateSettings {
    settings: std::sync::Arc<dyn SettingsPort>,
}

impl UpdateSettings {
    /// Create a new UpdateSettings use case.
    pub fn new(settings: std::sync::Arc<dyn SettingsPort>) -> Self {
        Self { settings }
    }

    /// Execute the use case.
    ///
    /// # Parameters / 参数
    /// - `settings`: The settings to persist
    ///
    /// # Returns / 返回值
    /// - `Ok(())` if settings are saved successfully
    /// - `Err(e)` if validation or save fails
    pub async fn execute(&self, settings: &Settings) -> Result<()> {
        let span = info_span!("usecase.update_settings.execute");

        async {
            // Load current settings for diffing
            let old_settings = self.settings.load().await?;

            // Calculate and log changes
            let changes = SettingsDiff::diff(&old_settings, settings);
            if !changes.is_empty() {
                info!(
                    changed_fields = %changes.to_log_string(),
                    "Updating application settings"
                );
            } else {
                info!("Updating application settings (no changes detected)");
            }

            // Basic validation: ensure schema version is current
            let current_version = uc_core::settings::model::CURRENT_SCHEMA_VERSION;
            if settings.schema_version != current_version {
                return Err(anyhow::anyhow!(
                    "Invalid schema version: expected {}, got {}",
                    current_version,
                    settings.schema_version
                ));
            }

            // Persist settings
            self.settings.save(settings).await?;

            info!(
                changed_fields = %changes.to_log_string(),
                "Settings updated successfully"
            );
            Ok(())
        }
        .instrument(span)
        .await
    }
}

/// Represents the difference between two Settings
struct SettingsDiff {
    general: Option<GeneralSettingsDiff>,
    sync: Option<SyncSettingsDiff>,
    retention_policy: Option<RetentionPolicyDiff>,
    security: Option<SecuritySettingsDiff>,
}

impl SettingsDiff {
    /// Calculate the difference between old and new settings
    fn diff(old: &Settings, new: &Settings) -> Self {
        Self {
            general: GeneralSettingsDiff::diff(&old.general, &new.general),
            sync: SyncSettingsDiff::diff(&old.sync, &new.sync),
            retention_policy: RetentionPolicyDiff::diff(
                &old.retention_policy,
                &new.retention_policy,
            ),
            security: SecuritySettingsDiff::diff(&old.security, &new.security),
        }
    }

    /// Check if there are any changes
    fn is_empty(&self) -> bool {
        self.general.is_none()
            && self.sync.is_none()
            && self.retention_policy.is_none()
            && self.security.is_none()
    }

    /// Convert to a structured log string
    fn to_log_string(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref diff) = self.general {
            parts.push(diff.to_log_string("general"));
        }
        if let Some(ref diff) = self.sync {
            parts.push(diff.to_log_string("sync"));
        }
        if let Some(ref diff) = self.retention_policy {
            parts.push(diff.to_log_string("retention_policy"));
        }
        if let Some(ref diff) = self.security {
            parts.push(diff.to_log_string("security"));
        }

        if parts.is_empty() {
            "(no changes)".to_string()
        } else {
            parts.join(", ")
        }
    }
}

struct GeneralSettingsDiff {
    auto_start: Option<(bool, bool)>,
    silent_start: Option<(bool, bool)>,
    auto_check_update: Option<(bool, bool)>,
    theme: Option<(String, String)>,
    theme_color: Option<(Option<String>, Option<String>)>,
    language: Option<(Option<String>, Option<String>)>,
    device_name: Option<(Option<String>, Option<String>)>,
}

impl GeneralSettingsDiff {
    fn diff(old: &GeneralSettings, new: &GeneralSettings) -> Option<Self> {
        let auto_start =
            (old.auto_start != new.auto_start).then_some((old.auto_start, new.auto_start));
        let silent_start =
            (old.silent_start != new.silent_start).then_some((old.silent_start, new.silent_start));
        let auto_check_update = (old.auto_check_update != new.auto_check_update)
            .then_some((old.auto_check_update, new.auto_check_update));
        let theme = (old.theme != new.theme)
            .then_some((format!("{:?}", old.theme), format!("{:?}", new.theme)));
        let theme_color = (old.theme_color != new.theme_color)
            .then_some((old.theme_color.clone(), new.theme_color.clone()));
        let language =
            (old.language != new.language).then_some((old.language.clone(), new.language.clone()));
        let device_name = (old.device_name != new.device_name)
            .then_some((old.device_name.clone(), new.device_name.clone()));

        if auto_start.is_none()
            && silent_start.is_none()
            && auto_check_update.is_none()
            && theme.is_none()
            && theme_color.is_none()
            && language.is_none()
            && device_name.is_none()
        {
            None
        } else {
            Some(Self {
                auto_start,
                silent_start,
                auto_check_update,
                theme,
                theme_color,
                language,
                device_name,
            })
        }
    }

    fn to_log_string(&self, prefix: &str) -> String {
        let mut parts = Vec::new();

        if let Some((old, new)) = &self.auto_start {
            parts.push(format!("{}.auto_start: {} → {}", prefix, old, new));
        }
        if let Some((old, new)) = &self.silent_start {
            parts.push(format!("{}.silent_start: {} → {}", prefix, old, new));
        }
        if let Some((old, new)) = &self.auto_check_update {
            parts.push(format!("{}.auto_check_update: {} → {}", prefix, old, new));
        }
        if let Some((old, new)) = &self.theme {
            parts.push(format!("{}.theme: {} → {}", prefix, old, new));
        }
        if let Some((old, new)) = &self.theme_color {
            parts.push(format!("{}.theme_color: {:?} → {:?}", prefix, old, new));
        }
        if let Some((old, new)) = &self.language {
            parts.push(format!("{}.language: {:?} → {:?}", prefix, old, new));
        }
        if let Some((old, new)) = &self.device_name {
            parts.push(format!("{}.device_name: {:?} → {:?}", prefix, old, new));
        }

        parts.join(", ")
    }
}

struct SyncSettingsDiff {
    auto_sync: Option<(bool, bool)>,
    sync_frequency: Option<(String, String)>,
    content_types: Option<(ContentTypes, ContentTypes)>,
}

impl SyncSettingsDiff {
    fn diff(old: &SyncSettings, new: &SyncSettings) -> Option<Self> {
        let auto_sync = (old.auto_sync != new.auto_sync).then_some((old.auto_sync, new.auto_sync));
        let sync_frequency = (old.sync_frequency != new.sync_frequency).then_some((
            format!("{:?}", old.sync_frequency),
            format!("{:?}", new.sync_frequency),
        ));
        let content_types_changed = old.content_types.text != new.content_types.text
            || old.content_types.image != new.content_types.image
            || old.content_types.link != new.content_types.link
            || old.content_types.file != new.content_types.file
            || old.content_types.code_snippet != new.content_types.code_snippet
            || old.content_types.rich_text != new.content_types.rich_text;
        let content_types =
            content_types_changed.then_some((old.content_types.clone(), new.content_types.clone()));

        if auto_sync.is_none() && sync_frequency.is_none() && content_types.is_none() {
            None
        } else {
            Some(Self {
                auto_sync,
                sync_frequency,
                content_types,
            })
        }
    }

    fn to_log_string(&self, prefix: &str) -> String {
        let mut parts = Vec::new();

        if let Some((old, new)) = &self.auto_sync {
            parts.push(format!("{}.auto_sync: {} → {}", prefix, old, new));
        }
        if let Some((old, new)) = &self.sync_frequency {
            parts.push(format!("{}.sync_frequency: {} → {}", prefix, old, new));
        }
        if let Some((old, new)) = &self.content_types {
            parts.push(format!("{}.content_types: {:?} → {:?}", prefix, old, new));
        }

        parts.join(", ")
    }
}

struct RetentionPolicyDiff {
    enabled: Option<(bool, bool)>,
    skip_pinned: Option<(bool, bool)>,
    evaluation: Option<(String, String)>,
}

impl RetentionPolicyDiff {
    fn diff(old: &RetentionPolicy, new: &RetentionPolicy) -> Option<Self> {
        let enabled = (old.enabled != new.enabled).then_some((old.enabled, new.enabled));
        let skip_pinned =
            (old.skip_pinned != new.skip_pinned).then_some((old.skip_pinned, new.skip_pinned));
        let evaluation = (old.evaluation != new.evaluation).then_some((
            format!("{:?}", old.evaluation),
            format!("{:?}", new.evaluation),
        ));

        if enabled.is_none() && skip_pinned.is_none() && evaluation.is_none() {
            None
        } else {
            Some(Self {
                enabled,
                skip_pinned,
                evaluation,
            })
        }
    }

    fn to_log_string(&self, prefix: &str) -> String {
        let mut parts = Vec::new();

        if let Some((old, new)) = &self.enabled {
            parts.push(format!("{}.enabled: {} → {}", prefix, old, new));
        }
        if let Some((old, new)) = &self.skip_pinned {
            parts.push(format!("{}.skip_pinned: {} → {}", prefix, old, new));
        }
        if let Some((old, new)) = &self.evaluation {
            parts.push(format!("{}.evaluation: {} → {}", prefix, old, new));
        }

        parts.join(", ")
    }
}

struct SecuritySettingsDiff {
    encryption_enabled: Option<(bool, bool)>,
    passphrase_configured: Option<(bool, bool)>,
}

impl SecuritySettingsDiff {
    fn diff(old: &SecuritySettings, new: &SecuritySettings) -> Option<Self> {
        let encryption_enabled = (old.encryption_enabled != new.encryption_enabled)
            .then_some((old.encryption_enabled, new.encryption_enabled));
        let passphrase_configured = (old.passphrase_configured != new.passphrase_configured)
            .then_some((old.passphrase_configured, new.passphrase_configured));

        if encryption_enabled.is_none() && passphrase_configured.is_none() {
            None
        } else {
            Some(Self {
                encryption_enabled,
                passphrase_configured,
            })
        }
    }

    fn to_log_string(&self, prefix: &str) -> String {
        let mut parts = Vec::new();

        if let Some((old, new)) = &self.encryption_enabled {
            parts.push(format!("{}.encryption_enabled: {} → {}", prefix, old, new));
        }
        if let Some((old, new)) = &self.passphrase_configured {
            parts.push(format!(
                "{}.passphrase_configured: {} → {}",
                prefix, old, new
            ));
        }

        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    struct MockSettingsPort {
        stored: Mutex<Settings>,
        load_count: AtomicUsize,
        save_count: AtomicUsize,
    }

    impl MockSettingsPort {
        fn new(initial: Settings) -> Self {
            Self {
                stored: Mutex::new(initial),
                load_count: AtomicUsize::new(0),
                save_count: AtomicUsize::new(0),
            }
        }

        fn load_count(&self) -> usize {
            self.load_count.load(Ordering::SeqCst)
        }

        fn save_count(&self) -> usize {
            self.save_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl SettingsPort for MockSettingsPort {
        async fn load(&self) -> anyhow::Result<Settings> {
            self.load_count.fetch_add(1, Ordering::SeqCst);
            Ok(self.stored.lock().unwrap().clone())
        }

        async fn save(&self, settings: &Settings) -> anyhow::Result<()> {
            self.save_count.fetch_add(1, Ordering::SeqCst);
            *self.stored.lock().unwrap() = settings.clone();
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_update_settings_loads_before_save() {
        let initial = Settings::default();
        let repo = Arc::new(MockSettingsPort::new(initial));

        let mut updated = Settings::default();
        updated.general.device_name = Some("device-1".to_string());

        let usecase = UpdateSettings::new(repo.clone());
        usecase.execute(&updated).await.unwrap();

        assert_eq!(repo.load_count(), 1);
        assert_eq!(repo.save_count(), 1);
        assert_eq!(
            repo.stored.lock().unwrap().general.device_name,
            updated.general.device_name
        );
    }

    #[test]
    fn test_settings_diff_empty_when_no_changes() {
        let settings = Settings::default();
        let diff = SettingsDiff::diff(&settings, &settings);

        assert!(diff.is_empty());
        assert_eq!(diff.to_log_string(), "(no changes)");
    }

    #[test]
    fn test_settings_diff_logs_changes_across_sections() {
        let old = Settings::default();
        let mut new = old.clone();
        new.general.auto_start = true;
        new.sync.sync_frequency = uc_core::settings::model::SyncFrequency::Interval;
        new.retention_policy.enabled = false;
        new.retention_policy.evaluation = uc_core::settings::model::RuleEvaluation::AllMatch;
        new.security.encryption_enabled = true;

        let diff = SettingsDiff::diff(&old, &new);
        let log = diff.to_log_string();

        assert!(!diff.is_empty());
        assert!(log.contains("general.auto_start: false → true"));
        assert!(log.contains("sync.sync_frequency: Realtime → Interval"));
        assert!(log.contains("retention_policy.enabled: true → false"));
        assert!(log.contains("retention_policy.evaluation: AnyMatch → AllMatch"));
        assert!(log.contains("security.encryption_enabled: false → true"));
    }
}
