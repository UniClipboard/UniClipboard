use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DurationSeconds};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

// 所有 settings struct 统一使用 `#[serde(default)]`：缺字段时回退到
// `Default::default()`（在 `defaults.rs` 中实现），保证向后兼容。
// 详见 issue #581：旧版本 settings.json 缺新增字段会让 daemon 启动失败。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    pub auto_start: bool,
    pub silent_start: bool,
    pub auto_check_update: bool,
    /// Whether to download the next available update in the background.
    ///
    /// Pre-fetching the installer bytes lets the click-to-install flow
    /// skip the download step entirely. Independent of
    /// `auto_check_update`: the daemon only honours this flag when the
    /// frontend has actually checked for an update; the UI further
    /// gates the toggle on `auto_check_update` so users can't get into
    /// "download but never check" states.
    pub auto_download_update: bool,
    pub theme: Theme,
    /// 旧版"全局主题预设"字段（v0.7 之前唯一字段）。
    ///
    /// 现在仅作为 `theme_color_light` / `theme_color_dark` 都为 `None` 时的
    /// 回退值,目的是让 v0.7 前持久化的用户偏好（"我选过 catppuccin"）
    /// 在升级后第一次 light/dark 切换之前仍生效。
    ///
    /// # Removal plan
    /// 一旦新版 UI 写入过 `theme_color_light` / `theme_color_dark`,这个字段
    /// 不再被读取。计划在 v0.9 把字段彻底删除并 bump `schema_version`。
    /// 删除前请确认 release notes / changelog 已经向用户提示过迁移窗口。
    #[serde(default)]
    pub theme_color: Option<String>,
    /// Light 模式下使用的主题预设名（如 `"zinc"`、`"catppuccin"`）。
    /// 为 `None` 时回退到 `theme_color`,再为 `None` 时使用引擎默认。
    #[serde(default)]
    pub theme_color_light: Option<String>,
    /// Dark 模式下使用的主题预设名（如 `"zinc"`、`"catppuccin"`）。
    /// 为 `None` 时回退到 `theme_color`,再为 `None` 时使用引擎默认。
    #[serde(default)]
    pub theme_color_dark: Option<String>,
    /// Light 模式下用户对预设 token 的自定义覆盖。
    ///
    /// Key 限制在 4 个核心 token：`primary` / `background` / `foreground` / `border`。
    /// Value 为 `oklch(L C H)` 字符串。前端在保存前会做格式校验,daemon 层只做透传。
    /// 空 map 表示不覆盖任何值,完全跟随当前 preset。
    #[serde(default)]
    pub theme_overrides_light: BTreeMap<String, String>,
    /// Dark 模式下用户对预设 token 的自定义覆盖。语义同 `theme_overrides_light`。
    #[serde(default)]
    pub theme_overrides_dark: BTreeMap<String, String>,
    pub language: Option<String>,
    pub device_name: Option<String>,
    /// Update channel preference. `None` means auto-detect from version string;
    /// `Some(channel)` means the user has overridden the channel.
    pub update_channel: Option<UpdateChannel>,
    /// Whether anonymous diagnostic telemetry is enabled.
    /// When `true` and a Sentry DSN is configured, the app forwards
    /// errors / warnings / structured logs (never clipboard content).
    pub telemetry_enabled: bool,
    /// Whether anonymous product usage analytics is enabled.
    /// 与 `telemetry_enabled` 拆开（schema doc §6.4）：前者控制 Sentry
    /// 错误上报，本字段控制产品 telemetry（漏斗 / 留存 / 同步可靠性事件）。
    /// 二者由用户独立勾选——GDPR 友好实践。
    pub usage_analytics_enabled: bool,
}

impl GeneralSettings {
    /// 解析 light 模式下应使用的主题预设名,带旧字段回退。
    pub fn effective_theme_color_light(&self) -> Option<&str> {
        self.theme_color_light
            .as_deref()
            .or(self.theme_color.as_deref())
    }

    /// 解析 dark 模式下应使用的主题预设名,带旧字段回退。
    pub fn effective_theme_color_dark(&self) -> Option<&str> {
        self.theme_color_dark
            .as_deref()
            .or(self.theme_color.as_deref())
    }
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
#[serde(default)]
pub struct ContentTypes {
    pub text: bool,
    pub image: bool,
    pub link: bool,
    pub file: bool,
    pub code_snippet: bool,
    pub rich_text: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SyncSettings {
    pub auto_sync: bool,
    pub sync_frequency: SyncFrequency,
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
#[serde(default, rename_all = "snake_case")]
pub struct RetentionPolicy {
    pub enabled: bool,
    pub rules: Vec<RetentionRule>,
    pub skip_pinned: bool,
    pub evaluation: RuleEvaluation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
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
    pub auto_unlock_enabled: bool,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
#[serde(default)]
pub struct FileSyncSettings {
    pub file_sync_enabled: bool,
    pub small_file_threshold: u64,
    pub max_file_size: u64,
    pub file_cache_quota_per_device: u64,
    pub file_retention_hours: u32,
    pub file_auto_cleanup: bool,
}

// ======================================================================
// NetworkSettings —— LAN-only Mode（v0.7.0）的后端持久化字段
//
// 反向命名规则（Pitfall 1 防御 — 见 .planning/research/PITFALLS.md §Pitfall 1）：
// - UI = "LAN-only Mode = ON"
// - 后端 = `allow_relay_fallback = false`
// - infra (iroh) = `IrohNodeConfig.disable_relays = true`
//
// 三层语义两次反转。**全工程只允许在 `uc-bootstrap/src/network_policy.rs`
// 唯一一处取反**；DTO ↔ View ↔ core 三层只搬运 `allow_relay_fallback`
// 业务正向语义，不取反。
// ======================================================================

/// 网络相关设置（LAN-only Mode 字段族）。
///
/// `#[serde(default)]` 让缺字段时回退到 `Default::default()`：
/// - `allow_relay_fallback = true`（允许 fallback，breaking change 警惕）
/// - `allow_overlay_network_addrs = false`（默认过滤虚拟网卡候选）
///
/// 修改默认值前请先 grep `LAN-only Mode` 文档与 changelog。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct NetworkSettings {
    /// 是否允许 iroh 在直连失败时回落到公网中继。
    /// `true`（默认）= 允许 fallback，跨网段设备仍可通过 relay 同步；
    /// `false` = LAN-only，禁用 relay，跨网段设备会失联。
    /// 业务正向语义：UI "LAN-only Mode = ON" → 此字段 = `false`。
    pub allow_relay_fallback: bool,

    /// 是否允许把 VPN / overlay 类虚拟网卡地址（CGNAT 100.64.0.0/10、
    /// Tailscale ULA fd7a:115c:a1e0::/48）作为 iroh 直连候选。
    ///
    /// 默认 `false`：默认过滤，避免对端不在同一 tailnet 时把死候选发给 peer，
    /// 拖慢 path-validation、占用 PathId 预算。两端确实都接入同一 VPN
    /// （如 Tailscale）希望让 iroh 借用该 overlay 网络互联时改为 `true`。
    ///
    /// 注意：`198.18.0.0/15`（Clash fake-ip）、`169.254.0.0/16`（IPv4 link-local）
    /// 与本字段无关，永远过滤。
    ///
    /// 修改后需重启 daemon 生效（iroh endpoint bind-time 常量）。
    pub allow_overlay_network_addrs: bool,
}

// ======================================================================
// MobileSyncSettings —— 移动端同步（v1：iOS Shortcut）的总开关。
//
// 字段刻意从一个 `enabled` 起步：v1 SPEC 要求 listen_port / bind_address
// 由 daemon 端常量 + profile offset 自动推导，不暴露给用户配置；
// install_method 是 register flow 的一次性选择而非持久化偏好。这里只
// 持久化"用户是否打开移动端同步监听"这一项，其余调用所需信息（当前
// LAN URL、可用 install_method）由 application 层 use case 在 view 中
// 实时拼装。
//
// 修改 enabled 后需要重启 daemon 才会生效（v1 不做热重载，详见
// `.context/mobile-sync/SPEC.md` §5）。
// ======================================================================

/// 移动端同步功能的设置族。
///
/// v3 SyncClipboard 兼容版(SPEC §14.10)按"功能开关 + 广告 URL 细节"两层
/// 拆分。daemon socket bind 行为是常量推导:`enabled && lan_listen_enabled`
/// 时 bind `0.0.0.0:lan_port`,否则不起 listener。
///
/// * `enabled` —— 总开关。关闭时 daemon 完全不起 LAN listener,即使
///   `lan_listen_enabled=true` 也无效。
/// * `lan_listen_enabled` —— LAN listener 子开关。仅在 `enabled=true` 时
///   生效。让用户能在不撤销已登记设备的情况下临时停用 listener
///   (例如出差换不可信网络时)。
/// * `lan_advertise_ip` —— 用户选定的、要写进 SyncClipboard install URL /
///   二维码的 IPv4 字符串(`192.168.1.5` 这类)。仅决定 iPhone 客户端看到
///   的 base_url,**不**决定 daemon socket 绑哪里(daemon 永远绑 `0.0.0.0`)。
///   从 list-interfaces 候选里挑一个 RFC1918 私有地址。`None` 对应 UI 的
///   "自动"选项,展示 / register_device base_url 都退回 `0.0.0.0`(iPhone
///   连不上,需用户挑一个具体 IP 后再添加设备)。
/// * `lan_port` —— 自定义端口。`None` 时取默认 `42720`(SPEC §3.2)。
///
/// 任意字段变更后都需要重启 daemon 才能生效(v1 不做配置热重载,详见
/// `.context/mobile-sync/SPEC.md` §1.2.5)。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct MobileSyncSettings {
    /// 是否启用移动端同步 LAN 监听总开关。
    /// 默认 `false`:移动端同步对未配对的局域网邻居有暴露面,必须由
    /// 用户在设置页显式开启。
    pub enabled: bool,

    /// LAN listener 子开关。`enabled=true` 时由本字段决定 daemon 启动
    /// 后是否真的把 listener spawn 起来(绑 0.0.0.0:lan_port);否则不起
    /// listener。默认 `false`。
    pub lan_listen_enabled: bool,

    /// 写进 install URL / 二维码的 LAN IPv4 字符串(用户从 list-interfaces
    /// 挑一个)。仅决定 iPhone 端 base_url, 不决定 daemon socket bind
    /// (daemon 永远绑 `0.0.0.0`)。`None` 对应 UI 的"自动"选项,退回
    /// `0.0.0.0`(iPhone 连不上,需挑具体 IP)。
    pub lan_advertise_ip: Option<String>,

    /// 用户自定义的 LAN 监听端口。`None` 时取默认 `42720`(SPEC §3.2)。
    pub lan_port: Option<u16>,
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

    #[serde(default)]
    pub network: NetworkSettings,

    #[serde(default)]
    pub mobile_sync: MobileSyncSettings,
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
