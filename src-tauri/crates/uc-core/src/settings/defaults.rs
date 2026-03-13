use std::collections::HashMap;
use std::time::Duration;

use super::model::*;

impl Default for UpdateChannel {
    /// Returns the default `UpdateChannel`, which is `Stable`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::UpdateChannel;
    ///
    /// let channel = UpdateChannel::default();
    /// assert_eq!(channel, UpdateChannel::Stable);
    /// ```
    fn default() -> Self {
        UpdateChannel::Stable
    }
}

impl Default for GeneralSettings {
    /// Returns the default `GeneralSettings` used when no user preferences are configured.
    ///
    /// The defaults are:
    /// - `auto_start`: false
    /// - `silent_start`: false
    /// - `auto_check_update`: true
    /// - `theme`: `Theme::System`
    /// - `theme_color`: `None`
    /// - `device_name`: `None`
    /// - `language`: `None`
    /// - `update_channel`: `None` (auto-detect from version)
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::{GeneralSettings, Theme};
    ///
    /// let settings = GeneralSettings::default();
    /// assert_eq!(settings.auto_start, false);
    /// assert_eq!(settings.silent_start, false);
    /// assert_eq!(settings.auto_check_update, true);
    /// assert_eq!(settings.theme, Theme::System);
    /// assert!(settings.theme_color.is_none());
    /// assert!(settings.device_name.is_none());
    /// assert!(settings.language.is_none());
    /// assert!(settings.update_channel.is_none());
    /// ```
    fn default() -> Self {
        Self {
            auto_start: false,
            silent_start: false,
            auto_check_update: true,
            theme: Theme::System,
            theme_color: None,
            device_name: None,
            language: None,
            update_channel: None,
        }
    }
}

impl Default for ContentTypes {
    /// Returns default `ContentTypes` with all fields set to `true`.
    ///
    /// New devices sync everything by default. Users can then disable
    /// specific content types per device.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::ContentTypes;
    ///
    /// let ct = ContentTypes::default();
    /// assert!(ct.text);
    /// assert!(ct.image);
    /// assert!(ct.link);
    /// assert!(ct.file);
    /// assert!(ct.code_snippet);
    /// assert!(ct.rich_text);
    /// ```
    fn default() -> Self {
        Self {
            text: true,
            image: true,
            link: true,
            file: true,
            code_snippet: true,
            rich_text: true,
        }
    }
}

impl Default for SyncSettings {
    /// Creates a `SyncSettings` populated with sensible defaults.
    ///
    /// The defaults enable automatic syncing, use realtime sync frequency, include the
    /// default content types, and limit individual files to 100 MB.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::{SyncSettings, SyncFrequency};
    ///
    /// let s = SyncSettings::default();
    /// assert!(s.auto_sync);
    /// assert_eq!(s.sync_frequency, SyncFrequency::Realtime);
    /// assert_eq!(s.max_file_size_mb, 100);
    /// ```
    fn default() -> Self {
        Self {
            auto_sync: true,
            sync_frequency: SyncFrequency::Realtime,
            content_types: ContentTypes::default(),
            max_file_size_mb: 100,
        }
    }
}

impl Default for RetentionPolicy {
    /// Creates a `RetentionPolicy` populated with sensible defaults.
    ///
    /// The default policy is enabled, skips pinned items, evaluates rules using `AnyMatch`,
    /// and includes two rules: keep items younger than 30 days and keep up to 500 most recent items.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use uc_core::settings::model::{RetentionPolicy, RuleEvaluation, RetentionRule};
    ///
    /// let p = RetentionPolicy::default();
    /// assert!(p.enabled);
    /// assert!(p.skip_pinned);
    /// assert_eq!(p.evaluation, RuleEvaluation::AnyMatch);
    /// assert!(matches!(p.rules.get(0), Some(RetentionRule::ByAge { .. })));
    /// assert!(matches!(p.rules.get(1), Some(RetentionRule::ByCount { .. })));
    /// if let Some(RetentionRule::ByAge { max_age }) = p.rules.get(0) {
    ///     assert_eq!(*max_age, Duration::from_secs(60 * 60 * 24 * 30));
    /// }
    /// if let Some(RetentionRule::ByCount { max_items }) = p.rules.get(1) {
    ///     assert_eq!(*max_items, 500);
    /// }
    /// ```
    fn default() -> Self {
        Self {
            enabled: true,
            skip_pinned: true,
            evaluation: RuleEvaluation::AnyMatch,
            rules: vec![
                RetentionRule::ByAge {
                    max_age: Duration::from_secs(60 * 60 * 24 * 30), // 30 days
                },
                RetentionRule::ByCount { max_items: 500 },
            ],
        }
    }
}

impl Default for SecuritySettings {
    /// Creates default security settings with encryption disabled and no passphrase configured.
    ///
    /// The default has `encryption_enabled = false`, `passphrase_configured = false`,
    /// and `auto_unlock_enabled = false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::SecuritySettings;
    ///
    /// let s = SecuritySettings::default();
    /// assert!(!s.encryption_enabled);
    /// assert!(!s.passphrase_configured);
    /// assert!(!s.auto_unlock_enabled);
    /// ```
    fn default() -> Self {
        Self {
            encryption_enabled: false,
            passphrase_configured: false,
            auto_unlock_enabled: false,
        }
    }
}

impl Default for PairingSettings {
    /// Creates default pairing settings for handshake timers and retry behavior.
    ///
    /// Defaults are:
    /// - `step_timeout`: 30 seconds
    /// - `user_verification_timeout`: 120 seconds
    /// - `session_timeout`: 300 seconds
    /// - `max_retries`: 3
    /// - `protocol_version`: "1.0.0"
    fn default() -> Self {
        Self {
            step_timeout: Duration::from_secs(30),
            user_verification_timeout: Duration::from_secs(120),
            session_timeout: Duration::from_secs(300),
            max_retries: 3,
            protocol_version: "1.0.0".to_string(),
        }
    }
}

impl Default for Settings {
    /// Constructs a Settings instance populated with the current schema version and sensible nested defaults.
    ///
    /// The created `Settings` uses `CURRENT_SCHEMA_VERSION` for `schema_version` and the `Default` implementations
    /// of the nested settings types for `general`, `sync`, `retention_policy`, `security`, and `pairing`.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::settings::model::{Settings, CURRENT_SCHEMA_VERSION};
    ///
    /// let settings = Settings::default();
    /// assert_eq!(settings.schema_version, CURRENT_SCHEMA_VERSION);
    /// // Nested defaults are available:
    /// let _ = settings.general;
    /// let _ = settings.sync;
    /// let _ = settings.retention_policy;
    /// let _ = settings.security;
    /// let _ = settings.pairing;
    /// ```
    ///
    /// # Returns
    ///
    /// `Settings` initialized with `CURRENT_SCHEMA_VERSION` and default values for `general`, `sync`, `retention_policy`, `security`, and `pairing`.
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            general: GeneralSettings::default(),
            sync: SyncSettings::default(),
            retention_policy: RetentionPolicy::default(),
            security: SecuritySettings::default(),
            pairing: PairingSettings::default(),
            keyboard_shortcuts: HashMap::new(),
        }
    }
}
