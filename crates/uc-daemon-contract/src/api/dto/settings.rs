use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DurationSeconds};
use utoipa::ToSchema;

use uc_core::settings::model as core;

// NOTE (ADR-008 §0.1): both bespoke `{data,ts}` wrappers for the settings
// endpoints have been deleted. `GET /settings` returns the pure generic
// `ApiEnvelope<SettingsDto>` (alias `SettingsEnvelope`) and `PUT /settings`
// returns `ApiEnvelope<SettingsUpdateResultDto>` (alias
// `SettingsUpdateResultEnvelope`) with `success` + `restartRequired` folded into
// the payload below. No legacy `UpdateSettingsResponse` wrapper remains.

/// Folded payload for `PUT /settings` (ADR-008 §0.1).
///
/// The current handler returns `success` and `restartRequired` as top-level
/// siblings of the `{data,ts}` envelope. This DTO folds those siblings INTO the
/// payload so the endpoint can return `ApiEnvelope<SettingsUpdateResultDto>`
/// with no bespoke wrapper. P1 only defines the type; the handler is rewired in
/// P2.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdateResultDto {
    pub success: bool,
    /// Whether the patch touched fields requiring a daemon restart (currently
    /// only `network.*`).
    pub restart_required: bool,
}

/// Request body for `POST /settings/relay-probe`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelayProbeRequestDto {
    /// Candidate relay URL to probe. Not persisted; the probe is repeatable.
    pub url: String,
}

/// Outcome of a relay reachability probe (`POST /settings/relay-probe`).
///
/// Mirrors the desktop `RelayProbeOutcome` Tauri DTO: a probe that fails to
/// reach the relay is a NORMAL categorized outcome (returned 200), not an HTTP
/// error — the daemon is healthy, the *relay* is the subject under test. Only a
/// missing relay-diagnostic adapter (server misconfiguration) surfaces as an
/// `ApiError`. The frontend selects user-facing copy off the `tag`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "tag", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RelayProbeOutcomeDto {
    /// Relay reachable; carries end-to-end round-trip latency.
    Success { latency_ms: u32 },
    /// The supplied URL is not a valid relay URL.
    InvalidUrl { message: String },
    /// DNS resolution of the relay host failed.
    Dns { message: String },
    /// TLS handshake with the relay failed.
    Tls { message: String },
    /// Relay-protocol handshake failed after TLS.
    Handshake { message: String },
    /// The probe exceeded its time budget.
    Timeout,
    /// Any other categorized probe failure.
    Other { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettingsDto {
    pub auto_start: bool,
    pub silent_start: bool,
    pub auto_check_update: bool,
    /// Whether to download the next available update in the background.
    /// Persisted alongside `auto_check_update`; consumed by the frontend's
    /// `UpdateContext` after a successful `check_for_update` to decide
    /// whether to start a silent download.
    #[serde(default)]
    pub auto_download_update: bool,
    pub theme: ThemeDto,
    /// 旧版"统一主题预设"字段（v0.7 之前唯一字段）。新前端不再写入,
    /// 但 wire 仍透传以便老 daemon ↔ 新前端 / 新 daemon ↔ 老前端兼容。
    /// 删除计划见 `uc_core::settings::model::GeneralSettings::theme_color`。
    #[serde(default)]
    pub theme_color: Option<String>,
    /// Light 模式下的主题预设名（如 `"zinc"`）；为 `None` 时 daemon 端
    /// 将回退到 `theme_color`。wire 字段名 `themeColorLight`（camelCase）。
    #[serde(default)]
    pub theme_color_light: Option<String>,
    /// Dark 模式下的主题预设名（如 `"zinc"`）；为 `None` 时 daemon 端
    /// 将回退到 `theme_color`。wire 字段名 `themeColorDark`（camelCase）。
    #[serde(default)]
    pub theme_color_dark: Option<String>,
    /// Light 模式下用户对预设 token 的自定义覆盖（`{ tokenName: oklchString }`）。
    /// 为空 map 表示完全跟随 preset。wire 字段名 `themeOverridesLight`。
    #[serde(default)]
    pub theme_overrides_light: std::collections::BTreeMap<String, String>,
    /// Dark 模式下用户对预设 token 的自定义覆盖（语义同 light）。wire 字段名 `themeOverridesDark`。
    #[serde(default)]
    pub theme_overrides_dark: std::collections::BTreeMap<String, String>,
    pub language: Option<String>,
    pub device_name: Option<String>,
    /// Update channel preference. `None` means auto-detect from version string;
    /// `Some(channel)` means the user has overridden the channel.
    #[serde(default)]
    pub update_channel: Option<UpdateChannelDto>,
    /// Whether anonymous diagnostic telemetry is enabled.
    pub telemetry_enabled: bool,
    /// Whether anonymous product usage analytics is enabled.
    /// 与 `telemetry_enabled` 拆开（schema doc §6.4）：前者控制 Sentry 错误
    /// 上报，本字段控制产品 telemetry（漏斗 / 留存 / 同步可靠性事件）。
    #[serde(default = "default_true")]
    pub usage_analytics_enabled: bool,
    /// Persistent local diagnostic logging mode. Takes effect after restart.
    #[serde(default)]
    pub debug_mode: bool,
}

fn default_true() -> bool {
    true
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
    pub sync_on_restore: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SyncFrequencyDto {
    Realtime,
    Interval,
}

// `rename_all = "camelCase"` 只 rename 变体名（`ByAge` → `byAge`），不会改写
// struct 变体内部的字段名。必须同时加 `rename_all_fields = "camelCase"`，
// 否则 wire 是 `{"byAge":{"max_age":N}}`，与前端 `{ byAge: { maxAge: N } }`
// 错位，导致 PUT /settings 反序列化失败返回 422（见 issue #606）。
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
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

/// Algorithm for network flow control. Wire form: `"cubic"` | `"bbr3"`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum CongestionControllerDto {
    /// Loss-based; excellent LAN throughput.
    #[default]
    Cubic,
    /// Bandwidth-probing; better for lossy/high-latency links.
    Bbr3,
}

/// LAN-only Mode（v0.7.0）DTO 镜像。
///
/// 反向命名规则（Pitfall 1）：业务正向语义 `allow_relay_fallback`，
/// 不在此层重命名为 `lan_only` 或类似镜像。wire 字段 = `allowRelayFallback`
/// （camelCase 自动转换）。取反唯一发生在 `uc-bootstrap/src/network_policy.rs`。
///
/// `allow_overlay_network_addrs` 控制是否把 VPN/overlay 类虚拟网卡 IP（CGNAT
/// 100.64.0.0/10、Tailscale ULA fd7a:115c:a1e0::/48）作为 iroh 直连候选发布
/// 给对端。默认 `false`（过滤）。专业用户在两端都接入同一 VPN 时可开启。
///
/// `custom_relay_urls` 为空时继续使用 iroh 默认 relay；非空时只使用这些
/// 用户配置的 relay URL。LAN-only 模式关闭 relay 时该列表保留但不生效。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSettingsDto {
    pub allow_relay_fallback: bool,
    #[serde(default)]
    pub allow_overlay_network_addrs: bool,
    #[serde(default)]
    pub custom_relay_urls: Vec<String>,
    #[serde(default)]
    pub congestion_controller: CongestionControllerDto,
}

/// 快捷面板出现位置 DTO。wire form: `center` | `follow_cursor`。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuickPanelPositionDto {
    Center,
    FollowCursor,
}

/// 快捷面板（Spotlight 风格）功能偏好 DTO。
///
/// wire 字段命名为 camelCase（`enabled` / `position`）。`#[serde(default)]`
/// 让缺字段时回退到 `Default`（`enabled = true`、`position = center`），与
/// `core::QuickPanelSettings` 默认保持一致——新装/老 wire 缺字段都视为
/// "启用 + 居中"，避免出现 wire 与磁盘真相撕裂。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
#[serde(default, rename_all = "camelCase")]
pub struct QuickPanelSettingsDto {
    pub enabled: bool,
    pub position: QuickPanelPositionDto,
}

impl Default for QuickPanelSettingsDto {
    fn default() -> Self {
        Self {
            enabled: true,
            position: QuickPanelPositionDto::Center,
        }
    }
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
    #[serde(default)]
    pub quick_panel: QuickPanelSettingsDto,
}

// =========================
// Patch DTOs
// =========================

/// All fields are optional — only provided fields are updated.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettingsPatchDto {
    pub auto_start: Option<bool>,
    pub silent_start: Option<bool>,
    pub auto_check_update: Option<bool>,
    pub auto_download_update: Option<bool>,
    pub theme: Option<ThemeDto>,
    /// 旧版"统一主题预设"patch 字段。`Some(None)` = 显式清空,`None` = 不修改。
    #[serde(default)]
    pub theme_color: Option<Option<String>>,
    /// Light 模式预设 patch。`Some(None)` = 显式清空（回退到 `theme_color` 或引擎默认）。
    #[serde(default)]
    pub theme_color_light: Option<Option<String>>,
    /// Dark 模式预设 patch。`Some(None)` = 显式清空（回退到 `theme_color` 或引擎默认）。
    #[serde(default)]
    pub theme_color_dark: Option<Option<String>>,
    /// Light 模式 overrides patch。`Some(map)` 整体替换；`None` 表示不修改。
    #[serde(default)]
    pub theme_overrides_light: Option<std::collections::BTreeMap<String, String>>,
    /// Dark 模式 overrides patch（语义同 light）。
    #[serde(default)]
    pub theme_overrides_dark: Option<std::collections::BTreeMap<String, String>>,
    pub language: Option<Option<String>>,
    pub device_name: Option<Option<String>>,
    pub update_channel: Option<Option<UpdateChannelDto>>,
    pub telemetry_enabled: Option<bool>,
    pub usage_analytics_enabled: Option<bool>,
    pub debug_mode: Option<bool>,
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
    pub sync_on_restore: Option<bool>,
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
    pub allow_overlay_network_addrs: Option<bool>,
    pub custom_relay_urls: Option<Vec<String>>,
    pub congestion_controller: Option<CongestionControllerDto>,
}

/// 快捷面板字段 patch DTO 镜像 — `null` = 不修改。
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuickPanelSettingsPatchDto {
    pub enabled: Option<bool>,
    pub position: Option<QuickPanelPositionDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct KeyboardShortcutsPatchDto {
    // `null` map values mean "clear this shortcut" at runtime, but utoipa v4
    // cannot express a nullable `additionalProperties` ($ref + nullable becomes
    // an `allOf`, which utoipa 4.x refuses to place under additionalProperties).
    // The wire schema therefore advertises the non-nullable value type; the
    // nullability is documented behaviorally and enforced by serde, not by the
    // OpenAPI schema.
    #[schema(value_type = std::collections::HashMap<String, ShortcutKeyDto>)]
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
    pub quick_panel: Option<QuickPanelSettingsPatchDto>,
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
            auto_download_update: value.auto_download_update,
            theme: value.theme.into(),
            theme_color: value.theme_color,
            theme_color_light: value.theme_color_light,
            theme_color_dark: value.theme_color_dark,
            theme_overrides_light: value.theme_overrides_light,
            theme_overrides_dark: value.theme_overrides_dark,
            language: value.language,
            device_name: value.device_name,
            update_channel: value.update_channel.map(Into::into),
            telemetry_enabled: value.telemetry_enabled,
            usage_analytics_enabled: value.usage_analytics_enabled,
            debug_mode: value.debug_mode,
        }
    }
}

impl From<core::SyncSettings> for SyncSettingsDto {
    fn from(value: core::SyncSettings) -> Self {
        Self {
            auto_sync: value.auto_sync,
            sync_frequency: value.sync_frequency.into(),
            content_types: value.content_types.into(),
            sync_on_restore: value.sync_on_restore,
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
            allow_overlay_network_addrs: value.allow_overlay_network_addrs,
            custom_relay_urls: value.custom_relay_urls,
            congestion_controller: value.congestion_controller.into(),
        }
    }
}

impl From<core::CongestionController> for CongestionControllerDto {
    fn from(value: core::CongestionController) -> Self {
        match value {
            core::CongestionController::Cubic => Self::Cubic,
            core::CongestionController::Bbr3 => Self::Bbr3,
        }
    }
}

impl From<CongestionControllerDto> for core::CongestionController {
    fn from(value: CongestionControllerDto) -> Self {
        match value {
            CongestionControllerDto::Cubic => Self::Cubic,
            CongestionControllerDto::Bbr3 => Self::Bbr3,
        }
    }
}

impl From<core::QuickPanelPosition> for QuickPanelPositionDto {
    fn from(value: core::QuickPanelPosition) -> Self {
        match value {
            core::QuickPanelPosition::Center => Self::Center,
            core::QuickPanelPosition::FollowCursor => Self::FollowCursor,
        }
    }
}

impl From<core::QuickPanelSettings> for QuickPanelSettingsDto {
    fn from(value: core::QuickPanelSettings) -> Self {
        Self {
            enabled: value.enabled,
            position: value.position.into(),
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

impl From<QuickPanelPositionDto> for core::QuickPanelPosition {
    fn from(value: QuickPanelPositionDto) -> Self {
        match value {
            QuickPanelPositionDto::Center => Self::Center,
            QuickPanelPositionDto::FollowCursor => Self::FollowCursor,
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
            quick_panel: value.quick_panel.into(),
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
            allow_overlay_network_addrs: false,
            custom_relay_urls: Vec::new(),
            congestion_controller: CongestionControllerDto::default(),
        };
        let json = serde_json::to_string(&dto).expect("serialize");
        assert_eq!(
            json,
            r#"{"allowRelayFallback":true,"allowOverlayNetworkAddrs":false,"customRelayUrls":[],"congestionController":"cubic"}"#
        );
    }

    #[test]
    fn dto_deserializes_camel_case_wire() {
        let json = r#"{"allowRelayFallback":false,"allowOverlayNetworkAddrs":true,"customRelayUrls":["https://relay.example.com."]}"#;
        let dto: NetworkSettingsDto = serde_json::from_str(json).expect("deserialize");
        assert!(!dto.allow_relay_fallback);
        assert!(dto.allow_overlay_network_addrs);
        assert_eq!(
            dto.custom_relay_urls,
            vec!["https://relay.example.com.".to_string()]
        );
    }

    /// 旧 wire（无 allowOverlayNetworkAddrs/customRelayUrls 字段）仍可反序列化。
    #[test]
    fn dto_deserializes_legacy_wire_without_overlay_field() {
        let json = r#"{"allowRelayFallback":true}"#;
        let dto: NetworkSettingsDto = serde_json::from_str(json).expect("deserialize legacy");
        assert!(dto.allow_relay_fallback);
        assert!(!dto.allow_overlay_network_addrs);
        assert!(dto.custom_relay_urls.is_empty());
    }

    #[test]
    fn from_core_passes_through_business_semantics() {
        let core_value = core::NetworkSettings {
            allow_relay_fallback: false,
            allow_overlay_network_addrs: true,
            custom_relay_urls: vec!["https://relay.example.com.".to_string()],
            congestion_controller: core::CongestionController::Bbr3,
        };
        let dto: NetworkSettingsDto = core_value.into();
        assert!(
            !dto.allow_relay_fallback,
            "DTO MUST NOT invert semantics (Pitfall 1)"
        );
        assert!(dto.allow_overlay_network_addrs);
        assert_eq!(
            dto.custom_relay_urls,
            vec!["https://relay.example.com.".to_string()]
        );
        assert_eq!(dto.congestion_controller, CongestionControllerDto::Bbr3);
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
    fn settings_update_result_serializes_restart_required_camel_case() {
        // ADR-008 §0.1: PUT /settings returns `ApiEnvelope<SettingsUpdateResultDto>`
        // = `{ data: { success, restartRequired }, ts }`. `restartRequired` must
        // still serialize camelCase inside the folded payload.
        let resp = crate::api::dto::envelope::ApiEnvelope::with_ts(
            SettingsUpdateResultDto {
                success: true,
                restart_required: true,
            },
            0,
        );
        let json = serde_json::to_string(&resp).expect("serialize");
        assert!(
            json.contains(r#""restartRequired":true"#),
            "wire field MUST be camelCase: got {json}"
        );
        assert!(
            json.contains(r#""success":true"#),
            "success must be folded into the envelope data: got {json}"
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

    #[test]
    fn patch_dto_with_custom_relay_urls() {
        let json = r#"{"customRelayUrls":["https://relay.example.com."]}"#;
        let dto: NetworkSettingsPatchDto = serde_json::from_str(json).expect("deserialize");
        assert_eq!(
            dto.custom_relay_urls,
            Some(vec!["https://relay.example.com.".to_string()])
        );
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
        assert!(dto.quick_panel.is_none());

        let net_patch = NetworkSettingsPatchDto::default();
        assert!(net_patch.allow_relay_fallback.is_none());
        assert!(net_patch.custom_relay_urls.is_none());

        let quick_patch = QuickPanelSettingsPatchDto::default();
        assert!(quick_patch.enabled.is_none());
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

    /// 老 wire 只带 themeColor 字段时,新 DTO 反序列化后两个新字段为 None,
    /// 不要回写任何"猜测值",回退由 daemon 端 effective_theme_color_* 处理。
    #[test]
    fn general_dto_legacy_theme_color_only_keeps_split_fields_none() {
        let json = r#"{
            "autoStart": false,
            "silentStart": false,
            "autoCheckUpdate": true,
            "theme": "system",
            "themeColor": "catppuccin",
            "language": null,
            "deviceName": null,
            "telemetryEnabled": true
        }"#;
        let dto: GeneralSettingsDto = serde_json::from_str(json).expect("deserialize legacy wire");
        assert_eq!(dto.theme_color.as_deref(), Some("catppuccin"));
        assert!(dto.theme_color_light.is_none());
        assert!(dto.theme_color_dark.is_none());
    }

    /// 新 wire 带 themeColorLight / themeColorDark 时,字段透传不变。
    #[test]
    fn general_dto_new_wire_round_trips_split_fields() {
        let json = r#"{
            "autoStart": false,
            "silentStart": false,
            "autoCheckUpdate": true,
            "theme": "system",
            "themeColor": null,
            "themeColorLight": "zinc",
            "themeColorDark": "claude",
            "language": null,
            "deviceName": null,
            "telemetryEnabled": true
        }"#;
        let dto: GeneralSettingsDto = serde_json::from_str(json).expect("deserialize new wire");
        assert_eq!(dto.theme_color_light.as_deref(), Some("zinc"));
        assert_eq!(dto.theme_color_dark.as_deref(), Some("claude"));
        // 序列化回 wire 仍是 camelCase 命名
        let json_out = serde_json::to_string(&dto).expect("serialize");
        assert!(json_out.contains(r#""themeColorLight":"zinc""#));
        assert!(json_out.contains(r#""themeColorDark":"claude""#));
    }

    /// patch DTO 新字段双向覆盖语义。
    #[test]
    fn general_patch_dto_split_fields_round_trip() {
        let json = r#"{ "themeColorLight": "zinc", "themeColorDark": "claude" }"#;
        let dto: GeneralSettingsPatchDto = serde_json::from_str(json).expect("deserialize patch");
        assert_eq!(dto.theme_color_light, Some(Some("zinc".to_string())));
        assert_eq!(dto.theme_color_dark, Some(Some("claude".to_string())));
    }

    /// patch DTO 缺字段时所有 split 字段都是 `None`(不修改)。
    ///
    /// 备注:wire 上的 JSON `null` 在默认 serde 下也会被解析为外层 `None`,
    /// 因此前端无法通过 wire 传 `Some(None)`("显式清空")语义;清空只能由
    /// daemon 内部 patch 调用产出。这是历史 `theme_color` 字段的既有约束,
    /// 拆分后的两个字段保持一致行为。
    #[test]
    fn general_patch_dto_missing_fields_means_no_change() {
        let json = r#"{}"#;
        let dto: GeneralSettingsPatchDto =
            serde_json::from_str(json).expect("deserialize empty patch");
        assert!(dto.theme_color.is_none());
        assert!(dto.theme_color_light.is_none());
        assert!(dto.theme_color_dark.is_none());
        assert!(dto.theme_overrides_light.is_none());
        assert!(dto.theme_overrides_dark.is_none());
    }

    /// 老 wire 不带 themeOverrides* 字段时 DTO 反序列化默认空 map。
    #[test]
    fn general_dto_legacy_wire_without_overrides_defaults_empty_map() {
        let json = r#"{
            "autoStart": false,
            "silentStart": false,
            "autoCheckUpdate": true,
            "theme": "system",
            "themeColor": null,
            "language": null,
            "deviceName": null,
            "telemetryEnabled": true
        }"#;
        let dto: GeneralSettingsDto = serde_json::from_str(json).expect("deserialize legacy wire");
        assert!(dto.theme_overrides_light.is_empty());
        assert!(dto.theme_overrides_dark.is_empty());
    }

    /// 新 wire 带 overrides 时 round-trip 正确。
    #[test]
    fn general_dto_overrides_round_trip_camel_case() {
        let json = r#"{
            "autoStart": false,
            "silentStart": false,
            "autoCheckUpdate": true,
            "theme": "system",
            "themeColor": null,
            "themeOverridesLight": { "primary": "oklch(0.5 0.2 270)" },
            "themeOverridesDark": { "background": "oklch(0.18 0.02 280)" },
            "language": null,
            "deviceName": null,
            "telemetryEnabled": true
        }"#;
        let dto: GeneralSettingsDto = serde_json::from_str(json).expect("deserialize new wire");
        assert_eq!(
            dto.theme_overrides_light.get("primary").map(String::as_str),
            Some("oklch(0.5 0.2 270)")
        );
        assert_eq!(
            dto.theme_overrides_dark
                .get("background")
                .map(String::as_str),
            Some("oklch(0.18 0.02 280)")
        );

        let out = serde_json::to_string(&dto).expect("serialize");
        assert!(out.contains(r#""themeOverridesLight":{"primary":"oklch(0.5 0.2 270)"}"#));
        assert!(out.contains(r#""themeOverridesDark":{"background":"oklch(0.18 0.02 280)"}"#));
    }

    /// patch DTO 显式带 overrides map 时 round-trip 正确,清空（空 map）也保留。
    #[test]
    fn general_patch_dto_overrides_round_trip() {
        let json = r#"{ "themeOverridesLight": { "primary": "oklch(0.5 0.2 270)" }, "themeOverridesDark": {} }"#;
        let dto: GeneralSettingsPatchDto = serde_json::from_str(json).expect("deserialize patch");
        let light = dto.theme_overrides_light.expect("light Some");
        assert_eq!(
            light.get("primary").map(String::as_str),
            Some("oklch(0.5 0.2 270)")
        );
        let dark = dto.theme_overrides_dark.expect("dark Some");
        assert!(dark.is_empty(), "explicit empty map preserved");
    }

    /// 老 wire 缺 `autoDownloadUpdate` 字段时 DTO 反序列化默认 false（opt-in）。
    /// 保证 v0.9 之前持久化的 settings.json 升级到带本字段的版本不会反序列化失败。
    #[test]
    fn general_dto_legacy_wire_without_auto_download_defaults_to_false() {
        let json = r#"{
            "autoStart": false,
            "silentStart": false,
            "autoCheckUpdate": true,
            "theme": "system",
            "themeColor": null,
            "language": null,
            "deviceName": null,
            "telemetryEnabled": true
        }"#;
        let dto: GeneralSettingsDto = serde_json::from_str(json).expect("deserialize legacy wire");
        assert!(
            !dto.auto_download_update,
            "missing autoDownloadUpdate must default to false (opt-in)"
        );
    }

    /// 新 wire 带 `autoDownloadUpdate` 字段时正确透传两个方向。
    #[test]
    fn general_dto_auto_download_round_trips_camel_case() {
        let json = r#"{
            "autoStart": false,
            "silentStart": false,
            "autoCheckUpdate": true,
            "autoDownloadUpdate": true,
            "theme": "system",
            "themeColor": null,
            "language": null,
            "deviceName": null,
            "telemetryEnabled": true
        }"#;
        let dto: GeneralSettingsDto =
            serde_json::from_str(json).expect("deserialize with autoDownloadUpdate");
        assert!(dto.auto_download_update);

        let out = serde_json::to_string(&dto).expect("serialize");
        assert!(
            out.contains(r#""autoDownloadUpdate":true"#),
            "wire field MUST be camelCase: {}",
            out
        );
    }

    /// patch DTO 缺字段不修改、显式带字段才修改 —— `autoDownloadUpdate` 同其他 bool patch。
    #[test]
    fn general_patch_dto_auto_download_optional_semantics() {
        let absent: GeneralSettingsPatchDto =
            serde_json::from_str("{}").expect("deserialize empty patch");
        assert!(
            absent.auto_download_update.is_none(),
            "missing => no change"
        );

        let explicit_false: GeneralSettingsPatchDto =
            serde_json::from_str(r#"{"autoDownloadUpdate": false}"#).expect("deserialize patch");
        assert_eq!(explicit_false.auto_download_update, Some(false));

        let explicit_true: GeneralSettingsPatchDto =
            serde_json::from_str(r#"{"autoDownloadUpdate": true}"#).expect("deserialize patch");
        assert_eq!(explicit_true.auto_download_update, Some(true));
    }
}

/// issue #606 回归守卫 —— `RetentionRuleDto` wire 形态锁定。
///
/// 历史背景：旧实现只在枚举上声明 `rename_all = "camelCase"`，serde 只 rename
/// 变体名（`ByAge` → `byAge`），不会改写 struct 变体内部字段名。结果 wire 是
/// `{"byAge":{"max_age":N}}`、前端发 `{"byAge":{"maxAge":N}}`，PUT /settings
/// 反序列化失败返回 422。修复加了 `rename_all_fields = "camelCase"`。
///
/// 下面五条用例锁住每个变体的 wire 形态，并显式 reject 旧 bug-shape；
/// 任何方向回退（删 `rename_all_fields`、把字段改 snake_case 等）都会被抓住。
#[cfg(test)]
mod retention_rule_dto_tests {
    use super::*;

    #[test]
    fn by_age_wire_uses_camel_case_field() {
        let rule = RetentionRuleDto::ByAge {
            max_age: Duration::from_secs(2_592_000),
        };
        let wire = serde_json::to_string(&rule).expect("serialize ByAge");
        assert_eq!(
            wire, r#"{"byAge":{"maxAge":2592000}}"#,
            "wire MUST be camelCase inside variant (issue #606)"
        );

        let parsed: RetentionRuleDto =
            serde_json::from_str(r#"{"byAge":{"maxAge":86400}}"#).expect("accept camelCase wire");
        match parsed {
            RetentionRuleDto::ByAge { max_age } => assert_eq!(max_age.as_secs(), 86400),
            _ => panic!("unexpected variant"),
        }

        // 关键负面用例：旧 bug-shape 必须被拒绝，避免回退悄无声息地通过。
        assert!(
            serde_json::from_str::<RetentionRuleDto>(r#"{"byAge":{"max_age":86400}}"#).is_err(),
            "snake_case field on wire MUST be rejected — that's the issue #606 bug shape"
        );
    }

    #[test]
    fn by_count_wire_uses_camel_case_field() {
        let rule = RetentionRuleDto::ByCount { max_items: 500 };
        assert_eq!(
            serde_json::to_string(&rule).unwrap(),
            r#"{"byCount":{"maxItems":500}}"#
        );

        let parsed: RetentionRuleDto =
            serde_json::from_str(r#"{"byCount":{"maxItems":1000}}"#).expect("accept camelCase");
        match parsed {
            RetentionRuleDto::ByCount { max_items } => assert_eq!(max_items, 1000),
            _ => panic!("unexpected variant"),
        }

        assert!(
            serde_json::from_str::<RetentionRuleDto>(r#"{"byCount":{"max_items":1}}"#).is_err(),
            "snake_case field must be rejected"
        );
    }

    #[test]
    fn by_content_type_wire_uses_camel_case_field() {
        let rule = RetentionRuleDto::ByContentType {
            content_type: ContentTypesDto {
                text: true,
                image: false,
                link: false,
                file: false,
                code_snippet: false,
                rich_text: false,
            },
            max_age: Duration::from_secs(86_400),
        };
        let wire = serde_json::to_value(&rule).expect("serialize ByContentType");
        let expected = serde_json::json!({
            "byContentType": {
                "contentType": {
                    "text": true,
                    "image": false,
                    "link": false,
                    "file": false,
                    "codeSnippet": false,
                    "richText": false,
                },
                "maxAge": 86_400,
            }
        });
        assert_eq!(wire, expected);
    }

    #[test]
    fn by_total_size_and_sensitive_wire_camel_case() {
        let by_size = RetentionRuleDto::ByTotalSize {
            max_bytes: 1_073_741_824,
        };
        assert_eq!(
            serde_json::to_string(&by_size).unwrap(),
            r#"{"byTotalSize":{"maxBytes":1073741824}}"#
        );

        let sensitive = RetentionRuleDto::Sensitive {
            max_age: Duration::from_secs(3600),
        };
        assert_eq!(
            serde_json::to_string(&sensitive).unwrap(),
            r#"{"sensitive":{"maxAge":3600}}"#
        );
    }

    /// 端到端：把前端 `StorageSection.setByAgeRule / setByCountRule` 拼出的
    /// patch body 用 `SettingsPatchDto` 反序列化。修复前这一步在 axum
    /// `Json<SettingsPatchDto>` 提取器内部抛 `missing field "max_age"`，
    /// 返回 422（issue #606 用户实际碰到的现象）。
    #[test]
    fn settings_patch_dto_accepts_frontend_retention_rules_payload() {
        let body = r#"{
            "retentionPolicy": {
                "enabled": true,
                "rules": [
                    {"byAge": {"maxAge": 5184000}},
                    {"byCount": {"maxItems": 1000}}
                ],
                "skipPinned": true,
                "evaluation": "anyMatch"
            }
        }"#;

        let patch: SettingsPatchDto =
            serde_json::from_str(body).expect("PUT body must deserialize (issue #606)");
        let retention = patch.retention_policy.expect("retentionPolicy present");
        let rules = retention.rules.expect("rules present");
        assert_eq!(rules.len(), 2);
        match &rules[0] {
            RetentionRuleDto::ByAge { max_age } => assert_eq!(max_age.as_secs(), 5_184_000),
            other => panic!("unexpected first rule: {other:?}"),
        }
        match &rules[1] {
            RetentionRuleDto::ByCount { max_items } => assert_eq!(*max_items, 1000),
            other => panic!("unexpected second rule: {other:?}"),
        }
    }

    /// 防御回归：旧 bug-shape（变体内部字段是 snake_case）在 PUT body 层级
    /// 也必须被拒绝。
    #[test]
    fn settings_patch_dto_rejects_legacy_snake_case_inside_variant() {
        let buggy = r#"{
            "retentionPolicy": {
                "rules": [{"byAge": {"max_age": 86400}}]
            }
        }"#;
        assert!(
            serde_json::from_str::<SettingsPatchDto>(buggy).is_err(),
            "snake_case field inside variant MUST NOT deserialize — that's the issue #606 bug shape"
        );
    }
}

/// 子集 A —— enum 变体 wire 形态锁定。
///
/// 这些 enum 看起来人畜无害(unit-only 变体没 issue #606 那种"内嵌字段
/// 被忽略"的坑),但 wire 字面量是与前端 TS 字面量类型硬绑定的契约:
///   - `ThemeDto` ↔ `type Theme = 'light' | 'dark' | 'system'`
///   - `UpdateChannelDto` ↔ `type UpdateChannel = 'stable' | 'alpha' | 'beta' | 'rc'`
///   - `SyncFrequencyDto` ↔ `type SyncFrequency = 'realtime' | 'interval'`
///   - `RuleEvaluationDto` ↔ `type RuleEvaluation = 'anyMatch' | 'allMatch'`
///   - `ShortcutKeyDto` (untagged) ↔ `type ShortcutKey = string | string[]`
///
/// 任何 PR 误把 `rename_all` 改成另一种风格、或者把 `untagged` 摘掉,
/// 前端会瞬间不识别但后端编译过 —— 这些测试是兜底的契约 fence。
///
/// 特别留意 `RuleEvaluationDto` 是 `camelCase`(`anyMatch`),
/// 而 `uc-core::RuleEvaluation` 是 `snake_case`(`any_match`)。
/// wire 与持久化格式不同,转换在 webserver `rule_evaluation_from_dto` 完成。
#[cfg(test)]
mod enum_wire_tests {
    use super::*;

    #[test]
    fn theme_dto_wire_is_snake_case_lowercase() {
        assert_eq!(
            serde_json::to_string(&ThemeDto::Light).unwrap(),
            r#""light""#
        );
        assert_eq!(serde_json::to_string(&ThemeDto::Dark).unwrap(), r#""dark""#);
        assert_eq!(
            serde_json::to_string(&ThemeDto::System).unwrap(),
            r#""system""#
        );

        let parsed: ThemeDto = serde_json::from_str(r#""dark""#).expect("accept 'dark'");
        assert_eq!(parsed, ThemeDto::Dark);

        // 防御:误改成 PascalCase / camelCase 必须解析失败。
        assert!(serde_json::from_str::<ThemeDto>(r#""Light""#).is_err());
        assert!(serde_json::from_str::<ThemeDto>(r#""systemTheme""#).is_err());
    }

    #[test]
    fn update_channel_dto_wire_is_snake_case_lowercase() {
        for (variant, literal) in [
            (UpdateChannelDto::Stable, r#""stable""#),
            (UpdateChannelDto::Alpha, r#""alpha""#),
            (UpdateChannelDto::Beta, r#""beta""#),
            (UpdateChannelDto::Rc, r#""rc""#),
        ] {
            assert_eq!(serde_json::to_string(&variant).unwrap(), literal);
            assert_eq!(
                serde_json::from_str::<UpdateChannelDto>(literal).unwrap(),
                variant
            );
        }

        assert!(serde_json::from_str::<UpdateChannelDto>(r#""RC""#).is_err());
    }

    #[test]
    fn sync_frequency_dto_wire_is_snake_case_lowercase() {
        assert_eq!(
            serde_json::to_string(&SyncFrequencyDto::Realtime).unwrap(),
            r#""realtime""#
        );
        assert_eq!(
            serde_json::to_string(&SyncFrequencyDto::Interval).unwrap(),
            r#""interval""#
        );

        let parsed: SyncFrequencyDto =
            serde_json::from_str(r#""realtime""#).expect("accept 'realtime'");
        assert_eq!(parsed, SyncFrequencyDto::Realtime);

        // 防御:历史上"Realtime"或者"real_time"都不是合法 wire。
        assert!(serde_json::from_str::<SyncFrequencyDto>(r#""Realtime""#).is_err());
        assert!(serde_json::from_str::<SyncFrequencyDto>(r#""real_time""#).is_err());
    }

    /// 关键 fence:`RuleEvaluationDto` 用 camelCase,与 `uc-core` 的 snake_case
    /// 形态不同。前端 TS 字面量是 `'anyMatch' | 'allMatch'`。
    #[test]
    fn rule_evaluation_dto_wire_is_camel_case() {
        assert_eq!(
            serde_json::to_string(&RuleEvaluationDto::AnyMatch).unwrap(),
            r#""anyMatch""#
        );
        assert_eq!(
            serde_json::to_string(&RuleEvaluationDto::AllMatch).unwrap(),
            r#""allMatch""#
        );

        let parsed: RuleEvaluationDto =
            serde_json::from_str(r#""anyMatch""#).expect("accept 'anyMatch'");
        assert_eq!(parsed, RuleEvaluationDto::AnyMatch);

        // 防御:把 wire 形态改回与 uc-core 同样的 snake_case 会让前端瞬间挂掉。
        assert!(
            serde_json::from_str::<RuleEvaluationDto>(r#""any_match""#).is_err(),
            "RuleEvaluationDto wire MUST be camelCase, not snake_case (前端契约)"
        );
    }

    /// `ShortcutKeyDto` 是 `untagged`,wire 直接是 string 或 string[]。
    /// 如果误改成 `tag = "kind"` 类的 internally-tagged,前端的
    /// `Record<string, string | string[]>` 会立刻失配。
    #[test]
    fn shortcut_key_dto_is_untagged_string_or_array() {
        let single = ShortcutKeyDto::Single("Ctrl+C".into());
        assert_eq!(serde_json::to_string(&single).unwrap(), r#""Ctrl+C""#);

        let multi = ShortcutKeyDto::Multiple(vec!["Ctrl+C".into(), "Meta+C".into()]);
        assert_eq!(
            serde_json::to_string(&multi).unwrap(),
            r#"["Ctrl+C","Meta+C"]"#
        );

        // 反向:两种 wire 都能解析回正确变体。
        let parsed_single: ShortcutKeyDto =
            serde_json::from_str(r#""Ctrl+V""#).expect("accept bare string");
        assert!(matches!(parsed_single, ShortcutKeyDto::Single(s) if s == "Ctrl+V"));

        let parsed_multi: ShortcutKeyDto =
            serde_json::from_str(r#"["a","b"]"#).expect("accept array");
        assert!(matches!(parsed_multi, ShortcutKeyDto::Multiple(v) if v.len() == 2));
    }
}

/// 子集 B —— `GeneralSettingsPatchDto` 的 `Option<Option<T>>` 字段当前 wire 语义锁定。
///
/// `theme_color / language / device_name / update_channel` 的字段类型是
/// `Option<Option<T>>`,facade 层(`models.rs` line 485+)按三态语义消费:
/// ```ignore
/// if let Some(v) = general.theme_color {
///     existing.general.theme_color = v;  // Some(None) ⇒ 清空, Some(Some(x)) ⇒ 设置
/// }
/// ```
///
/// 但 **wire 层目前是 2-state**:缺字段和 `null` 都被裸 serde 反序列化成
/// `None`(外层 Option),只有非 null 值才走到 `Some(...)` 分支。
/// 这意味着前端无法用 `null` 显式清空一个 `Option<String>` 字段 ——
/// 这是一个**已知 wire/facade 契约不齐**(类似 issue #606 那种)。
///
/// 修法是给这些字段加 `#[serde(with = "serde_with::rust::double_option")]`
/// 让 `null` ⇒ `Some(None)`、缺字段 ⇒ `None`。但那是行为变更,需要单独 PR
/// 评估前端是否依赖现行 collapsed 行为,故本 PR 只锁定**当前现实**:
///   - 缺字段 ⇒ `None`         (不改)
///   - `null` ⇒ `None`         (⚠️ 与"清空"不可区分 —— 见 TODO)
///   - 值     ⇒ `Some(Some(v))` (设置新值)
///
/// 任一测试 fail = wire 行为漂移,需要回头判断是有意修复还是回归。
// TODO(#606-followup): 决定是否启用 `serde_with::rust::double_option` 让
// `null` ⇒ `Some(None)` 真正支持"显式清空",并把下面 explicit_null 用例
// 的预期从 `None` 改成 `Some(None)`。
#[cfg(test)]
mod general_patch_optional_field_wire_tests {
    use super::*;

    #[test]
    fn missing_field_means_none() {
        let body = r#"{}"#;
        let dto: GeneralSettingsPatchDto = serde_json::from_str(body).expect("deserialize");
        assert!(dto.theme_color.is_none(), "missing field ⇒ None (不改)");
        assert!(dto.language.is_none());
        assert!(dto.device_name.is_none());
        assert!(dto.update_channel.is_none());
    }

    /// ⚠️ 当前行为:wire `null` 反序列化成外层 `None`,与缺字段不可区分。
    /// 修复 TODO 在模块 docstring。
    #[test]
    fn explicit_null_collapses_to_none_today() {
        let body = r#"{
            "themeColor": null,
            "language": null,
            "deviceName": null,
            "updateChannel": null
        }"#;
        let dto: GeneralSettingsPatchDto = serde_json::from_str(body).expect("deserialize");
        assert_eq!(
            dto.theme_color, None,
            "today wire `null` collapses to outer None — see module TODO for 3-state fix"
        );
        assert_eq!(dto.language, None);
        assert_eq!(dto.device_name, None);
        assert_eq!(dto.update_channel, None);
    }

    #[test]
    fn explicit_value_becomes_some_some() {
        let body = r#"{
            "themeColor": "blue",
            "language": "zh-CN",
            "deviceName": "ws-1",
            "updateChannel": "beta"
        }"#;
        let dto: GeneralSettingsPatchDto = serde_json::from_str(body).expect("deserialize");
        assert_eq!(dto.theme_color, Some(Some("blue".to_string())));
        assert_eq!(dto.language, Some(Some("zh-CN".to_string())));
        assert_eq!(dto.device_name, Some(Some("ws-1".to_string())));
        assert_eq!(dto.update_channel, Some(Some(UpdateChannelDto::Beta)));
    }
}

/// 子集 C —— `Duration` wire 形态(`serde_with::DurationSeconds<u64>`)。
///
/// 所有 `PairingSettingsDto` 的 timeout 字段、`RetentionRuleDto::ByAge.max_age`
/// 都靠 `DurationSeconds<u64>` 把 `Duration` 序列化成秒数 `u64`。误改成
/// `DurationMilliSeconds` 或 `DurationSecondsWithFrac` 会让前端拿 `number`
/// 解析出错位的数量级 / 浮点格式。这里把 wire 形态固定下来。
#[cfg(test)]
mod duration_wire_tests {
    use super::*;

    #[test]
    fn pairing_durations_serialize_as_u64_seconds() {
        let dto = PairingSettingsDto {
            step_timeout: Duration::from_secs(30),
            user_verification_timeout: Duration::from_secs(120),
            session_timeout: Duration::from_secs(3600),
            max_retries: 3,
            protocol_version: "1.0".into(),
        };
        let value = serde_json::to_value(&dto).expect("serialize");
        assert_eq!(
            value["stepTimeout"],
            serde_json::json!(30),
            "Duration MUST serialize as integer seconds, not millis / float / object"
        );
        assert_eq!(value["userVerificationTimeout"], serde_json::json!(120));
        assert_eq!(value["sessionTimeout"], serde_json::json!(3600));

        // 反向:整数秒能正确解析。
        let body = r#"{
            "stepTimeout": 30,
            "userVerificationTimeout": 120,
            "sessionTimeout": 3600,
            "maxRetries": 3,
            "protocolVersion": "1.0"
        }"#;
        let parsed: PairingSettingsDto = serde_json::from_str(body).expect("deserialize");
        assert_eq!(parsed.step_timeout, Duration::from_secs(30));
        assert_eq!(parsed.user_verification_timeout, Duration::from_secs(120));
        assert_eq!(parsed.session_timeout, Duration::from_secs(3600));
    }

    #[test]
    fn pairing_patch_durations_round_trip_optional_seconds() {
        // 缺字段 → None (不改)。
        let empty: PairingSettingsPatchDto =
            serde_json::from_str(r#"{}"#).expect("deserialize empty");
        assert!(empty.step_timeout.is_none());

        // 整数秒 → Some(Duration)。
        let body = r#"{"stepTimeout": 60}"#;
        let parsed: PairingSettingsPatchDto = serde_json::from_str(body).expect("deserialize");
        assert_eq!(parsed.step_timeout, Some(Duration::from_secs(60)));

        // 防御:DurationSeconds<u64> 不接受浮点(serde_with 行为)。
        // 这里只断言"如果未来改成 WithFrac 则该测试需要更新",
        // 避免静默语义漂移。
        let frac = r#"{"stepTimeout": 1.5}"#;
        assert!(
            serde_json::from_str::<PairingSettingsPatchDto>(frac).is_err(),
            "DurationSeconds<u64> MUST reject fractional input — change guard for accidental switch to WithFrac"
        );
    }

    /// `RetentionRuleDto::ByAge.max_age` 同样用 `DurationSeconds<u64>`,
    /// 锁定一下整数秒 ↔ Duration 来回都对(防止 issue #606 修复时
    /// 误把单位改了)。
    #[test]
    fn retention_by_age_max_age_is_u64_seconds() {
        let rule = RetentionRuleDto::ByAge {
            max_age: Duration::from_secs(2_592_000),
        };
        let wire = serde_json::to_value(&rule).expect("serialize");
        assert_eq!(wire["byAge"]["maxAge"], serde_json::json!(2_592_000u64));
    }
}
