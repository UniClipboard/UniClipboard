use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use uc_core::settings::model as core;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeView {
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateChannelView {
    Stable,
    Alpha,
    Beta,
    Rc,
}

/// Type alias — `ShortcutKeyView` is now `uc_core::settings::model::ShortcutKey`.
/// Kept as a alias for backward-compatible re-export.
pub type ShortcutKeyView = core::ShortcutKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentTypesView {
    pub text: bool,
    pub image: bool,
    pub link: bool,
    pub file: bool,
    pub code_snippet: bool,
    pub rich_text: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentTypesPatch {
    pub text: Option<bool>,
    pub image: Option<bool>,
    pub link: Option<bool>,
    pub file: Option<bool>,
    pub code_snippet: Option<bool>,
    pub rich_text: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncFrequencyView {
    Realtime,
    Interval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleEvaluationView {
    AnyMatch,
    AllMatch,
}

#[derive(Debug, Clone)]
pub enum RetentionRuleView {
    ByAge {
        max_age: Duration,
    },
    ByCount {
        max_items: usize,
    },
    ByContentType {
        content_type: ContentTypesView,
        max_age: Duration,
    },
    ByTotalSize {
        max_bytes: u64,
    },
    Sensitive {
        max_age: Duration,
    },
}

#[derive(Debug, Clone)]
pub struct GeneralSettingsView {
    pub auto_start: bool,
    pub silent_start: bool,
    pub auto_check_update: bool,
    pub auto_download_update: bool,
    pub theme: ThemeView,
    /// 旧版"统一主题预设"字段。新 UI 仅在 light/dark 字段都为 None 时回退。
    /// 删除计划见 `uc_core::settings::model::GeneralSettings::theme_color`。
    pub theme_color: Option<String>,
    /// Light 模式下的主题预设名（如 `"zinc"`）；None 时回退到 `theme_color`。
    pub theme_color_light: Option<String>,
    /// Dark 模式下的主题预设名（如 `"zinc"`）；None 时回退到 `theme_color`。
    pub theme_color_dark: Option<String>,
    /// Light 模式下用户对预设 token 的自定义覆盖（key = token 名, value = oklch 字符串）。
    pub theme_overrides_light: BTreeMap<String, String>,
    /// Dark 模式下用户对预设 token 的自定义覆盖（语义同 light）。
    pub theme_overrides_dark: BTreeMap<String, String>,
    pub language: Option<String>,
    pub device_name: Option<String>,
    pub update_channel: Option<UpdateChannelView>,
    pub telemetry_enabled: bool,
    pub usage_analytics_enabled: bool,
    pub debug_mode: bool,
}

#[derive(Debug, Clone)]
pub struct SyncSettingsView {
    pub auto_sync: bool,
    pub sync_frequency: SyncFrequencyView,
    pub content_types: ContentTypesView,
    pub sync_on_restore: bool,
}

#[derive(Debug, Clone)]
pub struct RetentionPolicyView {
    pub enabled: bool,
    pub rules: Vec<RetentionRuleView>,
    pub skip_pinned: bool,
    pub evaluation: RuleEvaluationView,
}

#[derive(Debug, Clone)]
pub struct SecuritySettingsView {
    pub encryption_enabled: bool,
    pub passphrase_configured: bool,
    pub auto_unlock_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct PairingSettingsView {
    pub step_timeout: Duration,
    pub user_verification_timeout: Duration,
    pub session_timeout: Duration,
    pub max_retries: u8,
    pub protocol_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSyncSettingsView {
    pub file_sync_enabled: bool,
    pub small_file_threshold: u64,
    pub max_file_size: u64,
    pub file_cache_quota_per_device: u64,
    pub file_retention_hours: u32,
    pub file_auto_cleanup: bool,
}

/// LAN-only Mode 业务字段镜像 —— 业务正向语义 `allow_relay_fallback`。
/// UI = "LAN-only Mode = ON" → 此字段 = false。
/// 取反唯一发生在 `uc-bootstrap/src/network_policy.rs`，本层只搬运。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSettingsView {
    pub allow_relay_fallback: bool,
    pub allow_overlay_network_addrs: bool,
    pub custom_relay_urls: Vec<String>,
    pub congestion_controller: CongestionControllerView,
}

/// Mirror of `uc_core::settings::model::CongestionController` for the
/// application layer view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CongestionControllerView {
    Cubic,
    Bbr3,
}

/// 快捷面板出现位置业务镜像。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickPanelPositionView {
    Center,
    FollowCursor,
}

/// 快捷面板功能偏好业务镜像。承载用户对"是否启用 / 出现在哪里"的偏好；
/// 落地副作用（OS 快捷键、窗口生命周期、坐标换算等）由消费此视图的上层负责。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickPanelSettingsView {
    pub enabled: bool,
    pub position: QuickPanelPositionView,
}

#[derive(Debug, Clone)]
pub struct SettingsView {
    pub schema_version: u32,
    pub general: GeneralSettingsView,
    pub sync: SyncSettingsView,
    pub retention_policy: RetentionPolicyView,
    pub security: SecuritySettingsView,
    pub pairing: PairingSettingsView,
    pub keyboard_shortcuts: HashMap<String, ShortcutKeyView>,
    pub file_sync: FileSyncSettingsView,
    pub network: NetworkSettingsView,
    pub quick_panel: QuickPanelSettingsView,
}

#[derive(Debug, Clone, Default)]
pub struct GeneralSettingsPatch {
    pub auto_start: Option<bool>,
    pub silent_start: Option<bool>,
    pub auto_check_update: Option<bool>,
    pub auto_download_update: Option<bool>,
    pub theme: Option<ThemeView>,
    /// 旧版"统一主题预设"字段。新 UI 不再写入,但仍保留 patch 入口便于
    /// 显式清空（`Some(None)`）旧字段或在迁移工具里使用。
    pub theme_color: Option<Option<String>>,
    /// Light 模式下的主题预设名 patch；`Some(None)` = 显式清空,`None` = 不修改。
    pub theme_color_light: Option<Option<String>>,
    /// Dark 模式下的主题预设名 patch；`Some(None)` = 显式清空,`None` = 不修改。
    pub theme_color_dark: Option<Option<String>>,
    /// Light 模式 overrides patch。`Some(map)` 整体替换；`None` 表示不修改。
    pub theme_overrides_light: Option<BTreeMap<String, String>>,
    /// Dark 模式 overrides patch（语义同 light）。
    pub theme_overrides_dark: Option<BTreeMap<String, String>>,
    pub language: Option<Option<String>>,
    pub device_name: Option<Option<String>>,
    pub update_channel: Option<Option<UpdateChannelView>>,
    pub telemetry_enabled: Option<bool>,
    pub usage_analytics_enabled: Option<bool>,
    pub debug_mode: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct SyncSettingsPatch {
    pub auto_sync: Option<bool>,
    pub sync_frequency: Option<SyncFrequencyView>,
    pub content_types: Option<ContentTypesPatch>,
    pub sync_on_restore: Option<bool>,
}

#[derive(Debug, Clone)]
pub enum RetentionRulePatchValue {
    ByAge {
        max_age: Duration,
    },
    ByCount {
        max_items: usize,
    },
    ByContentType {
        content_type: ContentTypesView,
        max_age: Duration,
    },
    ByTotalSize {
        max_bytes: u64,
    },
    Sensitive {
        max_age: Duration,
    },
}

#[derive(Debug, Clone, Default)]
pub struct RetentionPolicyPatch {
    pub enabled: Option<bool>,
    pub rules: Option<Vec<RetentionRulePatchValue>>,
    pub skip_pinned: Option<bool>,
    pub evaluation: Option<RuleEvaluationView>,
}

#[derive(Debug, Clone, Default)]
pub struct SecuritySettingsPatch {
    pub encryption_enabled: Option<bool>,
    pub auto_unlock_enabled: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct PairingSettingsPatch {
    pub step_timeout: Option<Duration>,
    pub user_verification_timeout: Option<Duration>,
    pub session_timeout: Option<Duration>,
    pub max_retries: Option<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct FileSyncSettingsPatch {
    pub file_sync_enabled: Option<bool>,
    pub small_file_threshold: Option<u64>,
    pub max_file_size: Option<u64>,
    pub file_cache_quota_per_device: Option<u64>,
    pub file_retention_hours: Option<u32>,
    pub file_auto_cleanup: Option<bool>,
}

/// LAN-only Mode 字段 patch 镜像 —— `None` = 不修改。
#[derive(Debug, Clone, Default)]
pub struct NetworkSettingsPatch {
    pub allow_relay_fallback: Option<bool>,
    pub allow_overlay_network_addrs: Option<bool>,
    pub custom_relay_urls: Option<Vec<String>>,
    pub congestion_controller: Option<CongestionControllerView>,
}

/// 快捷面板字段 patch 镜像 —— `None` = 不修改。
#[derive(Debug, Clone, Default)]
pub struct QuickPanelSettingsPatch {
    pub enabled: Option<bool>,
    pub position: Option<QuickPanelPositionView>,
}

#[derive(Debug, Clone, Default)]
pub struct SettingsPatch {
    pub general: Option<GeneralSettingsPatch>,
    pub sync: Option<SyncSettingsPatch>,
    pub retention_policy: Option<RetentionPolicyPatch>,
    pub security: Option<SecuritySettingsPatch>,
    pub pairing: Option<PairingSettingsPatch>,
    pub keyboard_shortcuts: Option<HashMap<String, Option<ShortcutKeyView>>>,
    pub file_sync: Option<FileSyncSettingsPatch>,
    pub network: Option<NetworkSettingsPatch>,
    pub quick_panel: Option<QuickPanelSettingsPatch>,
}

impl From<core::Theme> for ThemeView {
    fn from(value: core::Theme) -> Self {
        match value {
            core::Theme::Light => Self::Light,
            core::Theme::Dark => Self::Dark,
            core::Theme::System => Self::System,
        }
    }
}

impl From<ThemeView> for core::Theme {
    fn from(value: ThemeView) -> Self {
        match value {
            ThemeView::Light => Self::Light,
            ThemeView::Dark => Self::Dark,
            ThemeView::System => Self::System,
        }
    }
}

impl From<core::UpdateChannel> for UpdateChannelView {
    fn from(value: core::UpdateChannel) -> Self {
        match value {
            core::UpdateChannel::Stable => Self::Stable,
            core::UpdateChannel::Alpha => Self::Alpha,
            core::UpdateChannel::Beta => Self::Beta,
            core::UpdateChannel::Rc => Self::Rc,
        }
    }
}

impl From<core::QuickPanelPosition> for QuickPanelPositionView {
    fn from(value: core::QuickPanelPosition) -> Self {
        match value {
            core::QuickPanelPosition::Center => Self::Center,
            core::QuickPanelPosition::FollowCursor => Self::FollowCursor,
        }
    }
}

impl From<QuickPanelPositionView> for core::QuickPanelPosition {
    fn from(value: QuickPanelPositionView) -> Self {
        match value {
            QuickPanelPositionView::Center => Self::Center,
            QuickPanelPositionView::FollowCursor => Self::FollowCursor,
        }
    }
}

impl From<core::CongestionController> for CongestionControllerView {
    fn from(value: core::CongestionController) -> Self {
        match value {
            core::CongestionController::Cubic => Self::Cubic,
            core::CongestionController::Bbr3 => Self::Bbr3,
        }
    }
}

impl From<CongestionControllerView> for core::CongestionController {
    fn from(value: CongestionControllerView) -> Self {
        match value {
            CongestionControllerView::Cubic => Self::Cubic,
            CongestionControllerView::Bbr3 => Self::Bbr3,
        }
    }
}

impl From<UpdateChannelView> for core::UpdateChannel {
    fn from(value: UpdateChannelView) -> Self {
        match value {
            UpdateChannelView::Stable => Self::Stable,
            UpdateChannelView::Alpha => Self::Alpha,
            UpdateChannelView::Beta => Self::Beta,
            UpdateChannelView::Rc => Self::Rc,
        }
    }
}

// From<ShortcutKey> for ShortcutKeyView and vice versa removed —
// ShortcutKeyView is now a type alias for core::ShortcutKey, so the
// blanket From<T> for T handles identity conversion.

impl From<core::ContentTypes> for ContentTypesView {
    fn from(value: core::ContentTypes) -> Self {
        Self {
            text: value.text,
            image: value.image,
            link: value.link,
            file: value.file,
            code_snippet: value.code_snippet,
            rich_text: value.rich_text,
        }
    }
}

impl From<ContentTypesView> for core::ContentTypes {
    fn from(value: ContentTypesView) -> Self {
        Self {
            text: value.text,
            image: value.image,
            link: value.link,
            file: value.file,
            code_snippet: value.code_snippet,
            rich_text: value.rich_text,
        }
    }
}

impl From<core::SyncFrequency> for SyncFrequencyView {
    fn from(value: core::SyncFrequency) -> Self {
        match value {
            core::SyncFrequency::Realtime => Self::Realtime,
            core::SyncFrequency::Interval => Self::Interval,
        }
    }
}

impl From<SyncFrequencyView> for core::SyncFrequency {
    fn from(value: SyncFrequencyView) -> Self {
        match value {
            SyncFrequencyView::Realtime => Self::Realtime,
            SyncFrequencyView::Interval => Self::Interval,
        }
    }
}

impl From<core::RuleEvaluation> for RuleEvaluationView {
    fn from(value: core::RuleEvaluation) -> Self {
        match value {
            core::RuleEvaluation::AnyMatch => Self::AnyMatch,
            core::RuleEvaluation::AllMatch => Self::AllMatch,
        }
    }
}

impl From<RuleEvaluationView> for core::RuleEvaluation {
    fn from(value: RuleEvaluationView) -> Self {
        match value {
            RuleEvaluationView::AnyMatch => Self::AnyMatch,
            RuleEvaluationView::AllMatch => Self::AllMatch,
        }
    }
}

impl From<core::RetentionRule> for RetentionRuleView {
    fn from(value: core::RetentionRule) -> Self {
        match value {
            core::RetentionRule::ByAge { max_age } => Self::ByAge { max_age },
            core::RetentionRule::ByCount { max_items } => Self::ByCount { max_items },
            core::RetentionRule::ByContentType {
                content_type,
                max_age,
            } => Self::ByContentType {
                content_type: content_type.into(),
                max_age,
            },
            core::RetentionRule::ByTotalSize { max_bytes } => Self::ByTotalSize { max_bytes },
            core::RetentionRule::Sensitive { max_age } => Self::Sensitive { max_age },
        }
    }
}

impl From<RetentionRulePatchValue> for core::RetentionRule {
    fn from(value: RetentionRulePatchValue) -> Self {
        match value {
            RetentionRulePatchValue::ByAge { max_age } => Self::ByAge { max_age },
            RetentionRulePatchValue::ByCount { max_items } => Self::ByCount { max_items },
            RetentionRulePatchValue::ByContentType {
                content_type,
                max_age,
            } => Self::ByContentType {
                content_type: content_type.into(),
                max_age,
            },
            RetentionRulePatchValue::ByTotalSize { max_bytes } => Self::ByTotalSize { max_bytes },
            RetentionRulePatchValue::Sensitive { max_age } => Self::Sensitive { max_age },
        }
    }
}

impl From<core::Settings> for SettingsView {
    fn from(value: core::Settings) -> Self {
        Self {
            schema_version: value.schema_version,
            general: GeneralSettingsView {
                auto_start: value.general.auto_start,
                silent_start: value.general.silent_start,
                auto_check_update: value.general.auto_check_update,
                auto_download_update: value.general.auto_download_update,
                theme: value.general.theme.into(),
                theme_color: value.general.theme_color,
                theme_color_light: value.general.theme_color_light,
                theme_color_dark: value.general.theme_color_dark,
                theme_overrides_light: value.general.theme_overrides_light,
                theme_overrides_dark: value.general.theme_overrides_dark,
                language: value.general.language,
                device_name: value.general.device_name,
                update_channel: value.general.update_channel.map(Into::into),
                telemetry_enabled: value.general.telemetry_enabled,
                usage_analytics_enabled: value.general.usage_analytics_enabled,
                debug_mode: value.general.debug_mode,
            },
            sync: SyncSettingsView {
                auto_sync: value.sync.auto_sync,
                sync_frequency: value.sync.sync_frequency.into(),
                content_types: value.sync.content_types.into(),
                sync_on_restore: value.sync.sync_on_restore,
            },
            retention_policy: RetentionPolicyView {
                enabled: value.retention_policy.enabled,
                rules: value
                    .retention_policy
                    .rules
                    .into_iter()
                    .map(Into::into)
                    .collect(),
                skip_pinned: value.retention_policy.skip_pinned,
                evaluation: value.retention_policy.evaluation.into(),
            },
            security: SecuritySettingsView {
                encryption_enabled: value.security.encryption_enabled,
                passphrase_configured: value.security.passphrase_configured,
                auto_unlock_enabled: value.security.auto_unlock_enabled,
            },
            pairing: PairingSettingsView {
                step_timeout: value.pairing.step_timeout,
                user_verification_timeout: value.pairing.user_verification_timeout,
                session_timeout: value.pairing.session_timeout,
                max_retries: value.pairing.max_retries,
                protocol_version: value.pairing.protocol_version,
            },
            keyboard_shortcuts: value
                .keyboard_shortcuts
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
            file_sync: FileSyncSettingsView {
                file_sync_enabled: value.file_sync.file_sync_enabled,
                small_file_threshold: value.file_sync.small_file_threshold,
                max_file_size: value.file_sync.max_file_size,
                file_cache_quota_per_device: value.file_sync.file_cache_quota_per_device,
                file_retention_hours: value.file_sync.file_retention_hours,
                file_auto_cleanup: value.file_sync.file_auto_cleanup,
            },
            network: NetworkSettingsView {
                allow_relay_fallback: value.network.allow_relay_fallback,
                allow_overlay_network_addrs: value.network.allow_overlay_network_addrs,
                custom_relay_urls: value.network.custom_relay_urls,
                congestion_controller: value.network.congestion_controller.into(),
            },
            quick_panel: QuickPanelSettingsView {
                enabled: value.quick_panel.enabled,
                position: value.quick_panel.position.into(),
            },
        }
    }
}

pub(crate) fn apply_settings_patch(
    mut existing: core::Settings,
    patch: SettingsPatch,
) -> core::Settings {
    if let Some(general) = patch.general {
        if let Some(v) = general.auto_start {
            existing.general.auto_start = v;
        }
        if let Some(v) = general.silent_start {
            existing.general.silent_start = v;
        }
        if let Some(v) = general.auto_check_update {
            existing.general.auto_check_update = v;
        }
        if let Some(v) = general.auto_download_update {
            existing.general.auto_download_update = v;
        }
        if let Some(v) = general.theme {
            existing.general.theme = v.into();
        }
        if let Some(v) = general.theme_color {
            existing.general.theme_color = v;
        }
        if let Some(v) = general.theme_color_light {
            existing.general.theme_color_light = v;
        }
        if let Some(v) = general.theme_color_dark {
            existing.general.theme_color_dark = v;
        }
        if let Some(v) = general.theme_overrides_light {
            existing.general.theme_overrides_light = v;
        }
        if let Some(v) = general.theme_overrides_dark {
            existing.general.theme_overrides_dark = v;
        }
        if let Some(v) = general.language {
            existing.general.language = v;
        }
        if let Some(v) = general.device_name {
            existing.general.device_name = v;
        }
        if let Some(v) = general.update_channel {
            existing.general.update_channel = v.map(Into::into);
        }
        if let Some(v) = general.telemetry_enabled {
            existing.general.telemetry_enabled = v;
        }
        if let Some(v) = general.usage_analytics_enabled {
            existing.general.usage_analytics_enabled = v;
        }
        if let Some(v) = general.debug_mode {
            existing.general.debug_mode = v;
        }
    }

    if let Some(sync) = patch.sync {
        if let Some(v) = sync.auto_sync {
            existing.sync.auto_sync = v;
        }
        if let Some(v) = sync.sync_frequency {
            existing.sync.sync_frequency = v.into();
        }
        if let Some(content_types) = sync.content_types {
            apply_content_types_patch(&mut existing.sync.content_types, content_types);
        }
        if let Some(v) = sync.sync_on_restore {
            existing.sync.sync_on_restore = v;
        }
    }

    if let Some(retention_policy) = patch.retention_policy {
        if let Some(v) = retention_policy.enabled {
            existing.retention_policy.enabled = v;
        }
        if let Some(v) = retention_policy.skip_pinned {
            existing.retention_policy.skip_pinned = v;
        }
        if let Some(v) = retention_policy.evaluation {
            existing.retention_policy.evaluation = v.into();
        }
        if let Some(rules) = retention_policy.rules {
            existing.retention_policy.rules = rules.into_iter().map(Into::into).collect();
        }
    }

    if let Some(security) = patch.security {
        if let Some(v) = security.encryption_enabled {
            existing.security.encryption_enabled = v;
        }
        if let Some(v) = security.auto_unlock_enabled {
            existing.security.auto_unlock_enabled = v;
        }
    }

    if let Some(pairing) = patch.pairing {
        if let Some(v) = pairing.step_timeout {
            existing.pairing.step_timeout = v;
        }
        if let Some(v) = pairing.user_verification_timeout {
            existing.pairing.user_verification_timeout = v;
        }
        if let Some(v) = pairing.session_timeout {
            existing.pairing.session_timeout = v;
        }
        if let Some(v) = pairing.max_retries {
            existing.pairing.max_retries = v;
        }
    }

    if let Some(keyboard_shortcuts) = patch.keyboard_shortcuts {
        for (name, value) in keyboard_shortcuts {
            match value {
                Some(shortcut) => {
                    existing.keyboard_shortcuts.insert(name, shortcut.into());
                }
                None => {
                    existing.keyboard_shortcuts.remove(&name);
                }
            }
        }
    }

    if let Some(file_sync) = patch.file_sync {
        if let Some(v) = file_sync.file_sync_enabled {
            existing.file_sync.file_sync_enabled = v;
        }
        if let Some(v) = file_sync.small_file_threshold {
            existing.file_sync.small_file_threshold = v;
        }
        if let Some(v) = file_sync.max_file_size {
            existing.file_sync.max_file_size = v;
        }
        if let Some(v) = file_sync.file_cache_quota_per_device {
            existing.file_sync.file_cache_quota_per_device = v;
        }
        if let Some(v) = file_sync.file_retention_hours {
            existing.file_sync.file_retention_hours = v;
        }
        if let Some(v) = file_sync.file_auto_cleanup {
            existing.file_sync.file_auto_cleanup = v;
        }
    }

    if let Some(network) = patch.network {
        if let Some(v) = network.allow_relay_fallback {
            existing.network.allow_relay_fallback = v;
        }
        if let Some(v) = network.allow_overlay_network_addrs {
            existing.network.allow_overlay_network_addrs = v;
        }
        if let Some(v) = network.custom_relay_urls {
            existing.network.custom_relay_urls = normalize_relay_urls(v);
        }
        if let Some(v) = network.congestion_controller {
            existing.network.congestion_controller = v.into();
        }
    }

    if let Some(quick_panel) = patch.quick_panel {
        if let Some(v) = quick_panel.enabled {
            existing.quick_panel.enabled = v;
        }
        if let Some(v) = quick_panel.position {
            existing.quick_panel.position = v.into();
        }
    }

    existing
}

fn normalize_relay_urls(urls: Vec<String>) -> Vec<String> {
    urls.into_iter()
        .map(|url| url.trim().to_string())
        .filter(|url| !url.is_empty())
        .collect()
}

pub(crate) fn validate_settings(settings: &core::Settings) -> Result<(), String> {
    validate_custom_relay_urls(&settings.network.custom_relay_urls)
}

fn validate_custom_relay_urls(urls: &[String]) -> Result<(), String> {
    for raw in urls {
        let url = url::Url::parse(raw)
            .map_err(|err| format!("invalid custom relay URL `{raw}`: {err}"))?;
        let scheme = url.scheme();
        if scheme != "http" && scheme != "https" {
            return Err(format!(
                "invalid custom relay URL `{raw}`: scheme must be http or https"
            ));
        }
        if url.host_str().is_none() {
            return Err(format!(
                "invalid custom relay URL `{raw}`: host is required"
            ));
        }
    }
    Ok(())
}

fn apply_content_types_patch(target: &mut core::ContentTypes, patch: ContentTypesPatch) {
    if let Some(v) = patch.text {
        target.text = v;
    }
    if let Some(v) = patch.image {
        target.image = v;
    }
    if let Some(v) = patch.link {
        target.link = v;
    }
    if let Some(v) = patch.file {
        target.file = v;
    }
    if let Some(v) = patch.code_snippet {
        target.code_snippet = v;
    }
    if let Some(v) = patch.rich_text {
        target.rich_text = v;
    }
}

#[cfg(test)]
mod network_settings_apply_patch_tests {
    use super::*;
    use uc_core::settings::model::{NetworkSettings, Settings};

    fn baseline_with_network(allow: bool) -> Settings {
        let mut s = Settings::default();
        s.network = NetworkSettings {
            allow_relay_fallback: allow,
            allow_overlay_network_addrs: false,
            custom_relay_urls: Vec::new(),
            congestion_controller: Default::default(),
        };
        s
    }

    fn baseline_with_overlay(allow_overlay: bool) -> Settings {
        let mut s = Settings::default();
        s.network = NetworkSettings {
            allow_relay_fallback: true,
            allow_overlay_network_addrs: allow_overlay,
            custom_relay_urls: Vec::new(),
            congestion_controller: Default::default(),
        };
        s
    }

    /// NETSET-02 #2 硬约束：旧客户端 PUT 不带 `network` 段时，
    /// 不抹掉已存在 `network` 字段。
    #[test]
    fn apply_patch_with_no_network_section_keeps_existing() {
        let existing = baseline_with_network(false);
        let patch = SettingsPatch::default(); // patch.network = None
        let result = apply_settings_patch(existing, patch);
        assert!(
            !result.network.allow_relay_fallback,
            "patch.network=None must keep existing false"
        );
    }

    /// 嵌套 None — patch.network = Some 但内部字段 None — 仍不抹掉。
    #[test]
    fn apply_patch_with_empty_network_section_keeps_existing() {
        let existing = baseline_with_network(true);
        let patch = SettingsPatch {
            network: Some(NetworkSettingsPatch::default()),
            ..Default::default()
        };
        let result = apply_settings_patch(existing, patch);
        assert!(
            result.network.allow_relay_fallback,
            "inner None must keep existing true"
        );
    }

    /// 显式 Some(false) 写入生效。
    #[test]
    fn apply_patch_with_explicit_false_writes_through() {
        let existing = baseline_with_network(true);
        let patch = SettingsPatch {
            network: Some(NetworkSettingsPatch {
                allow_relay_fallback: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = apply_settings_patch(existing, patch);
        assert!(!result.network.allow_relay_fallback);
    }

    /// 显式 Some(true) 写入生效（双向覆盖）。
    #[test]
    fn apply_patch_with_explicit_true_writes_through() {
        let existing = baseline_with_network(false);
        let patch = SettingsPatch {
            network: Some(NetworkSettingsPatch {
                allow_relay_fallback: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = apply_settings_patch(existing, patch);
        assert!(result.network.allow_relay_fallback);
    }

    /// From<core::Settings> 透明搬运（不取反）。
    #[test]
    fn from_core_settings_passes_through_business_semantics() {
        let mut s = Settings::default();
        s.network.allow_relay_fallback = false;
        let view: SettingsView = s.into();
        assert!(
            !view.network.allow_relay_fallback,
            "view 必须保留业务正向语义，不取反"
        );
    }

    /// allow_overlay_network_addrs：patch 缺字段时不抹掉已存在值。
    #[test]
    fn apply_patch_with_no_overlay_field_keeps_existing() {
        let existing = baseline_with_overlay(true);
        let patch = SettingsPatch {
            network: Some(NetworkSettingsPatch::default()),
            ..Default::default()
        };
        let result = apply_settings_patch(existing, patch);
        assert!(
            result.network.allow_overlay_network_addrs,
            "inner None must keep existing true"
        );
    }

    /// allow_overlay_network_addrs：显式 Some(true) 写入生效。
    #[test]
    fn apply_patch_with_overlay_explicit_true_writes_through() {
        let existing = baseline_with_overlay(false);
        let patch = SettingsPatch {
            network: Some(NetworkSettingsPatch {
                allow_overlay_network_addrs: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = apply_settings_patch(existing, patch);
        assert!(result.network.allow_overlay_network_addrs);
    }

    /// allow_overlay_network_addrs：显式 Some(false) 双向覆盖。
    #[test]
    fn apply_patch_with_overlay_explicit_false_writes_through() {
        let existing = baseline_with_overlay(true);
        let patch = SettingsPatch {
            network: Some(NetworkSettingsPatch {
                allow_overlay_network_addrs: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = apply_settings_patch(existing, patch);
        assert!(!result.network.allow_overlay_network_addrs);
    }

    /// View 透明搬运 allow_overlay_network_addrs。
    #[test]
    fn from_core_settings_passes_through_overlay_field() {
        let mut s = Settings::default();
        s.network.allow_overlay_network_addrs = true;
        let view: SettingsView = s.into();
        assert!(view.network.allow_overlay_network_addrs);
    }

    /// custom_relay_urls：patch 缺字段时不抹掉已存在列表。
    #[test]
    fn apply_patch_with_no_custom_relay_urls_keeps_existing() {
        let mut existing = baseline_with_network(true);
        existing.network.custom_relay_urls = vec!["https://relay.example.com.".to_string()];
        let patch = SettingsPatch {
            network: Some(NetworkSettingsPatch::default()),
            ..Default::default()
        };
        let result = apply_settings_patch(existing, patch);
        assert_eq!(
            result.network.custom_relay_urls,
            vec!["https://relay.example.com.".to_string()]
        );
    }

    /// custom_relay_urls：显式 Some(list) 会 trim，并过滤空行。
    #[test]
    fn apply_patch_with_custom_relay_urls_normalizes_values() {
        let existing = baseline_with_network(true);
        let patch = SettingsPatch {
            network: Some(NetworkSettingsPatch {
                custom_relay_urls: Some(vec![
                    " https://relay-a.example.com. ".to_string(),
                    "".to_string(),
                    "https://relay-b.example.com.".to_string(),
                ]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = apply_settings_patch(existing, patch);
        assert_eq!(
            result.network.custom_relay_urls,
            vec![
                "https://relay-a.example.com.".to_string(),
                "https://relay-b.example.com.".to_string()
            ]
        );
    }

    /// custom_relay_urls：View 透明搬运，不在 application 层转换成 iroh 类型。
    #[test]
    fn from_core_settings_passes_through_custom_relay_urls() {
        let mut s = Settings::default();
        s.network.custom_relay_urls = vec!["https://relay.example.com.".to_string()];
        let view: SettingsView = s.into();
        assert_eq!(
            view.network.custom_relay_urls,
            vec!["https://relay.example.com.".to_string()]
        );
    }

    /// custom_relay_urls：只接受 HTTP(S) relay URL。
    #[test]
    fn validate_custom_relay_urls_rejects_invalid_scheme() {
        let urls = vec!["ftp://relay.example.com".to_string()];
        let err = validate_custom_relay_urls(&urls).expect_err("invalid scheme");
        assert!(err.contains("scheme must be http or https"));
    }

    /// custom_relay_urls：合法 HTTP(S) URL 通过校验。
    #[test]
    fn validate_custom_relay_urls_accepts_http_urls() {
        let urls = vec![
            "https://relay-a.example.com.".to_string(),
            "http://127.0.0.1:3340".to_string(),
        ];
        validate_custom_relay_urls(&urls).expect("valid relay urls");
    }
}
