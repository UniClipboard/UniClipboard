use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeSummary {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannelSummary {
    Stable,
    Alpha,
    Beta,
    Rc,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupModeSummary {
    #[default]
    Normal,
    Silent,
    Lightweight,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ShortcutKeySummary {
    Single(String),
    Multiple(Vec<String>),
}

impl fmt::Debug for ShortcutKeySummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Single(_) => formatter.write_str("ShortcutKeySummary::Single([REDACTED])"),
            Self::Multiple(values) => formatter
                .debug_struct("ShortcutKeySummary::Multiple")
                .field("count", &values.len())
                .finish(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsContentTypes {
    pub text: bool,
    pub image: bool,
    pub link: bool,
    pub file: bool,
    pub code_snippet: bool,
    pub rich_text: bool,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsContentTypesPatch {
    pub text: Option<bool>,
    pub image: Option<bool>,
    pub link: Option<bool>,
    pub file: Option<bool>,
    pub code_snippet: Option<bool>,
    pub rich_text: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncFrequencySummary {
    #[default]
    Realtime,
    Interval,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleEvaluationSummary {
    #[default]
    AnyMatch,
    AllMatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionRuleSummary {
    ByAge {
        max_age_secs: u64,
    },
    ByCount {
        max_items: u64,
    },
    ByContentType {
        content_type: SettingsContentTypes,
        max_age_secs: u64,
    },
    ByTotalSize {
        max_bytes: u64,
    },
    Sensitive {
        max_age_secs: u64,
    },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionRulePatch {
    ByAge {
        max_age_secs: u64,
    },
    ByCount {
        max_items: u64,
    },
    ByContentType {
        content_type: SettingsContentTypes,
        max_age_secs: u64,
    },
    ByTotalSize {
        max_bytes: u64,
    },
    Sensitive {
        max_age_secs: u64,
    },
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneralSettingsSummary {
    pub auto_start: bool,
    pub startup_mode: StartupModeSummary,
    pub restore_last_entry_on_startup: bool,
    pub auto_check_update: bool,
    pub auto_download_update: bool,
    pub theme: ThemeSummary,
    pub theme_color: Option<String>,
    pub theme_color_light: Option<String>,
    pub theme_color_dark: Option<String>,
    pub theme_overrides_light: BTreeMap<String, String>,
    pub theme_overrides_dark: BTreeMap<String, String>,
    pub language: Option<String>,
    pub device_name: Option<String>,
    pub update_channel: Option<UpdateChannelSummary>,
    pub telemetry_enabled: bool,
    pub usage_analytics_enabled: bool,
    pub debug_mode: bool,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneralSettingsPatch {
    pub auto_start: Option<bool>,
    pub startup_mode: Option<StartupModeSummary>,
    pub restore_last_entry_on_startup: Option<bool>,
    pub auto_check_update: Option<bool>,
    pub auto_download_update: Option<bool>,
    pub theme: Option<ThemeSummary>,
    pub theme_color: Option<Option<String>>,
    pub theme_color_light: Option<Option<String>>,
    pub theme_color_dark: Option<Option<String>>,
    pub theme_overrides_light: Option<BTreeMap<String, String>>,
    pub theme_overrides_dark: Option<BTreeMap<String, String>>,
    pub language: Option<Option<String>>,
    pub device_name: Option<Option<String>>,
    pub update_channel: Option<Option<UpdateChannelSummary>>,
    pub telemetry_enabled: Option<bool>,
    pub usage_analytics_enabled: Option<bool>,
    pub debug_mode: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSettingsSummary {
    pub auto_sync: bool,
    pub sync_frequency: SyncFrequencySummary,
    pub content_types: SettingsContentTypes,
    pub sync_on_restore: bool,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSettingsPatch {
    pub auto_sync: Option<bool>,
    pub sync_frequency: Option<SyncFrequencySummary>,
    pub content_types: Option<SettingsContentTypesPatch>,
    pub sync_on_restore: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPolicySummary {
    pub enabled: bool,
    pub rules: Vec<RetentionRuleSummary>,
    pub skip_pinned: bool,
    pub evaluation: RuleEvaluationSummary,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionPolicyPatch {
    pub enabled: Option<bool>,
    pub rules: Option<Vec<RetentionRulePatch>>,
    pub skip_pinned: Option<bool>,
    pub evaluation: Option<RuleEvaluationSummary>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecuritySettingsSummary {
    pub encryption_enabled: bool,
    pub passphrase_configured: bool,
    pub auto_unlock_enabled: bool,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecuritySettingsPatch {
    pub encryption_enabled: Option<bool>,
    pub auto_unlock_enabled: Option<bool>,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingSettingsSummary {
    pub step_timeout_secs: u64,
    pub user_verification_timeout_secs: u64,
    pub session_timeout_secs: u64,
    pub max_retries: u8,
    pub protocol_version: String,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairingSettingsPatch {
    pub step_timeout_secs: Option<u64>,
    pub user_verification_timeout_secs: Option<u64>,
    pub session_timeout_secs: Option<u64>,
    pub max_retries: Option<u8>,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSyncSettingsSummary {
    pub file_sync_enabled: bool,
    pub small_file_threshold: u64,
    pub max_file_size: u64,
    pub file_cache_quota_per_device: u64,
    pub file_retention_hours: u32,
    pub file_auto_cleanup: bool,
    pub max_file_set_total_bytes: u64,
    pub max_file_set_member_count: u64,
    pub auto_save_dir: Option<String>,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSyncSettingsPatch {
    pub file_sync_enabled: Option<bool>,
    pub small_file_threshold: Option<u64>,
    pub max_file_size: Option<u64>,
    pub file_cache_quota_per_device: Option<u64>,
    pub file_retention_hours: Option<u32>,
    pub file_auto_cleanup: Option<bool>,
    pub max_file_set_total_bytes: Option<u64>,
    pub max_file_set_member_count: Option<u64>,
    pub auto_save_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CongestionControllerSummary {
    #[default]
    Cubic,
    Bbr3,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkSettingsSummary {
    pub allow_relay_fallback: bool,
    pub allow_overlay_network_addrs: bool,
    pub custom_relay_urls: Vec<String>,
    pub congestion_controller: CongestionControllerSummary,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkSettingsPatch {
    pub allow_relay_fallback: Option<bool>,
    pub allow_overlay_network_addrs: Option<bool>,
    pub custom_relay_urls: Option<Vec<String>>,
    pub congestion_controller: Option<CongestionControllerSummary>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickPanelPositionSummary {
    #[default]
    Center,
    FollowCursor,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickPanelDoubleTapModifierSummary {
    #[default]
    Disabled,
    Alt,
    Control,
    Meta,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickPanelSettingsSummary {
    pub enabled: bool,
    pub position: QuickPanelPositionSummary,
    pub double_tap_modifier: QuickPanelDoubleTapModifierSummary,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickPanelSettingsPatch {
    pub enabled: Option<bool>,
    pub position: Option<QuickPanelPositionSummary>,
    pub double_tap_modifier: Option<QuickPanelDoubleTapModifierSummary>,
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsSummary {
    pub schema_version: u32,
    pub general: GeneralSettingsSummary,
    pub sync: SyncSettingsSummary,
    pub retention_policy: RetentionPolicySummary,
    pub security: SecuritySettingsSummary,
    pub pairing: PairingSettingsSummary,
    pub keyboard_shortcuts: BTreeMap<String, ShortcutKeySummary>,
    pub file_sync: FileSyncSettingsSummary,
    pub network: NetworkSettingsSummary,
    pub quick_panel: QuickPanelSettingsSummary,
}

impl fmt::Debug for SettingsSummary {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SettingsSummary")
            .field("schema_version", &self.schema_version)
            .field("retention_rule_count", &self.retention_policy.rules.len())
            .field("keyboard_shortcut_count", &self.keyboard_shortcuts.len())
            .field("custom_relay_count", &self.network.custom_relay_urls.len())
            .field("has_auto_save_dir", &self.file_sync.auto_save_dir.is_some())
            .finish()
    }
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsPatch {
    pub general: Option<GeneralSettingsPatch>,
    pub sync: Option<SyncSettingsPatch>,
    pub retention_policy: Option<RetentionPolicyPatch>,
    pub security: Option<SecuritySettingsPatch>,
    pub pairing: Option<PairingSettingsPatch>,
    pub keyboard_shortcuts: Option<BTreeMap<String, Option<ShortcutKeySummary>>>,
    pub file_sync: Option<FileSyncSettingsPatch>,
    pub network: Option<NetworkSettingsPatch>,
    pub quick_panel: Option<QuickPanelSettingsPatch>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SettingsUpdateOutcome {
    Updated(Box<SettingsSummary>),
    Rejected { reason: String },
}

impl fmt::Debug for SettingsUpdateOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Updated(_) => formatter.write_str("SettingsUpdateOutcome::Updated"),
            Self::Rejected { .. } => formatter.write_str("SettingsUpdateOutcome::Rejected"),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayProbeInput {
    pub url: String,
}

impl fmt::Debug for RelayProbeInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RelayProbeInput([REDACTED])")
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayProbeOutcome {
    Success { latency_ms: u32 },
    InvalidUrl { message: String },
    Dns { message: String },
    Tls { message: String },
    Handshake { message: String },
    Timeout,
    Other { message: String },
}

impl fmt::Debug for RelayProbeOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::Success { .. } => "success",
            Self::InvalidUrl { .. } => "invalid_url",
            Self::Dns { .. } => "dns",
            Self::Tls { .. } => "tls",
            Self::Handshake { .. } => "handshake",
            Self::Timeout => "timeout",
            Self::Other { .. } => "other",
        };
        formatter
            .debug_struct("RelayProbeOutcome")
            .field("kind", &kind)
            .finish()
    }
}
