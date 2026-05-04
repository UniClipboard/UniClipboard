use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DurationSeconds};
use utoipa::ToSchema;

use uc_core::settings::model as core;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetSettingsResponse {
    pub data: SettingsDto,
    pub ts: i64,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsResponse {
    pub success: bool,
    pub data: SettingsDto,
    pub ts: i64,
    /// 表示本次 patch 涉及需要重启 daemon 才能生效的字段（目前仅 `network.*`）。
    /// 该字段由 webserver handler 内联计算（plan 04），不依赖 application facade
    /// 公共签名变更（D-D1 / Pitfall 3 防御 — 调用方显式承担"还没真正生效"）。
    pub restart_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettingsDto {
    pub auto_start: bool,
    pub silent_start: bool,
    pub auto_check_update: bool,
    pub theme: ThemeDto,
    pub theme_color: Option<String>,
    pub language: Option<String>,
    pub device_name: Option<String>,
    /// Update channel preference. `None` means auto-detect from version string;
    /// `Some(channel)` means the user has overridden the channel.
    #[serde(default)]
    pub update_channel: Option<UpdateChannelDto>,
    /// Whether anonymous diagnostic telemetry is enabled.
    pub telemetry_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ThemeDto {
    Light,
    Dark,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannelDto {
    Stable,
    Alpha,
    Beta,
    Rc,
}

/// A keyboard shortcut value that can be either a single key combo or multiple alternatives.
///
/// Serialised with `#[serde(untagged)]` so that `"Ctrl+C"` and `["Ctrl+C","Meta+C"]` are both
/// accepted without a wrapping tag, matching the TypeScript type `string | string[]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(untagged)]
pub enum ShortcutKeyDto {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContentTypesDto {
    pub text: bool,
    pub image: bool,
    pub link: bool,
    pub file: bool,
    pub code_snippet: bool,
    pub rich_text: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncSettingsDto {
    pub auto_sync: bool,
    pub sync_frequency: SyncFrequencyDto,
    pub content_types: ContentTypesDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SyncFrequencyDto {
    Realtime,
    Interval,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum RetentionRuleDto {
    /// 按时间清理
    ByAge {
        #[serde_as(as = "DurationSeconds<u64>")]
        #[schema(value_type = u64)]
        max_age: Duration,
    },

    /// 按总数量上限
    ByCount { max_items: usize },

    /// 按内容类型的最大存活时间
    ByContentType {
        content_type: ContentTypesDto,
        #[serde_as(as = "DurationSeconds<u64>")]
        #[schema(value_type = u64)]
        max_age: Duration,
    },

    /// 按磁盘占用大小
    ByTotalSize { max_bytes: u64 },

    /// 敏感内容快速过期
    Sensitive {
        #[serde_as(as = "DurationSeconds<u64>")]
        #[schema(value_type = u64)]
        max_age: Duration,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum RuleEvaluationDto {
    AnyMatch,
    AllMatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPolicyDto {
    pub enabled: bool,
    pub rules: Vec<RetentionRuleDto>,
    pub skip_pinned: bool,
    pub evaluation: RuleEvaluationDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySettingsDto {
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
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PairingSettingsDto {
    #[serde_as(as = "DurationSeconds<u64>")]
    #[schema(value_type = u64)]
    pub step_timeout: Duration,

    #[serde_as(as = "DurationSeconds<u64>")]
    #[schema(value_type = u64)]
    pub user_verification_timeout: Duration,

    #[serde_as(as = "DurationSeconds<u64>")]
    #[schema(value_type = u64)]
    pub session_timeout: Duration,

    pub max_retries: u8,
    pub protocol_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileSyncSettingsDto {
    pub file_sync_enabled: bool,
    pub small_file_threshold: u64,
    pub max_file_size: u64,
    pub file_cache_quota_per_device: u64,
    pub file_retention_hours: u32,
    pub file_auto_cleanup: bool,
}

/// LAN-only Mode（v0.7.0）DTO 镜像。
///
/// 反向命名规则（Pitfall 1）：业务正向语义 `allow_relay_fallback`，
/// 不在此层重命名为 `lan_only` 或类似镜像。wire 字段 = `allowRelayFallback`
/// （camelCase 自动转换）。取反唯一发生在 `uc-bootstrap/src/network_policy.rs`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettingsDto {
    pub allow_relay_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    pub schema_version: u32,
    pub general: GeneralSettingsDto,
    pub sync: SyncSettingsDto,
    pub retention_policy: RetentionPolicyDto,
    pub security: SecuritySettingsDto,
    pub pairing: PairingSettingsDto,
    pub keyboard_shortcuts: HashMap<String, ShortcutKeyDto>,
    pub file_sync: FileSyncSettingsDto,
    pub network: NetworkSettingsDto,
}

// =========================
// Patch DTOs
// =========================

/// All fields are optional — only provided fields are updated.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettingsPatchDto {
    pub auto_start: Option<bool>,
    pub silent_start: Option<bool>,
    pub auto_check_update: Option<bool>,
    pub theme: Option<ThemeDto>,
    pub theme_color: Option<Option<String>>,
    pub language: Option<Option<String>>,
    pub device_name: Option<Option<String>>,
    pub update_channel: Option<Option<UpdateChannelDto>>,
    pub telemetry_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContentTypesPatchDto {
    pub text: Option<bool>,
    pub image: Option<bool>,
    pub link: Option<bool>,
    pub file: Option<bool>,
    pub code_snippet: Option<bool>,
    pub rich_text: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SyncSettingsPatchDto {
    pub auto_sync: Option<bool>,
    pub sync_frequency: Option<SyncFrequencyDto>,
    pub content_types: Option<ContentTypesPatchDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RetentionPolicyPatchDto {
    pub enabled: Option<bool>,
    pub rules: Option<Vec<RetentionRuleDto>>,
    pub skip_pinned: Option<bool>,
    pub evaluation: Option<RuleEvaluationDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecuritySettingsPatchDto {
    /// 写入时设置是否启用本地数据加密（需要 passphrase）
    pub encryption_enabled: Option<bool>,
    /// 写入时设置是否启用启动时自动解锁
    pub auto_unlock_enabled: Option<bool>,
    /// 写入时设置 passphrase（由前端/daemon 内部触发解锁流程）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PairingSettingsPatchDto {
    #[serde_as(as = "Option<DurationSeconds<u64>>")]
    #[schema(value_type = Option<u64>)]
    pub step_timeout: Option<Duration>,

    #[serde_as(as = "Option<DurationSeconds<u64>>")]
    #[schema(value_type = Option<u64>)]
    pub user_verification_timeout: Option<Duration>,

    #[serde_as(as = "Option<DurationSeconds<u64>>")]
    #[schema(value_type = Option<u64>)]
    pub session_timeout: Option<Duration>,

    pub max_retries: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileSyncSettingsPatchDto {
    pub file_sync_enabled: Option<bool>,
    pub small_file_threshold: Option<u64>,
    pub max_file_size: Option<u64>,
    pub file_cache_quota_per_device: Option<u64>,
    pub file_retention_hours: Option<u32>,
    pub file_auto_cleanup: Option<bool>,
}

/// LAN-only Mode 字段 patch DTO 镜像 — `null` = 不修改。
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettingsPatchDto {
    pub allow_relay_fallback: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardShortcutsPatchDto {
    pub shortcuts: HashMap<String, Option<ShortcutKeyDto>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatchDto {
    pub general: Option<GeneralSettingsPatchDto>,
    pub sync: Option<SyncSettingsPatchDto>,
    pub retention_policy: Option<RetentionPolicyPatchDto>,
    pub security: Option<SecuritySettingsPatchDto>,
    pub pairing: Option<PairingSettingsPatchDto>,
    pub keyboard_shortcuts: Option<KeyboardShortcutsPatchDto>,
    pub file_sync: Option<FileSyncSettingsPatchDto>,
    pub network: Option<NetworkSettingsPatchDto>,
}

// =========================
// From<core model> for DTO
// =========================

impl From<core::Theme> for ThemeDto {
    fn from(value: core::Theme) -> Self {
        match value {
            core::Theme::Light => Self::Light,
            core::Theme::Dark => Self::Dark,
            core::Theme::System => Self::System,
        }
    }
}

impl From<core::UpdateChannel> for UpdateChannelDto {
    fn from(value: core::UpdateChannel) -> Self {
        match value {
            core::UpdateChannel::Stable => Self::Stable,
            core::UpdateChannel::Alpha => Self::Alpha,
            core::UpdateChannel::Beta => Self::Beta,
            core::UpdateChannel::Rc => Self::Rc,
        }
    }
}

impl From<core::ShortcutKey> for ShortcutKeyDto {
    fn from(value: core::ShortcutKey) -> Self {
        match value {
            core::ShortcutKey::Single(v) => Self::Single(v),
            core::ShortcutKey::Multiple(v) => Self::Multiple(v),
        }
    }
}

impl From<core::ContentTypes> for ContentTypesDto {
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

impl From<core::SyncFrequency> for SyncFrequencyDto {
    fn from(value: core::SyncFrequency) -> Self {
        match value {
            core::SyncFrequency::Realtime => Self::Realtime,
            core::SyncFrequency::Interval => Self::Interval,
        }
    }
}

impl From<core::GeneralSettings> for GeneralSettingsDto {
    fn from(value: core::GeneralSettings) -> Self {
        Self {
            auto_start: value.auto_start,
            silent_start: value.silent_start,
            auto_check_update: value.auto_check_update,
            theme: value.theme.into(),
            theme_color: value.theme_color,
            language: value.language,
            device_name: value.device_name,
            update_channel: value.update_channel.map(Into::into),
            telemetry_enabled: value.telemetry_enabled,
        }
    }
}

impl From<core::SyncSettings> for SyncSettingsDto {
    fn from(value: core::SyncSettings) -> Self {
        Self {
            auto_sync: value.auto_sync,
            sync_frequency: value.sync_frequency.into(),
            content_types: value.content_types.into(),
        }
    }
}

impl From<core::RetentionRule> for RetentionRuleDto {
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

impl From<core::RuleEvaluation> for RuleEvaluationDto {
    fn from(value: core::RuleEvaluation) -> Self {
        match value {
            core::RuleEvaluation::AnyMatch => Self::AnyMatch,
            core::RuleEvaluation::AllMatch => Self::AllMatch,
        }
    }
}

impl From<core::RetentionPolicy> for RetentionPolicyDto {
    fn from(value: core::RetentionPolicy) -> Self {
        Self {
            enabled: value.enabled,
            rules: value.rules.into_iter().map(Into::into).collect(),
            skip_pinned: value.skip_pinned,
            evaluation: value.evaluation.into(),
        }
    }
}

impl From<core::SecuritySettings> for SecuritySettingsDto {
    fn from(value: core::SecuritySettings) -> Self {
        Self {
            encryption_enabled: value.encryption_enabled,
            passphrase_configured: value.passphrase_configured,
            auto_unlock_enabled: value.auto_unlock_enabled,
        }
    }
}

impl From<core::PairingSettings> for PairingSettingsDto {
    fn from(value: core::PairingSettings) -> Self {
        Self {
            step_timeout: value.step_timeout,
            user_verification_timeout: value.user_verification_timeout,
            session_timeout: value.session_timeout,
            max_retries: value.max_retries,
            protocol_version: value.protocol_version,
        }
    }
}

impl From<core::FileSyncSettings> for FileSyncSettingsDto {
    fn from(value: core::FileSyncSettings) -> Self {
        Self {
            file_sync_enabled: value.file_sync_enabled,
            small_file_threshold: value.small_file_threshold,
            max_file_size: value.max_file_size,
            file_cache_quota_per_device: value.file_cache_quota_per_device,
            file_retention_hours: value.file_retention_hours,
            file_auto_cleanup: value.file_auto_cleanup,
        }
    }
}

impl From<core::NetworkSettings> for NetworkSettingsDto {
    fn from(value: core::NetworkSettings) -> Self {
        Self {
            allow_relay_fallback: value.allow_relay_fallback,
        }
    }
}

// =========================
// From<Dto> for core model (for merge_settings_patch)
// =========================

impl From<ThemeDto> for core::Theme {
    fn from(value: ThemeDto) -> Self {
        match value {
            ThemeDto::Light => Self::Light,
            ThemeDto::Dark => Self::Dark,
            ThemeDto::System => Self::System,
        }
    }
}

impl From<UpdateChannelDto> for core::UpdateChannel {
    fn from(value: UpdateChannelDto) -> Self {
        match value {
            UpdateChannelDto::Stable => Self::Stable,
            UpdateChannelDto::Alpha => Self::Alpha,
            UpdateChannelDto::Beta => Self::Beta,
            UpdateChannelDto::Rc => Self::Rc,
        }
    }
}

impl From<ShortcutKeyDto> for core::ShortcutKey {
    fn from(value: ShortcutKeyDto) -> Self {
        match value {
            ShortcutKeyDto::Single(v) => Self::Single(v),
            ShortcutKeyDto::Multiple(v) => Self::Multiple(v),
        }
    }
}

impl From<ContentTypesDto> for core::ContentTypes {
    fn from(value: ContentTypesDto) -> Self {
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

impl From<SyncFrequencyDto> for core::SyncFrequency {
    fn from(value: SyncFrequencyDto) -> Self {
        match value {
            SyncFrequencyDto::Realtime => Self::Realtime,
            SyncFrequencyDto::Interval => Self::Interval,
        }
    }
}

impl From<RuleEvaluationDto> for core::RuleEvaluation {
    fn from(value: RuleEvaluationDto) -> Self {
        match value {
            RuleEvaluationDto::AnyMatch => Self::AnyMatch,
            RuleEvaluationDto::AllMatch => Self::AllMatch,
        }
    }
}

impl From<core::Settings> for SettingsDto {
    fn from(value: core::Settings) -> Self {
        Self {
            schema_version: value.schema_version,
            general: value.general.into(),
            sync: value.sync.into(),
            retention_policy: value.retention_policy.into(),
            security: value.security.into(),
            pairing: value.pairing.into(),
            keyboard_shortcuts: value
                .keyboard_shortcuts
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
            file_sync: value.file_sync.into(),
            network: value.network.into(),
        }
    }
}

#[cfg(test)]
mod network_dto_tests {
    use super::*;

    #[test]
    fn dto_serializes_camel_case_wire() {
        let dto = NetworkSettingsDto {
            allow_relay_fallback: true,
        };
        let json = serde_json::to_string(&dto).expect("serialize");
        assert_eq!(json, r#"{"allowRelayFallback":true}"#);
    }

    #[test]
    fn dto_deserializes_camel_case_wire() {
        let json = r#"{"allowRelayFallback":false}"#;
        let dto: NetworkSettingsDto = serde_json::from_str(json).expect("deserialize");
        assert!(!dto.allow_relay_fallback);
    }

    #[test]
    fn from_core_passes_through_business_semantics() {
        let core_value = core::NetworkSettings {
            allow_relay_fallback: false,
        };
        let dto: NetworkSettingsDto = core_value.into();
        assert!(
            !dto.allow_relay_fallback,
            "DTO MUST NOT invert semantics (Pitfall 1)"
        );
    }

    #[test]
    fn settings_dto_default_includes_network() {
        let core_settings = core::Settings::default();
        let dto: SettingsDto = core_settings.into();
        assert!(
            dto.network.allow_relay_fallback,
            "Settings::default network MUST be true"
        );
    }

    #[test]
    fn update_settings_response_serializes_restart_required_camel_case() {
        let resp = UpdateSettingsResponse {
            success: true,
            data: SettingsDto::from(core::Settings::default()),
            ts: 0,
            restart_required: true,
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(
            json.contains(r#""restartRequired":true"#),
            "wire field MUST be camelCase: got {json}"
        );
    }

    #[test]
    fn patch_dto_with_null_field_means_none() {
        let json = r#"{"allowRelayFallback":null}"#;
        let dto: NetworkSettingsPatchDto = serde_json::from_str(json).expect("deserialize");
        assert!(dto.allow_relay_fallback.is_none());
    }

    #[test]
    fn patch_dto_with_explicit_false() {
        let json = r#"{"allowRelayFallback":false}"#;
        let dto: NetworkSettingsPatchDto = serde_json::from_str(json).expect("deserialize");
        assert_eq!(dto.allow_relay_fallback, Some(false));
    }

    /// checker BLOCKER 5：`SettingsPatchDto::default()` 全字段 None，
    /// 让下游 plan 04 测试用 `..Default::default()` 简化 baseline 构造。
    #[test]
    fn settings_patch_dto_default_is_all_none() {
        let dto = SettingsPatchDto::default();
        assert!(dto.general.is_none());
        assert!(dto.sync.is_none());
        assert!(dto.retention_policy.is_none());
        assert!(dto.security.is_none());
        assert!(dto.pairing.is_none());
        assert!(dto.keyboard_shortcuts.is_none());
        assert!(dto.file_sync.is_none());
        assert!(dto.network.is_none());

        let net_patch = NetworkSettingsPatchDto::default();
        assert!(net_patch.allow_relay_fallback.is_none());
    }

    /// checker WARNING 3：向后兼容硬断言 —— PUT body `{}` 反序列化所有
    /// 顶层字段全 None；与 Phase 94 之前一致，没有引入新强制字段。
    #[test]
    fn settings_patch_dto_deserializes_empty_object_to_all_none() {
        let json = r#"{}"#;
        let dto: SettingsPatchDto = serde_json::from_str(json).expect("deserialize empty body");
        assert!(dto.general.is_none());
        assert!(dto.network.is_none());
        assert!(dto.file_sync.is_none());
    }
}
