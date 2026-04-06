use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DurationSeconds};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub auto_start: bool,
    pub silent_start: bool,
    pub auto_check_update: bool,
    pub theme: Theme,
    pub theme_color: Option<String>,
    pub language: Option<String>,
    pub device_name: Option<String>,
    /// Update channel preference. `None` means auto-detect from version string;
    /// `Some(channel)` means the user has overridden the channel.
    #[serde(default)]
    pub update_channel: Option<UpdateChannel>,
    /// Whether anonymous diagnostic telemetry is enabled.
    /// When `true` and an OTLP endpoint is configured, the app sends
    /// info/warn/error level events (never clipboard content).
    #[serde(default = "default_telemetry_enabled")]
    pub telemetry_enabled: bool,
}

fn default_telemetry_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    Stable,
    Alpha,
    Beta,
    Rc,
}

/// A keyboard shortcut value that can be either a single key combo or multiple alternatives.
///
/// Serialised with `#[serde(untagged)]` so that `"Ctrl+C"` and `["Ctrl+C","Meta+C"]` are both
/// accepted without a wrapping tag, matching the TypeScript type `string | string[]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ShortcutKey {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentTypes {
    pub text: bool,
    pub image: bool,
    pub link: bool,
    pub file: bool,
    pub code_snippet: bool,
    pub rich_text: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncSettings {
    pub auto_sync: bool,
    pub sync_frequency: SyncFrequency,

    #[serde(default)]
    pub content_types: ContentTypes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncFrequency {
    Realtime,
    Interval,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionRule {
    /// 按时间清理
    ByAge {
        #[serde_as(as = "DurationSeconds<u64>")]
        max_age: Duration,
    },

    /// 按总数量上限
    ByCount { max_items: usize },

    /// 按内容类型的最大存活时间
    ByContentType {
        content_type: ContentTypes,
        #[serde_as(as = "DurationSeconds<u64>")]
        max_age: Duration,
    },

    /// 按磁盘占用大小
    ByTotalSize { max_bytes: u64 },

    /// 敏感内容快速过期
    Sensitive {
        #[serde_as(as = "DurationSeconds<u64>")]
        max_age: Duration,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuleEvaluation {
    AnyMatch, // OR（推荐，默认）
    AllMatch, // AND（极少用）
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RetentionPolicy {
    pub enabled: bool,
    pub rules: Vec<RetentionRule>,
    pub skip_pinned: bool,
    pub evaluation: RuleEvaluation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SecuritySettings {
    /// 是否启用本地数据加密
    pub encryption_enabled: bool,

    /// 是否已经在 keyring 中设置过口令
    ///
    /// 仅用于 UI 与流程判断
    /// 不代表当前口令是否“可用”
    pub passphrase_configured: bool,

    /// 是否启用启动时自动解锁
    ///
    /// 仅用于 UI 与流程判断
    /// 需要用户在系统弹窗中选择“始终允许”才能静默生效
    #[serde(default)]
    pub auto_unlock_enabled: bool,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingSettings {
    #[serde_as(as = "DurationSeconds<u64>")]
    pub step_timeout: Duration,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub user_verification_timeout: Duration,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub session_timeout: Duration,
    pub max_retries: u8,
    pub protocol_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileSyncSettings {
    pub file_sync_enabled: bool,
    pub small_file_threshold: u64,
    pub max_file_size: u64,
    pub file_cache_quota_per_device: u64,
    pub file_retention_hours: u32,
    pub file_auto_cleanup: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "current_schema_version")]
    pub schema_version: u32,

    #[serde(default)]
    pub general: GeneralSettings,

    #[serde(default)]
    pub sync: SyncSettings,

    #[serde(default)]
    pub retention_policy: RetentionPolicy,

    #[serde(default)]
    pub security: SecuritySettings,

    #[serde(default)]
    pub pairing: PairingSettings,

    #[serde(default)]
    pub keyboard_shortcuts: HashMap<String, ShortcutKey>,

    #[serde(default)]
    pub file_sync: FileSyncSettings,
    // #[serde(default)]
    // pub network: NetworkSettings,
}

/// The current schema version used for settings persistence.
///
/// # Returns
///
/// The schema version as a `u32`.
///
/// # Examples
///
/// ```
/// use uc_core::settings::model::{current_schema_version, CURRENT_SCHEMA_VERSION};
///
/// let v = current_schema_version();
/// assert_eq!(v, CURRENT_SCHEMA_VERSION);
/// ```
pub fn current_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

#[cfg(test)]
mod tests {
    use super::{
        FileSyncSettings, GeneralSettings, RuleEvaluation, SecuritySettings, Settings,
        SyncFrequency, UpdateChannel,
    };
    use serde_json::json;

    #[test]
    fn test_update_channel_serialization_roundtrip() {
        let channels = [
            UpdateChannel::Stable,
            UpdateChannel::Alpha,
            UpdateChannel::Beta,
            UpdateChannel::Rc,
        ];
        for channel in &channels {
            let serialized = serde_json::to_value(channel).expect("serialize UpdateChannel");
            let deserialized: UpdateChannel =
                serde_json::from_value(serialized).expect("deserialize UpdateChannel");
            assert_eq!(channel, &deserialized);
        }
    }

    #[test]
    fn test_update_channel_serialized_names() {
        assert_eq!(
            serde_json::to_value(UpdateChannel::Stable).unwrap(),
            json!("stable")
        );
        assert_eq!(
            serde_json::to_value(UpdateChannel::Alpha).unwrap(),
            json!("alpha")
        );
        assert_eq!(
            serde_json::to_value(UpdateChannel::Beta).unwrap(),
            json!("beta")
        );
        assert_eq!(
            serde_json::to_value(UpdateChannel::Rc).unwrap(),
            json!("rc")
        );
    }

    #[test]
    fn test_general_settings_missing_update_channel_defaults_to_none() {
        let value = json!({
            "auto_start": false,
            "silent_start": false,
            "auto_check_update": true,
            "theme": "system",
            "theme_color": null,
            "language": null,
            "device_name": null
        });

        let settings: GeneralSettings =
            serde_json::from_value(value).expect("deserialize GeneralSettings");

        assert!(settings.update_channel.is_none());
    }

    #[test]
    fn test_general_settings_explicit_update_channel_is_preserved() {
        let value = json!({
            "auto_start": false,
            "silent_start": false,
            "auto_check_update": true,
            "theme": "system",
            "theme_color": null,
            "language": null,
            "device_name": null,
            "update_channel": "beta"
        });

        let settings: GeneralSettings =
            serde_json::from_value(value).expect("deserialize GeneralSettings");

        assert_eq!(settings.update_channel, Some(UpdateChannel::Beta));
    }

    #[test]
    fn test_sync_frequency_equality() {
        assert_eq!(SyncFrequency::Realtime, SyncFrequency::Realtime);
        assert_ne!(SyncFrequency::Realtime, SyncFrequency::Interval);
    }

    #[test]
    fn test_rule_evaluation_equality() {
        assert_eq!(RuleEvaluation::AnyMatch, RuleEvaluation::AnyMatch);
        assert_ne!(RuleEvaluation::AnyMatch, RuleEvaluation::AllMatch);
    }

    #[test]
    fn test_security_settings_defaults_auto_unlock_on_missing_field() {
        let value = json!({
            "encryption_enabled": true,
            "passphrase_configured": true
        });

        let settings: SecuritySettings =
            serde_json::from_value(value).expect("deserialize security settings");

        assert!(settings.encryption_enabled);
        assert!(settings.passphrase_configured);
        assert!(!settings.auto_unlock_enabled);
    }

    #[test]
    fn test_settings_missing_keyboard_shortcuts_defaults_to_empty() {
        let value = json!({
            "schema_version": 1
        });

        let settings: Settings = serde_json::from_value(value).expect("deserialize settings");
        assert!(
            settings.keyboard_shortcuts.is_empty(),
            "keyboard_shortcuts should default to empty HashMap"
        );
    }

    #[test]
    fn test_pairing_settings_defaults_when_missing() {
        let value = serde_json::json!({
            "schema_version": 1
        });

        let settings: Settings = serde_json::from_value(value).expect("deserialize settings");
        assert_eq!(settings.pairing.step_timeout.as_secs(), 30);
        assert_eq!(settings.pairing.user_verification_timeout.as_secs(), 120);
        assert_eq!(settings.pairing.session_timeout.as_secs(), 300);
        assert_eq!(settings.pairing.max_retries, 3);
        assert_eq!(settings.pairing.protocol_version, "1.0.0");
    }

    #[test]
    fn test_file_sync_settings_backward_compat_defaults_when_missing() {
        let value = serde_json::json!({
            "schema_version": 1
        });

        let settings: Settings = serde_json::from_value(value).expect("deserialize settings");
        let fs = &settings.file_sync;
        assert!(fs.file_sync_enabled);
        assert_eq!(fs.small_file_threshold, 10 * 1024 * 1024);
        assert_eq!(fs.max_file_size, 5 * 1024 * 1024 * 1024);
        assert_eq!(fs.file_cache_quota_per_device, 500 * 1024 * 1024);
        assert_eq!(fs.file_retention_hours, 24);
        assert!(fs.file_auto_cleanup);
    }

    #[test]
    fn test_sync_settings_ignores_legacy_max_file_size_mb_field() {
        let value = serde_json::json!({
            "auto_sync": true,
            "sync_frequency": "realtime",
            "content_types": {
                "text": true,
                "image": true,
                "link": true,
                "file": true,
                "code_snippet": true,
                "rich_text": true
            },
            "max_file_size_mb": 42
        });

        let settings: super::SyncSettings =
            serde_json::from_value(value).expect("deserialize sync settings");
        assert!(settings.auto_sync);
        assert_eq!(settings.sync_frequency, SyncFrequency::Realtime);
    }

    #[test]
    fn test_file_sync_settings_roundtrip() {
        let fs = FileSyncSettings {
            file_sync_enabled: false,
            small_file_threshold: 1_000_000,
            max_file_size: 2_000_000_000,
            file_cache_quota_per_device: 100_000_000,
            file_retention_hours: 48,
            file_auto_cleanup: false,
        };

        let serialized = serde_json::to_value(&fs).expect("serialize");
        let deserialized: FileSyncSettings =
            serde_json::from_value(serialized).expect("deserialize");
        assert_eq!(fs, deserialized);
    }

    #[test]
    fn test_file_sync_settings_default_values() {
        let fs = FileSyncSettings::default();
        assert!(fs.file_sync_enabled);
        assert_eq!(fs.small_file_threshold, 10 * 1024 * 1024);
        assert_eq!(fs.max_file_size, 5 * 1024 * 1024 * 1024);
        assert_eq!(fs.file_cache_quota_per_device, 500 * 1024 * 1024);
        assert_eq!(fs.file_retention_hours, 24);
        assert!(fs.file_auto_cleanup);
    }
}
