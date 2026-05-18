//! Mobile sync Tauri commands —— GUI 走 in-process facade 直调
//! `MobileSyncFacade`（与 CLI 同模式，不经 webserver）。
//!
//! 6 个对外 command：register / revoke / list devices / get / update settings /
//! list lan interfaces。所有 facade 错误翻译为前端可 pattern-match 的
//! `MobileSyncError` discriminated union。`qr_code_png_bytes` 在 Tauri 边界
//! 转 base64 字符串，前端 `<img src="data:image/png;base64,...">` 直接渲染。

use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::{info_span, Instrument};
use uc_application::facade::mobile_sync::{
    GetMobileSyncSettingsError, LanInterfaceOption, ListLanInterfacesError, ListMobileDevicesError,
    MobileDeviceSummary, MobileSyncFacade, MobileSyncSettingsView,
    RegisterMobileShortcutDeviceError, RegisterMobileShortcutDeviceInput,
    RegisterMobileShortcutDeviceOutput, RevokeMobileDeviceError, RevokeMobileDeviceInput,
    RotateMobilePasswordError, RotateMobilePasswordInput, RotateMobilePasswordOutput,
    ShortcutInstallMethod, ShortcutInstallMethodOption, UpdateMobileSyncSettingsError,
    UpdateMobileSyncSettingsInput, UpdateMobileSyncSettingsOutput,
};
use uc_core::mobile_sync::{MobileClientType, MobileDeviceId};
use uc_platform::ports::observability::TraceMetadata;

use crate::bootstrap::TauriAppRuntime;
use crate::commands::record_trace_fields;

// ============================================================================
// Error taxonomy ─ frontend-facing discriminated union
// ============================================================================

/// 前端直接 `error.code` switch 的 typed 错误。
///
/// 序列化形态：`{"code": "USERNAME_TAKEN", "username": "..."}`。
/// 不复用 `commands::error::CommandError`（那个用了 `tag = "code", content =
/// "message"` 只能带单一字符串负载），mobile_sync 校验错误需要 `min` /
/// `max` / `username` / `reason` 等结构化字段。
#[derive(Debug, Clone, Serialize, specta::Type, thiserror::Error)]
#[serde(
    tag = "code",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
pub enum MobileSyncError {
    #[error("mobile sync facade not available in this runtime")]
    FacadeUnavailable,

    #[error("device label must not be empty")]
    LabelEmpty,

    #[error("device label too long (max {max})")]
    LabelTooLong {
        // 长度上限是个小整数（≤ 256），TS 端拿 `number` 即可。
        // `Number<usize>` 显式声明精度策略，避免 specta 默认 panic。
        #[specta(type = specta_typescript::Number<usize>)]
        max: usize,
    },

    #[error("LAN listener disabled; enable it first")]
    LanListenerDisabled,

    #[error("username already taken: {username}")]
    UsernameTaken { username: String },

    #[error("username too short: must be at least {min} characters (got {got})")]
    UsernameTooShort {
        #[specta(type = specta_typescript::Number<usize>)]
        min: usize,
        #[specta(type = specta_typescript::Number<usize>)]
        got: usize,
    },

    #[error("username too long: must be at most {max} characters (got {got})")]
    UsernameTooLong {
        #[specta(type = specta_typescript::Number<usize>)]
        max: usize,
        #[specta(type = specta_typescript::Number<usize>)]
        got: usize,
    },

    #[error("username must start with an ASCII letter")]
    UsernameMustStartWithLetter,

    #[error("username contains forbidden characters (only letters, digits, underscore allowed)")]
    UsernameContainsForbiddenChars,

    #[error("password too short (min {min})")]
    PasswordTooShort {
        #[specta(type = specta_typescript::Number<usize>)]
        min: usize,
    },

    #[error("password too long (max {max})")]
    PasswordTooLong {
        #[specta(type = specta_typescript::Number<usize>)]
        max: usize,
    },

    #[error("password hashing failed: {message}")]
    PasswordHashFailed { message: String },

    #[error("device not found: {device_id}")]
    DeviceNotFound { device_id: String },

    #[error("invalid LAN parameter: {reason}")]
    InvalidLanParameter { reason: String },

    #[error("settings load failed: {message}")]
    SettingsLoadFailed { message: String },

    #[error("settings save failed: {message}")]
    SettingsSaveFailed { message: String },

    #[error("endpoint info probe failed: {message}")]
    EndpointInfoFailed { message: String },

    #[error("LAN probe failed: {message}")]
    LanProbeFailed { message: String },

    #[error("no usable LAN interface for auto-pick base_url")]
    NoLanInterfaceAvailable,

    #[error("persistence failed: {message}")]
    PersistenceFailed { message: String },

    #[error("QR rendering failed: {message}")]
    QrRenderFailed { message: String },
}

/// label 长度上限——facade 内常量（`MAX_LABEL_LEN = 64`）的镜像。前端校验
/// 与服务端校验一致即可，不需要单独 query API。
const LABEL_MAX_LEN: usize = 64;

impl From<RegisterMobileShortcutDeviceError> for MobileSyncError {
    fn from(err: RegisterMobileShortcutDeviceError) -> Self {
        match err {
            RegisterMobileShortcutDeviceError::LabelEmpty => Self::LabelEmpty,
            RegisterMobileShortcutDeviceError::LabelTooLong => {
                Self::LabelTooLong { max: LABEL_MAX_LEN }
            }
            RegisterMobileShortcutDeviceError::LanListenerDisabled => Self::LanListenerDisabled,
            RegisterMobileShortcutDeviceError::UsernameTaken(username) => {
                Self::UsernameTaken { username }
            }
            RegisterMobileShortcutDeviceError::UsernameTooShort { min, got } => {
                Self::UsernameTooShort { min, got }
            }
            RegisterMobileShortcutDeviceError::UsernameTooLong { max, got } => {
                Self::UsernameTooLong { max, got }
            }
            RegisterMobileShortcutDeviceError::UsernameMustStartWithLetter => {
                Self::UsernameMustStartWithLetter
            }
            RegisterMobileShortcutDeviceError::UsernameContainsForbiddenChars => {
                Self::UsernameContainsForbiddenChars
            }
            RegisterMobileShortcutDeviceError::PasswordTooShort { min } => {
                Self::PasswordTooShort { min }
            }
            RegisterMobileShortcutDeviceError::PasswordTooLong { max } => {
                Self::PasswordTooLong { max }
            }
            RegisterMobileShortcutDeviceError::PasswordHashFailed(message) => {
                Self::PasswordHashFailed { message }
            }
            RegisterMobileShortcutDeviceError::PersistenceFailed(message) => {
                Self::PersistenceFailed { message }
            }
            RegisterMobileShortcutDeviceError::QrRenderFailed(message) => {
                Self::QrRenderFailed { message }
            }
            RegisterMobileShortcutDeviceError::SettingsLoadFailed(message) => {
                Self::SettingsLoadFailed { message }
            }
            RegisterMobileShortcutDeviceError::NoLanInterfaceAvailable => {
                Self::NoLanInterfaceAvailable
            }
            RegisterMobileShortcutDeviceError::LanInterfaceProbeFailed(message) => {
                Self::LanProbeFailed { message }
            }
        }
    }
}

impl From<RevokeMobileDeviceError> for MobileSyncError {
    fn from(err: RevokeMobileDeviceError) -> Self {
        match err {
            RevokeMobileDeviceError::NotFound(device_id) => Self::DeviceNotFound { device_id },
            RevokeMobileDeviceError::PersistenceFailed(message) => {
                Self::PersistenceFailed { message }
            }
        }
    }
}

impl From<ListMobileDevicesError> for MobileSyncError {
    fn from(err: ListMobileDevicesError) -> Self {
        match err {
            ListMobileDevicesError::PersistenceFailed(message) => {
                Self::PersistenceFailed { message }
            }
        }
    }
}

impl From<RotateMobilePasswordError> for MobileSyncError {
    fn from(err: RotateMobilePasswordError) -> Self {
        match err {
            RotateMobilePasswordError::NotFound(id) => Self::DeviceNotFound {
                device_id: id.into_string(),
            },
            RotateMobilePasswordError::PasswordTooShort { min } => Self::PasswordTooShort { min },
            RotateMobilePasswordError::PasswordTooLong { max } => Self::PasswordTooLong { max },
            RotateMobilePasswordError::PasswordHashFailed(message) => {
                Self::PasswordHashFailed { message }
            }
            RotateMobilePasswordError::PersistenceFailed(message) => {
                Self::PersistenceFailed { message }
            }
        }
    }
}

impl From<GetMobileSyncSettingsError> for MobileSyncError {
    fn from(err: GetMobileSyncSettingsError) -> Self {
        match err {
            GetMobileSyncSettingsError::SettingsLoadFailed(message) => {
                Self::SettingsLoadFailed { message }
            }
            GetMobileSyncSettingsError::EndpointInfoFailed(message) => {
                Self::EndpointInfoFailed { message }
            }
        }
    }
}

impl From<UpdateMobileSyncSettingsError> for MobileSyncError {
    fn from(err: UpdateMobileSyncSettingsError) -> Self {
        match err {
            UpdateMobileSyncSettingsError::SettingsLoadFailed(message) => {
                Self::SettingsLoadFailed { message }
            }
            UpdateMobileSyncSettingsError::SettingsSaveFailed(message) => {
                Self::SettingsSaveFailed { message }
            }
            UpdateMobileSyncSettingsError::InvalidLanParameter(reason) => {
                Self::InvalidLanParameter { reason }
            }
        }
    }
}

impl From<ListLanInterfacesError> for MobileSyncError {
    fn from(err: ListLanInterfacesError) -> Self {
        match err {
            ListLanInterfacesError::ProbeFailed(message) => Self::LanProbeFailed { message },
        }
    }
}

// ============================================================================
// Tauri-friendly DTOs
// ============================================================================

/// `register_mobile_device` 入参。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RegisterMobileDeviceArgs {
    pub label: String,
    /// 留空（缺字段或显式 null）走 minter 自动颁发；给值则按规则严格校验。
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

/// `register_mobile_device` 返回值。
///
/// `password` 是**唯一一次**面向前端的明文回显——之后只以 PHC 形式存在于
/// 服务端 sqlite。前端拿到后必须立即在 modal 里展示 + 强制用户勾选"已保存"
/// 才让关闭。
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RegisterMobileDeviceResult {
    pub device_id: String,
    pub label: String,
    pub client_type: String,
    // Unix epoch ms 时间戳；理由同 `TraceMetadata::timestamp`（远在 2^53 内）。
    #[specta(type = specta_typescript::Number<i64>)]
    pub created_at_ms: i64,
    pub base_url: String,
    pub username: String,
    pub password: String,
    /// SyncClipboard "Clipboard EX" iCloud 共享链接(常量)。前端把它放在
    /// "安装快捷指令"次要 tab 里, 不再作为 connect-URI tab 的主 QR 内容。
    pub install_url: String,
    /// `installUrl` 的二维码 PNG, Base64 编码后由前端 `<img src="data:...">`
    /// 直接渲染。让 iPhone 相机直接扫码安装快捷指令, 替代用户在桌面上
    /// 肉眼抄长长的 iCloud 链接到 Safari 的旧体验。与 `qrCodePngBase64`
    /// (编 `connectUri`) 字节不同, 用途也不同 — 前者一次性安装, 后者
    /// 每次添加设备扫一次。
    pub install_qr_code_png_base64: String,
    /// `uniclipboard://connect?v=1&svc=mobile-sync&p=<base64url-json>`。
    /// QR 主内容: iOS Shortcut 扫描后一次性解出 base_url / username /
    /// password 直接填三栏, 替代旧版"用户肉眼抄写"。协议详见
    /// `docs/architecture/mobile-sync-connect-uri.md`。
    pub connect_uri: String,
    /// Base64-encoded PNG bytes; 前端 `<img src="data:image/png;base64,...">`
    /// 直接渲染。当前编码的是 `connectUri`(阶段 2 起), 不再是 `installUrl`。
    pub qr_code_png_base64: String,
}

impl From<RegisterMobileShortcutDeviceOutput> for RegisterMobileDeviceResult {
    fn from(out: RegisterMobileShortcutDeviceOutput) -> Self {
        Self {
            device_id: out.device.device_id.into_string(),
            label: out.device.label,
            client_type: client_type_wire(&out.device.client_type).to_string(),
            created_at_ms: out.device.created_at_ms,
            base_url: out.base_url,
            username: out.username,
            password: out.password,
            install_url: out.install_url,
            install_qr_code_png_base64: BASE64.encode(out.install_qr_code_png_bytes),
            connect_uri: out.connect_uri,
            qr_code_png_base64: BASE64.encode(out.qr_code_png_bytes),
        }
    }
}

fn client_type_wire(t: &MobileClientType) -> &'static str {
    t.as_wire_str()
}

/// `rotate_mobile_password` 入参。`password = None` (字段缺失或 null) 走
/// minter 自动颁发新明文;给值则按规则严格校验。
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RotateMobilePasswordArgs {
    pub device_id: String,
    #[serde(default)]
    pub password: Option<String>,
}

/// `rotate_mobile_password` 返回值。`password` 是**唯一一次**面向用户的
/// 明文回显 —— 之后只以 PHC 形式存在。前端必须立即在 modal 里展示, 并
/// 提示用户同步更新 iPhone shortcut 里的 password 字段(旧密码已立即
/// 失效)。
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RotateMobilePasswordResult {
    pub device_id: String,
    pub username: String,
    pub password: String,
}

impl From<RotateMobilePasswordOutput> for RotateMobilePasswordResult {
    fn from(out: RotateMobilePasswordOutput) -> Self {
        Self {
            device_id: out.device_id.into_string(),
            username: out.username,
            password: out.password,
        }
    }
}

/// `list_mobile_devices` 单条结果。来自 `MobileDeviceSummary`，不含
/// password_hash；username 透传给前端作为辅助识别字段。
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MobileDeviceView {
    pub device_id: String,
    pub label: String,
    pub client_type: String,
    pub username: String,
    // Unix epoch ms；理由同 `TraceMetadata::timestamp`。
    #[specta(type = specta_typescript::Number<i64>)]
    pub created_at_ms: i64,
    #[specta(type = Option<specta_typescript::Number<i64>>)]
    pub last_seen_at_ms: Option<i64>,
    pub last_seen_ip: Option<String>,
    pub reported_name: Option<String>,
    pub reported_os: Option<String>,
}

impl From<MobileDeviceSummary> for MobileDeviceView {
    fn from(s: MobileDeviceSummary) -> Self {
        Self {
            device_id: s.device_id.into_string(),
            label: s.label,
            client_type: client_type_wire(&s.client_type).to_string(),
            username: s.username,
            created_at_ms: s.created_at_ms,
            last_seen_at_ms: s.last_seen_at_ms,
            last_seen_ip: s.last_seen_ip,
            reported_name: s.reported_name,
            reported_os: s.reported_os,
        }
    }
}

/// `get_mobile_sync_settings` 返回值。
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MobileSyncSettingsViewDto {
    pub enabled: bool,
    pub lan_listen_enabled: bool,
    pub lan_advertise_ip: Option<String>,
    pub lan_port: Option<u16>,
    /// daemon 端 LAN listener 的 bind 失败原因(端口占用 / IP 不存在 / 权限)。
    /// `Some` 表示 daemon 真的尝试过 bind 但失败,前端据此显示具体错误。
    /// 监听 URL 不再透出 —— 前端从 `lanAdvertiseIp` + `lanPort` 自行拼接。
    pub lan_listener_error: Option<String>,
    pub shortcut_install_methods: Vec<ShortcutInstallMethodView>,
}

impl From<MobileSyncSettingsView> for MobileSyncSettingsViewDto {
    fn from(v: MobileSyncSettingsView) -> Self {
        Self {
            enabled: v.enabled,
            lan_listen_enabled: v.lan_listen_enabled,
            lan_advertise_ip: v.lan_advertise_ip,
            lan_port: v.lan_port,
            lan_listener_error: v.lan_listener_error,
            shortcut_install_methods: v
                .shortcut_install_methods
                .into_iter()
                .map(Into::into)
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutInstallMethodView {
    /// `tokenInjected` / `icloudGeneric`（v1 仅前者可用）。
    pub method: String,
    pub available: bool,
    pub disabled_reason: Option<String>,
}

impl From<ShortcutInstallMethodOption> for ShortcutInstallMethodView {
    fn from(o: ShortcutInstallMethodOption) -> Self {
        let method = match o.method {
            ShortcutInstallMethod::TokenInjected => "tokenInjected",
            ShortcutInstallMethod::IcloudGeneric => "icloudGeneric",
        };
        Self {
            method: method.to_string(),
            available: o.available,
            disabled_reason: o.disabled_reason,
        }
    }
}

/// `update_mobile_sync_settings` 入参 patch。
///
/// `lanAdvertiseIp` / `lanPort` 是三态：字段缺失=不动；显式 null=清空；给值=写入。
/// 前端用 `JSON.stringify` 时 `undefined` 字段被 drop（缺失），`null` 显式
/// 序列化（→ `Some(None)`），有值（→ `Some(Some(value))`）。
#[derive(Debug, Clone, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMobileSyncSettingsArgs {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub lan_listen_enabled: Option<bool>,
    // `Option<Option<T>>` 三态：JSON wire 上其实只有两种可观察形态——
    // 字段缺失（前端 `JSON.stringify` drop `undefined`）和显式 `null`。
    // 我们用 `Option<Option<T>>` 在 Rust 内部保留三态语义（缺失 = 不动；
    // null = 清空；有值 = 设置），但 wire 类型就是 `T | null` 可选字段。
    //
    // `#[specta(type = Option<T>)]` 把这一约束告诉 specta：生成的 TS 字段
    // 是 `lanAdvertiseIp?: string | null`。如果不显式覆盖，specta 会因为
    // `#[serde(deserialize_with)]` 改变了 wire 类型而拒绝生成 binding。
    #[serde(default, deserialize_with = "deserialize_optional_optional_string")]
    #[specta(type = Option<String>)]
    pub lan_advertise_ip: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_optional_optional_u16")]
    #[specta(type = Option<u16>)]
    pub lan_port: Option<Option<u16>>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMobileSyncSettingsResult {
    pub enabled: bool,
    pub lan_listen_enabled: bool,
    pub lan_advertise_ip: Option<String>,
    pub lan_port: Option<u16>,
    /// Wire-兼容历史字段。daemon 装入 LAN lifecycle controller 时
    /// (GUI 路径) settings 改动即时生效, 本字段永远为 false; CLI fallback
    /// 装配仍按"任一字段实际变化 → true / 同值 → false"返回, 表达
    /// "下一次 daemon 重启才生效"的旧语义。前端按本字段决定是否弹
    /// restart 横幅, true → 弹, false → 即时反馈即可。
    pub restart_required: bool,
    /// 即时生效路径下 LAN listener bind 失败的原因。
    ///
    /// 写盘成功但 adapter `apply(target)` 后端口没起来时(端口占用、权限
    /// 不足、IP 不可分配等),facade 从 `MobileSyncEndpointInfoPort` 读出
    /// `BindFailed{reason}` 并透传到此字段。前端引导对话框据此在 happy
    /// path 流程中提前 toast.error 并阻断下一步, 避免用户填完 label 才
    /// 发现 iPhone 连不上。CLI fallback / 无 lifecycle 装配下永远为 None。
    pub lan_listener_bind_error: Option<String>,
}

impl From<UpdateMobileSyncSettingsOutput> for UpdateMobileSyncSettingsResult {
    fn from(o: UpdateMobileSyncSettingsOutput) -> Self {
        Self {
            enabled: o.enabled,
            lan_listen_enabled: o.lan_listen_enabled,
            lan_advertise_ip: o.lan_advertise_ip,
            lan_port: o.lan_port,
            restart_required: o.restart_required,
            lan_listener_bind_error: o.lan_listener_bind_error,
        }
    }
}

/// `list_mobile_lan_interfaces` 单条结果。
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LanInterfaceView {
    pub name: String,
    pub ipv4: String,
}

impl From<LanInterfaceOption> for LanInterfaceView {
    fn from(o: LanInterfaceOption) -> Self {
        Self {
            name: o.name,
            ipv4: o.ipv4,
        }
    }
}

// `Option<Option<T>>` 三态反序列化。
//
// serde 默认对 `Option<Option<T>>` 把 `null` 跟字段缺失都收敛成 outer
// `None`，区分不出"显式清空"。标准技巧：内层先用 `Option::deserialize`
// 解析（null → None，有值 → Some(value)），再外层无脑 `Some(...)` 包一下。
// 配合 struct 字段上的 `#[serde(default)]`，三态正好对齐：
// - 字段缺失 → default 触发 → outer None
// - 显式 null → Some(None)
// - 有值 → Some(Some(value))
fn deserialize_optional_optional_string<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(deserializer)?))
}

fn deserialize_optional_optional_u16<'de, D>(
    deserializer: D,
) -> Result<Option<Option<u16>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<u16>::deserialize(deserializer)?))
}

// ============================================================================
// Helper
// ============================================================================

fn mobile_sync_facade(
    runtime: &Arc<TauriAppRuntime>,
) -> Result<Arc<MobileSyncFacade>, MobileSyncError> {
    runtime
        .app_facade()
        .mobile_sync
        .get()
        .cloned()
        .ok_or(MobileSyncError::FacadeUnavailable)
}

// ============================================================================
// Commands
// ============================================================================

/// 登记一台 iPhone Shortcut 设备：颁发 (username, password) Basic Auth 凭据 +
/// 渲染 SyncClipboard install URL 的二维码。`password` 在返回值里是**唯一一次**
/// 面向用户的明文回显——前端必须立即展示 + 强制用户勾选"已保存"才允许关闭。
#[tauri::command]
#[specta::specta]
pub async fn register_mobile_device(
    runtime: State<'_, Arc<TauriAppRuntime>>,
    args: RegisterMobileDeviceArgs,
    _trace: Option<TraceMetadata>,
) -> Result<RegisterMobileDeviceResult, MobileSyncError> {
    let span = info_span!(
        "command.mobile_sync.register_device",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let facade = mobile_sync_facade(&runtime)?;
        let input = RegisterMobileShortcutDeviceInput {
            label: args.label,
            username: args.username,
            password: args.password,
        };
        let out = facade.register_device(input).await?;
        Ok(RegisterMobileDeviceResult::from(out))
    }
    .instrument(span)
    .await
}

/// 撤销一台已登记设备。`Ok(())` 表示成功；`DeviceNotFound` 表示设备已不在
/// 仓储里（UI 列表过期），前端据此提示刷新。
#[tauri::command]
#[specta::specta]
pub async fn revoke_mobile_device(
    runtime: State<'_, Arc<TauriAppRuntime>>,
    device_id: String,
    _trace: Option<TraceMetadata>,
) -> Result<(), MobileSyncError> {
    let span = info_span!(
        "command.mobile_sync.revoke_device",
        device_id = %device_id,
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let facade = mobile_sync_facade(&runtime)?;
        facade
            .revoke_device(RevokeMobileDeviceInput {
                device_id: MobileDeviceId::new(device_id),
            })
            .await?;
        Ok(())
    }
    .instrument(span)
    .await
}

/// 列出已登记设备。结果按"最近活跃 desc → 创建时间 desc"排序。不含
/// password_hash；username 作为辅助识别字段透传给前端。
#[tauri::command]
#[specta::specta]
pub async fn list_mobile_devices(
    runtime: State<'_, Arc<TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<Vec<MobileDeviceView>, MobileSyncError> {
    let span = info_span!(
        "command.mobile_sync.list_devices",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let facade = mobile_sync_facade(&runtime)?;
        let devices = facade.list_devices().await?;
        Ok(devices.into_iter().map(Into::into).collect())
    }
    .instrument(span)
    .await
}

/// 给一台已登记设备换一份新密码。`password = None`(字段缺失 / null)走
/// minter 自动颁发;给值则按 8–256 字符校验。返回值 `password` 是**唯一一次**
/// 明文回显 —— 之后只以 PHC 存在,UI 必须立即展示并告知用户同步更新 iPhone
/// shortcut 里的 password 字段(旧密码已立即失效)。
#[tauri::command]
#[specta::specta]
pub async fn rotate_mobile_password(
    runtime: State<'_, Arc<TauriAppRuntime>>,
    args: RotateMobilePasswordArgs,
    _trace: Option<TraceMetadata>,
) -> Result<RotateMobilePasswordResult, MobileSyncError> {
    let span = info_span!(
        "command.mobile_sync.rotate_password",
        device_id = %args.device_id,
        custom_password = args.password.is_some(),
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let facade = mobile_sync_facade(&runtime)?;
        let input = RotateMobilePasswordInput {
            device_id: MobileDeviceId::new(args.device_id),
            password: args.password,
        };
        let out = facade.rotate_password(input).await?;
        Ok(RotateMobilePasswordResult::from(out))
    }
    .instrument(span)
    .await
}

/// 读移动端同步设置 + 当前 LAN URL + 可用 install methods 的合成视图。
#[tauri::command]
#[specta::specta]
pub async fn get_mobile_sync_settings(
    runtime: State<'_, Arc<TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<MobileSyncSettingsViewDto, MobileSyncError> {
    let span = info_span!(
        "command.mobile_sync.get_settings",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let facade = mobile_sync_facade(&runtime)?;
        let view = facade.get_settings().await?;
        Ok(view.into())
    }
    .instrument(span)
    .await
}

/// 更新移动端同步设置。GUI daemon 装配下 LAN listener 由
/// `MobileLanLifecyclePort` 即时切换,无需重启;`restart_required` 字段
/// 的具体语义见 [`UpdateMobileSyncSettingsResult::restart_required`] 文档。
#[tauri::command]
#[specta::specta]
pub async fn update_mobile_sync_settings(
    runtime: State<'_, Arc<TauriAppRuntime>>,
    args: UpdateMobileSyncSettingsArgs,
    _trace: Option<TraceMetadata>,
) -> Result<UpdateMobileSyncSettingsResult, MobileSyncError> {
    let span = info_span!(
        "command.mobile_sync.update_settings",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let facade = mobile_sync_facade(&runtime)?;
        let input = UpdateMobileSyncSettingsInput {
            enabled: args.enabled,
            lan_listen_enabled: args.lan_listen_enabled,
            lan_advertise_ip: args.lan_advertise_ip,
            lan_port: args.lan_port,
        };
        let out = facade.update_settings(input).await?;
        Ok(out.into())
    }
    .instrument(span)
    .await
}

/// 列出可作为二维码 URL 候选的本机 IPv4 LAN 接口。仅返回 RFC1918 私有地址，
/// 按 10/8 → 172.16/12 → 192.168/16 排序。
#[tauri::command]
#[specta::specta]
pub async fn list_mobile_lan_interfaces(
    runtime: State<'_, Arc<TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<Vec<LanInterfaceView>, MobileSyncError> {
    let span = info_span!(
        "command.mobile_sync.list_lan_interfaces",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let facade = mobile_sync_facade(&runtime)?;
        let interfaces = facade.list_lan_interfaces().await?;
        Ok(interfaces.into_iter().map(Into::into).collect())
    }
    .instrument(span)
    .await
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    //! 边界层 wiring 测试：error 翻译表 / Tauri-friendly DTO 序列化形态。
    //! 业务语义在 `uc-application::usecases::mobile_sync` 各 use case 单测覆盖。

    use super::*;

    #[test]
    fn error_password_too_short_serializes_with_min_field() {
        let err = MobileSyncError::PasswordTooShort { min: 8 };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "PASSWORD_TOO_SHORT");
        assert_eq!(json["min"], 8);
    }

    #[test]
    fn error_username_taken_serializes_with_username_field() {
        let err = MobileSyncError::UsernameTaken {
            username: "alice".to_string(),
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "USERNAME_TAKEN");
        assert_eq!(json["username"], "alice");
    }

    #[test]
    fn error_device_not_found_serializes_camel_case_device_id_field() {
        // 验证 rename_all_fields = "camelCase":Rust 内部 `device_id` 字段
        // 序列化为前端友好的 `deviceId`,与其余 DTO 一致。
        let err = MobileSyncError::DeviceNotFound {
            device_id: "did_abc".to_string(),
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "DEVICE_NOT_FOUND");
        assert_eq!(json["deviceId"], "did_abc");
        assert!(json.get("device_id").is_none());
    }

    #[test]
    fn error_facade_unavailable_serializes_unit_variant() {
        let err = MobileSyncError::FacadeUnavailable;
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "FACADE_UNAVAILABLE");
        // unit variant 只有 code 字段；其余 absent
        assert!(json.get("message").is_none());
    }

    #[test]
    fn error_label_too_long_translation_uses_constant_max() {
        let app_err = RegisterMobileShortcutDeviceError::LabelTooLong;
        let mapped = MobileSyncError::from(app_err);
        match mapped {
            MobileSyncError::LabelTooLong { max } => assert_eq!(max, LABEL_MAX_LEN),
            other => panic!("unexpected mapping: {other:?}"),
        }
    }

    #[test]
    fn update_args_three_state_lan_advertise_ip_absent() {
        // 字段缺失 → outer None
        let json = r#"{}"#;
        let args: UpdateMobileSyncSettingsArgs = serde_json::from_str(json).unwrap();
        assert!(args.lan_advertise_ip.is_none());
    }

    #[test]
    fn update_args_three_state_lan_advertise_ip_explicit_null() {
        // 显式 null → Some(None)
        let json = r#"{"lanAdvertiseIp": null}"#;
        let args: UpdateMobileSyncSettingsArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.lan_advertise_ip, Some(None));
    }

    #[test]
    fn update_args_three_state_lan_advertise_ip_with_value() {
        // 有值 → Some(Some(value))
        let json = r#"{"lanAdvertiseIp": "192.168.1.5"}"#;
        let args: UpdateMobileSyncSettingsArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.lan_advertise_ip, Some(Some("192.168.1.5".to_string())));
    }

    #[test]
    fn update_args_three_state_lan_port_explicit_null() {
        let json = r#"{"lanPort": null}"#;
        let args: UpdateMobileSyncSettingsArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.lan_port, Some(None));
    }

    #[test]
    fn register_args_username_password_optional() {
        // 不给 username / password → 走 minter 自动颁发路径
        let json = r#"{"label": "iPhone"}"#;
        let args: RegisterMobileDeviceArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.label, "iPhone");
        assert!(args.username.is_none());
        assert!(args.password.is_none());
    }

    #[test]
    fn register_result_qr_is_base64_encoded() {
        // 用一个 facade output 走 conversion，断言 PNG bytes 进 base64
        use uc_core::mobile_sync::{MobileClientType, MobileDevice, MobileDeviceId};
        let png_bytes = vec![0x89, 0x50, 0x4E, 0x47]; // 不需要真 PNG 头，断言 encode 结果即可
        let out = RegisterMobileShortcutDeviceOutput {
            device: MobileDevice {
                device_id: MobileDeviceId::new("did_test"),
                label: "Test".to_string(),
                client_type: MobileClientType::IosShortcut,
                username: "mobile_abcd1234".to_string(),
                password_hash: "$argon2id$...".to_string(),
                created_at_ms: 0,
                last_seen_at_ms: None,
                last_seen_ip: None,
                reported_name: None,
                reported_os: None,
            },
            base_url: "http://192.168.1.5:42720".to_string(),
            username: "mobile_abcd1234".to_string(),
            password: "secretpw".to_string(),
            install_url: "https://www.icloud.com/shortcuts/abc".to_string(),
            install_qr_code_png_bytes: vec![0x89, 0x50, 0x4E, 0x47, 0xAB],
            connect_uri: "uniclipboard://connect?v=1&svc=mobile-sync&p=fixture".to_string(),
            qr_code_png_bytes: png_bytes.clone(),
            qr_code_ascii: "...".to_string(),
        };
        let dto = RegisterMobileDeviceResult::from(out);
        assert_eq!(dto.qr_code_png_base64, BASE64.encode(&png_bytes));
        assert_eq!(
            dto.install_qr_code_png_base64,
            BASE64.encode([0x89, 0x50, 0x4E, 0x47, 0xAB]),
            "install QR PNG bytes must be base64-encoded the same way as the main QR"
        );
        assert_eq!(dto.client_type, "ios_shortcut");
        assert_eq!(dto.device_id, "did_test");
        assert_eq!(dto.password, "secretpw");
        // install_url 与 connect_uri 都是一次性回显字段, 透传不再做编码转换。
        assert_eq!(dto.install_url, "https://www.icloud.com/shortcuts/abc");
        assert_eq!(
            dto.connect_uri,
            "uniclipboard://connect?v=1&svc=mobile-sync&p=fixture"
        );
    }

    #[test]
    fn register_result_serializes_connect_uri_camel_case() {
        // Tauri / specta 边界: 字段在 wire 上必须是 camelCase `connectUri`,
        // 这是前端 TS DTO 与 Rust struct 的接口契约。如果未来重命名了
        // serde rename_all 或字段名, 这个测试会立刻失败。
        use uc_core::mobile_sync::{MobileClientType, MobileDevice, MobileDeviceId};
        let out = RegisterMobileShortcutDeviceOutput {
            device: MobileDevice {
                device_id: MobileDeviceId::new("did_test"),
                label: "Test".to_string(),
                client_type: MobileClientType::IosShortcut,
                username: "mobile_abcd1234".to_string(),
                password_hash: "$argon2id$...".to_string(),
                created_at_ms: 0,
                last_seen_at_ms: None,
                last_seen_ip: None,
                reported_name: None,
                reported_os: None,
            },
            base_url: "http://192.168.1.5:42720".to_string(),
            username: "mobile_abcd1234".to_string(),
            password: "secretpw".to_string(),
            install_url: "https://example.com".to_string(),
            install_qr_code_png_bytes: vec![],
            connect_uri: "uniclipboard://connect?v=1&svc=mobile-sync&p=X".to_string(),
            qr_code_png_bytes: vec![],
            qr_code_ascii: String::new(),
        };
        let dto = RegisterMobileDeviceResult::from(out);
        let json = serde_json::to_string(&dto).expect("serialize");
        assert!(
            json.contains("\"connectUri\":\"uniclipboard://connect?v=1&svc=mobile-sync&p=X\""),
            "missing camelCase connectUri in: {json}"
        );
        assert!(json.contains("\"installUrl\":\"https://example.com\""));
        // 阶段 5: install QR 走 wire camelCase, 与 qrCodePngBase64 对称。
        assert!(
            json.contains("\"installQrCodePngBase64\":\"\""),
            "missing camelCase installQrCodePngBase64 in: {json}"
        );
    }

    #[test]
    fn rotate_args_password_optional() {
        // 不给 password → 走 minter 自动颁发
        let json = r#"{"deviceId": "did_x"}"#;
        let args: RotateMobilePasswordArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.device_id, "did_x");
        assert!(args.password.is_none());
    }

    #[test]
    fn rotate_args_with_custom_password_camel_case() {
        let json = r#"{"deviceId": "did_x", "password": "brand-new"}"#;
        let args: RotateMobilePasswordArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.device_id, "did_x");
        assert_eq!(args.password.as_deref(), Some("brand-new"));
    }

    #[test]
    fn rotate_result_serializes_camel_case() {
        // 走 facade output → DTO 转换路径,断言 device_id / username / password
        // 都按 camelCase 序列化(前端 wire 形态)。
        use uc_core::mobile_sync::MobileDeviceId;
        let dto = RotateMobilePasswordResult::from(RotateMobilePasswordOutput {
            device_id: MobileDeviceId::new("did_rot"),
            username: "mobile_alice".into(),
            password: "fresh-pw".into(),
        });
        let json = serde_json::to_value(&dto).unwrap();
        assert_eq!(json["deviceId"], "did_rot");
        assert_eq!(json["username"], "mobile_alice");
        assert_eq!(json["password"], "fresh-pw");
    }

    #[test]
    fn rotate_error_not_found_translates_to_device_not_found_with_camel_case() {
        let app_err = RotateMobilePasswordError::NotFound(
            uc_core::mobile_sync::MobileDeviceId::new("did_ghost"),
        );
        let mapped = MobileSyncError::from(app_err);
        let json = serde_json::to_value(&mapped).unwrap();
        assert_eq!(json["code"], "DEVICE_NOT_FOUND");
        assert_eq!(json["deviceId"], "did_ghost");
    }

    #[test]
    fn rotate_error_password_too_short_translates_with_min_field() {
        let app_err = RotateMobilePasswordError::PasswordTooShort { min: 8 };
        let mapped = MobileSyncError::from(app_err);
        let json = serde_json::to_value(&mapped).unwrap();
        assert_eq!(json["code"], "PASSWORD_TOO_SHORT");
        assert_eq!(json["min"], 8);
    }

    #[test]
    fn install_method_serializes_camelcase_strings() {
        let opt = ShortcutInstallMethodOption {
            method: ShortcutInstallMethod::IcloudGeneric,
            available: false,
            disabled_reason: Some("v2 only".to_string()),
        };
        let view: ShortcutInstallMethodView = opt.into();
        assert_eq!(view.method, "icloudGeneric");
        assert!(!view.available);

        let opt2 = ShortcutInstallMethodOption {
            method: ShortcutInstallMethod::TokenInjected,
            available: true,
            disabled_reason: None,
        };
        let view2: ShortcutInstallMethodView = opt2.into();
        assert_eq!(view2.method, "tokenInjected");
    }
}
