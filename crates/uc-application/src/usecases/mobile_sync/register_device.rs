//! `RegisterMobileShortcutDeviceUseCase` —— 在 daemon 上登记一台 iPhone
//! Shortcut 客户端,颁发其独立 (username, password) Basic Auth 凭据。
//!
//! v3 SyncClipboard 兼容路径(`.context/mobile-sync/SPEC.md` §14)。
//!
//! ## 凭据来源
//!
//! 调用方可以选择:
//! 1. 全自动 —— input.username / input.password 均 `None`,minter 一次性
//!    颁发 (username, password, password_hash, device_id) 四元组,minter
//!    内部用 OsRng 保证不可猜。
//! 2. 完全自定义 —— input 同时给 username 和 password,本 use case 校验
//!    格式 / 长度 / 唯一性,使用 minter 仅取一个 device_id。
//! 3. 部分自定义 —— 只给 username 或只给 password。**未给的那一项**仍走
//!    minter 自动生成路径,已给的那一项用自定义路径校验后落库。
//!
//! 三种模式共享同一个 happy path 出口 —— 三类校验失败都翻译成
//! [`RegisterMobileShortcutDeviceError`] 的对应变体。
//!
//! 失败一律走 [`RegisterMobileShortcutDeviceError`] —— 把底层 port 错误
//! 翻译为用户/调用方能理解的语义(`uc-application/AGENTS.md` §13)。

use std::sync::Arc;

use tracing::{info, instrument, warn};

use uc_core::mobile_sync::{
    LanInterface, MintedCredentials, MobileClientType, MobileDevice, MobileDeviceError,
};
use uc_core::ports::{
    ClockPort, FindMobileDeviceByUsernamePort, LanInterfaceProbeError, LanInterfaceProbePort,
    MobileCredentialsMinterPort, PasswordHasherError, PasswordHasherPort, SaveMobileDevicePort,
    SettingsPort,
};
use uc_core::settings::model::MobileSyncSettings;
use uc_observability::analytics::{AnalyticsPort, Event};

use super::connect_uri::{build_mobile_sync_connect_uri, ConnectUriError, ConnectUriOther};
use super::list_lan_interfaces::may_advertise_interface;

// ─── public-shaped (input / output / error) ─────────────────────────────

/// 调用方提交的请求。`username` / `password` 留空(`None`)走自动颁发;给
/// 值则按本 use case 的校验规则强制 —— 详见模块顶部三模式说明。
#[derive(Debug, Clone, Default)]
pub struct RegisterMobileShortcutDeviceInput {
    /// 必填:用户可读设备标签,非空且 ≤ [`MAX_LABEL_LEN`] 字符。
    pub label: String,
    /// 可选:用户自定义 username。给值时按 [`MIN_USERNAME_LEN`] /
    /// [`MAX_USERNAME_LEN`] / `[A-Za-z0-9_]` / 字母开头 / 与现有设备不冲突
    /// 的规则严格校验。
    pub username: Option<String>,
    /// 可选:用户自定义明文密码。给值时按 [`MIN_PASSWORD_LEN`] /
    /// [`MAX_PASSWORD_LEN`] 校验,**不**强制复杂度(iPhone 端输入不便, 用户
    /// 自取风险自担, NIST 现代指南底线)。
    pub password: Option<String>,
}

/// 颁发成功后的产物。
///
/// `password` 字段是**唯一一次**面向用户回显的明文密码 —— 之后该值仅以
/// `password_hash` 形式存在于服务端 sqlite,无法再次取回。前端 / CLI 必须
/// 在本次响应里就把它展示给用户(配合"复制"按钮)。
#[derive(Debug, Clone)]
pub struct RegisterMobileShortcutDeviceOutput {
    /// 服务端持久化的设备实体(包含 username / password_hash 等)。注意
    /// 调用方若要把它原样转发给上层 view,应再过一次 summary 类型,避免
    /// password_hash 暴露给 UI(`list_devices::MobileDeviceSummary` 已实现)。
    pub device: MobileDevice,
    /// daemon 当前对外公布的 base URL,用户在 SyncClipboard shortcut 里
    /// 填进 `url` 框。LAN 形态形如 `http://192.168.1.5:42720`;若用户配置了
    /// `lan_advertise_base_url`,则为该完整地址(可为 `https://域名`)。
    pub base_url: String,
    /// 一次性回显:用户在 SyncClipboard shortcut 里填进 `username` 框。
    /// 自定义模式下与 `input.username` 相同;自动模式下来自 minter。
    pub username: String,
    /// 一次性回显:明文密码,用户在 SyncClipboard shortcut 里填进 `password` 框。
    /// 自定义模式下与 `input.password` 相同;自动模式下来自 minter。
    pub password: String,
    /// SyncClipboard "Clipboard EX" iCloud 共享链接(常量) —— iOS 用户**首次**
    /// 接入时需先安装该 shortcut, 之后才能扫 `connect_uri` 自动填三栏。
    /// 前端把它放在"安装快捷指令"次要 tab 里, 不再作为 QR 主内容。
    pub install_url: String,
    /// `install_url` 的二维码 PNG 字节流。前端"安装快捷指令"次要 tab 把它
    /// 渲染成 QR 让 iPhone 相机直接扫(替代用户在桌面上肉眼抄长长的 iCloud
    /// 链接到 Safari)。内容是 `install_url` 字面值, 是一个常量;
    /// 与 `qr_code_png_bytes`(编 `connect_uri`)字节不同, 用途也不同。
    pub install_qr_code_png_bytes: Vec<u8>,
    /// `uniclipboard://connect?v=1&svc=mobile-sync&p=<base64url-json>` 深链。
    ///
    /// 单一 QR 内容真相 —— 协议 v1 详见
    /// `docs/architecture/mobile-sync-connect-uri.md`。iOS Shortcut 拿到该
    /// URI 后可一次性解出 `base_url / username / password` 并直接写入三栏,
    /// 替代用户肉眼抄写的旧体验。
    ///
    /// 与 `username` / `password` 等明文字段同源, 同样仅本次响应回显;
    /// 之后服务端只持有 `password_hash`, 重新生成 QR 必须重新调用本 use case。
    pub connect_uri: String,
    /// `connect_uri` 的二维码 PNG 字节流, 前端走 base64 data URL 直接渲染。
    pub qr_code_png_bytes: Vec<u8>,
    /// `connect_uri` 的二维码 ASCII(块字符), CLI 直接 `println!`。
    pub qr_code_ascii: String,
}

/// use case 失败的全部语义。
#[derive(Debug, thiserror::Error)]
pub enum RegisterMobileShortcutDeviceError {
    /// 标签为空 —— UI / CLI 应在用户提交前先校验,这里是兜底。
    #[error("device label must not be empty")]
    LabelEmpty,

    /// 标签过长(超过 64 字符)—— 防止配置串 / sqlite 行被滥用为 BLOB。
    #[error("device label too long (max 64 chars)")]
    LabelTooLong,

    /// LAN 监听未启用 —— 没有可写入 SyncClipboard shortcut 的 base_url,
    /// 必须先开启。
    #[error("LAN listener is not enabled; enable it first")]
    LanListenerDisabled,

    /// 自定义 username 已被其它已登记设备占用。
    #[error("username already taken: {0}")]
    UsernameTaken(String),

    /// 自定义 username 长度低于 [`MIN_USERNAME_LEN`]。
    #[error("username too short: must be at least {min} characters (got {got})")]
    UsernameTooShort { min: usize, got: usize },

    /// 自定义 username 长度超过 [`MAX_USERNAME_LEN`]。
    #[error("username too long: must be at most {max} characters (got {got})")]
    UsernameTooLong { max: usize, got: usize },

    /// 自定义 username 首字符不是 ASCII 字母 —— 避免 Basic Auth header 解析歧义。
    #[error("username must start with an ASCII letter")]
    UsernameMustStartWithLetter,

    /// 自定义 username 含 `[A-Za-z0-9_]` 以外的字符。
    #[error("username contains forbidden characters (only letters, digits, underscore allowed)")]
    UsernameContainsForbiddenChars,

    /// 自定义 password 长度低于 [`MIN_PASSWORD_LEN`]。
    #[error("password too short (min {min} chars)")]
    PasswordTooShort { min: usize },

    /// 自定义 password 长度超过 [`MAX_PASSWORD_LEN`]。Argon2id DOS 防护。
    #[error("password too long (max {max} chars)")]
    PasswordTooLong { max: usize },

    /// 自定义 password 哈希失败(算法库内部错误)。
    #[error("password hashing failed: {0}")]
    PasswordHashFailed(String),

    /// 持久化失败(重复 device id / username 碰撞 / 底层存储错误)。
    #[error("device persistence failed: {0}")]
    PersistenceFailed(String),

    /// 二维码渲染失败(URL 过长 / qrcode 库内部错误)。install_url 是已知常量,
    /// 实际只有 PNG 编码失败时才会触发。
    #[error("qr code rendering failed: {0}")]
    QrRenderFailed(String),

    /// 读取 settings 失败 —— 用于 base_url 推导。错误是真正的失败,
    /// 应当告知用户并支持重试。
    #[error("settings load failed: {0}")]
    SettingsLoadFailed(String),

    /// 没有任何可进码的候选地址:无公网入口、无钉死 IP,且本机检测不到
    /// 任何合格网卡(RFC1918 / Tailscale CGNAT)—— iPhone 没有可达的
    /// base_url。用户需先连入 LAN 或在配置里手动指定 IP / 公网地址。
    #[error("no usable LAN interface for auto-pick base_url")]
    NoLanInterfaceAvailable,

    /// 探测 LAN 接口失败(底层 syscall 错误)。
    #[error("lan interface probe failed: {0}")]
    LanInterfaceProbeFailed(String),
}

// ─── use case ───────────────────────────────────────────────────────────

/// 设备标签最大长度。
const MAX_LABEL_LEN: usize = 64;

/// `lan_port` 缺省值（SPEC §3.2）。
const DEFAULT_LAN_PORT: u16 = 42720;
/// `urls` 候选去重后的截断上限（`docs/planning/mobile-sync-qr-multi-url.md` §5.4）。
const MAX_ADVERTISE_URLS: usize = 20;

/// 自定义 username 最小长度。
pub const MIN_USERNAME_LEN: usize = 6;
/// 自定义 username 最大长度。
pub const MAX_USERNAME_LEN: usize = 32;
/// 自定义 password 最小长度。**用户选"宽松"** —— 不强制复杂度(iPhone
/// 输入不便),只设 NIST 现代指南底线。
pub const MIN_PASSWORD_LEN: usize = 8;
/// 自定义 password 最大长度。Argon2id 哈希前的输入上限,防 DOS。
pub const MAX_PASSWORD_LEN: usize = 256;

/// SyncClipboard "Clipboard EX" iCloud 共享链接(v3 v1 唯一支持的客户端
/// 入口)。Apple 已签名,可被任何 iPhone 在开启「允许不受信任的快捷指令」
/// 之前直接安装(走 iCloud 信任路径)。
///
/// 该常量与 `.context/mobile-sync/SPEC.md` §14.2 + findings.md v3 段落对齐;
/// 升级 v2 引入 ClipboardAuto 时新增一个 install URL 的常量,不替换本值。
pub const SYNC_CLIPBOARD_EX_INSTALL_URL: &str =
    "https://www.icloud.com/shortcuts/9c2319d7d6404521b941271e89194f30";

pub(crate) struct RegisterMobileShortcutDeviceUseCase {
    credentials_minter: Arc<dyn MobileCredentialsMinterPort>,
    password_hasher: Arc<dyn PasswordHasherPort>,
    find_by_username: Arc<dyn FindMobileDeviceByUsernamePort>,
    save: Arc<dyn SaveMobileDevicePort>,
    settings: Arc<dyn SettingsPort>,
    clock: Arc<dyn ClockPort>,
    lan_interface_probe: Arc<dyn LanInterfaceProbePort>,
    /// schema doc §7.6 / §12.2 P1：iPhone Shortcut 集成的启用计数 anchor。
    /// happy path 在 repository.save 成功之后 emit `MobileDeviceRegistered`;
    /// 任何前置校验失败 / 持久化失败都不上报——doc §12.5 已明确撤销 /
    /// rotate 等高频但低产品意义的事件不埋。
    analytics: Arc<dyn AnalyticsPort>,
}

impl RegisterMobileShortcutDeviceUseCase {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        credentials_minter: Arc<dyn MobileCredentialsMinterPort>,
        password_hasher: Arc<dyn PasswordHasherPort>,
        find_by_username: Arc<dyn FindMobileDeviceByUsernamePort>,
        save: Arc<dyn SaveMobileDevicePort>,
        settings: Arc<dyn SettingsPort>,
        clock: Arc<dyn ClockPort>,
        lan_interface_probe: Arc<dyn LanInterfaceProbePort>,
        analytics: Arc<dyn AnalyticsPort>,
    ) -> Self {
        Self {
            credentials_minter,
            password_hasher,
            find_by_username,
            save,
            settings,
            clock,
            lan_interface_probe,
            analytics,
        }
    }

    /// 收集要写进二维码的全部候选地址（`docs/planning/mobile-sync-qr-multi-url.md` §5），
    /// 产出有序、去重、截断后的列表；`urls[0]` 即 v1 语义的主 `url`。
    ///
    /// 收集顺序：
    /// 1. 公网入口 `lan_advertise_base_url`（Some 时，原样，恒排首位）；
    /// 2. 用户钉死的 `lan_advertise_ip`（Some 时）—— 无公网入口时它就是
    ///    `urls[0]`，与 v1 的 `url` 取值保持一致；
    /// 3. 全部合格网卡 IP：[`may_advertise_interface`] 口径（RFC1918 +
    ///    Tailscale CGNAT 100.64/10 的宽地址段，且剔除 docker0 / veth* /
    ///    br-* 容器虚拟网卡），按 10/8 → 172.16/12 → 192.168/16 →
    ///    100.64/10 桶序、段内 IPv4 数值序。下拉展示（`list_lan_interfaces`）
    ///    与此处共用同一判定，保证两处口径不漂移。
    ///
    /// 网卡探测失败时：若 1/2 已有候选则降级继续（v1 在这两条路径下根本
    /// 不探测网卡，不能让探测失败反过来弄死老路径），否则照旧报
    /// [`RegisterMobileShortcutDeviceError::LanInterfaceProbeFailed`]。
    async fn collect_advertise_urls(
        &self,
        mobile_sync: &MobileSyncSettings,
    ) -> Result<Vec<String>, RegisterMobileShortcutDeviceError> {
        let port = mobile_sync.lan_port.unwrap_or(DEFAULT_LAN_PORT);
        let mut candidates: Vec<String> = Vec::new();

        if let Some(url) = mobile_sync.lan_advertise_base_url.clone() {
            // 已在 update_settings 写入时校验 + 归一化（无尾斜杠）。
            candidates.push(url);
        }
        if let Some(ip) = mobile_sync.lan_advertise_ip.as_deref() {
            candidates.push(format!("http://{ip}:{port}"));
        }

        match self.lan_interface_probe.list_interfaces().await {
            Ok(raw) => {
                let mut nics: Vec<LanInterface> =
                    raw.into_iter().filter(may_advertise_interface).collect();
                nics.sort_by(|a, b| {
                    advertise_bucket(&a.ipv4.octets())
                        .cmp(&advertise_bucket(&b.ipv4.octets()))
                        .then_with(|| a.ipv4.cmp(&b.ipv4))
                });
                candidates.extend(
                    nics.into_iter()
                        .map(|iface| format!("http://{}:{port}", iface.ipv4)),
                );
            }
            Err(err) if !candidates.is_empty() => {
                warn!(
                    error = %err,
                    "lan interface probe failed; QR will only carry configured advertise entries"
                );
            }
            Err(err) => return Err(translate_probe_error(err)),
        }

        // 按最终字符串去重，保留靠前位置（公网入口/钉死 IP 撞上网卡 URL 时
        // 只留一份且位置不变）。
        let mut seen = std::collections::HashSet::new();
        candidates.retain(|url| seen.insert(url.clone()));

        if candidates.is_empty() {
            return Err(RegisterMobileShortcutDeviceError::NoLanInterfaceAvailable);
        }

        // 截断不得静默（规格 §5.4）。
        if candidates.len() > MAX_ADVERTISE_URLS {
            warn!(
                dropped = candidates.len() - MAX_ADVERTISE_URLS,
                max = MAX_ADVERTISE_URLS,
                "advertise url candidates exceed cap; truncating"
            );
            candidates.truncate(MAX_ADVERTISE_URLS);
        }

        Ok(candidates)
    }

    /// 登记一台新 iPhone Shortcut 设备。
    ///
    /// 候选地址由 settings 决定（[`collect_advertise_urls`]）:
    /// `lan_listen_enabled=false` → `LanListenerDisabled`(用户没开 LAN);
    /// 否则 `urls = [公网入口?, 钉死 IP?, ...全部合格网卡 IP]`,
    /// `base_url = urls[0]`(v1 主 `url` 语义不变);
    /// 全空时 → `NoLanInterfaceAvailable`。
    ///
    /// 不依赖 `MobileSyncEndpointInfoPort`(那是 daemon 进程内运行时状
    /// 态, CLI 进程不可达)。
    ///
    /// [`collect_advertise_urls`]: Self::collect_advertise_urls
    ///
    /// happy path 不可中途部分提交:repository 写成功后, 后续二维码渲染
    /// 失败会留下"已登记但用户拿不到 install URL"的孤儿记录。v1 接受
    /// 该缺陷 —— 用户重新点"添加 iPhone"即可生成新设备;旧的孤儿设备
    /// 会被显示在列表里, 撤销即可清理。
    #[instrument(
        skip(self, input),
        fields(
            label_len = input.label.len(),
            custom_username = input.username.is_some(),
            custom_password = input.password.is_some(),
        )
    )]
    pub(crate) async fn execute(
        &self,
        input: RegisterMobileShortcutDeviceInput,
    ) -> Result<RegisterMobileShortcutDeviceOutput, RegisterMobileShortcutDeviceError> {
        // 0. 标签前置校验 —— 兜底, 不依赖上层。
        let label = validate_label(input.label)?;

        // 0.1 自定义凭据形态前置校验 —— 在 settings / minter 之前做, 让
        //     "格式不合法"快速失败,避免无谓的 IO。username 先 trim 再校验
        //     (空格不算合法字符);password 不 trim(用户密码可能含前后空格)。
        let custom_username = input.username.as_ref().map(|u| u.trim().to_string());
        if let Some(ref u) = custom_username {
            validate_username_shape(u)?;
        }
        if let Some(ref p) = input.password {
            validate_password_length(p)?;
        }

        // 1. 读 settings 决定 base_url —— 没开 LAN 监听就直接拒绝, 避免
        //    颁发了凭据却没 base_url 给用户的尴尬中间态。
        let settings = self.settings.load().await.map_err(|err| {
            RegisterMobileShortcutDeviceError::SettingsLoadFailed(err.to_string())
        })?;
        if !settings.mobile_sync.lan_listen_enabled {
            return Err(RegisterMobileShortcutDeviceError::LanListenerDisabled);
        }
        // 候选地址全收集(docs/planning/mobile-sync-qr-multi-url.md §5):公网入口 +
        // 钉死 IP + 全部合格网卡,一并进码让扫码端逐个探活;`urls[0]` 即
        // v1 语义的主 `url`,老客户端只读它。daemon 永远 bind
        // `0.0.0.0:lan_port`,但 iPhone 得到的候选必须是真实可达的地址
        // (0.0.0.0 / 127.0.0.1 在 iPhone 上都连不通)。
        let advertise_urls = self.collect_advertise_urls(&settings.mobile_sync).await?;
        let base_url = advertise_urls[0].clone();

        // 2. 颁发凭据 —— minter 一次性给 4 项 baseline;然后按 input
        //    选择性覆盖 username / (password + password_hash)。device_id
        //    永远来自 minter(用户不能自定义 device_id, 它是稳定内部 id)。
        let MintedCredentials {
            username: minted_username,
            password: minted_password,
            password_hash: minted_hash,
            device_id,
        } = self.credentials_minter.mint_credentials();

        // 自定义 username:0.1 已校验形态(对 trim 后值);此处只做唯一性
        // 检查。
        let username = match custom_username {
            Some(u) => {
                self.ensure_username_available(&u).await?;
                u
            }
            None => minted_username,
        };

        // 自定义 password:hash 自定义明文;否则沿用 minter 的 (password,
        // password_hash) 同源对。
        let (password, password_hash) = match input.password {
            Some(p) => {
                // 长度在 0.1 已校验。
                let hash = self
                    .password_hasher
                    .hash(&p)
                    .await
                    .map_err(translate_hasher_error)?;
                (p, hash)
            }
            None => (minted_password, minted_hash),
        };

        // 3. 构造并持久化 MobileDevice。
        let now_ms = self.clock.now_ms();
        let device = MobileDevice {
            device_id: device_id.clone(),
            label: label.clone(),
            client_type: MobileClientType::IosShortcut,
            username: username.clone(),
            password_hash,
            created_at_ms: now_ms,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        };
        self.save
            .save(&device)
            .await
            .map_err(translate_device_error)?;

        // schema doc §7.6 / §12.2 P1：registration anchor 落在 save 之后。
        // 后续 QR 渲染失败仍保留事件——doc 模块注释已说明该路径下会留下
        // "已登记但用户拿不到 install URL"的孤儿记录，但设备 IS registered,
        // telemetry 反映这一事实，与 UI 报错语义不冲突。
        self.analytics.capture(Event::MobileDeviceRegistered);

        // 4. 组装 connect URI 与 二维码。规范 §3.2 白名单 `o` 字段:
        //    - label: 设备显示名, 让客户端 UI 能复用
        //    - did:   服务端 device_id, 用于日志关联
        //    - proto: 协议族提示, v1 固定 "syncclipboard"
        //    - install: 暂留空, 阶段 4 决定是否下沉 iCloud 链接到 payload 里
        //
        //    install_url 不再用作 QR 内容, 改为前端二级"首次安装"卡片显示。
        let other = ConnectUriOther {
            label: Some(label.clone()),
            did: Some(device_id.as_str().to_string()),
            proto: Some("syncclipboard".to_string()),
            install: None,
        };
        let connect_uri =
            build_mobile_sync_connect_uri(&advertise_urls, &username, &password, other)
                .map_err(translate_connect_uri_error)?;

        let install_url = SYNC_CLIPBOARD_EX_INSTALL_URL.to_string();
        let (qr_code_png_bytes, qr_code_ascii) = render_qr_code(&connect_uri)?;
        // install_url 的 QR 走同一条 render_qr_code 流水线, 与 connect URI
        // QR 渲染管线对称(防止前端/CLI 出现"两张 QR 看起来不一样"的视觉
        // 不一致)。ASCII 不渲染 —— CLI 用例不展示 install QR(只展示 URL 文本)。
        let (install_qr_code_png_bytes, _install_qr_ascii) = render_qr_code(&install_url)?;

        Ok(RegisterMobileShortcutDeviceOutput {
            device,
            base_url,
            username,
            password,
            install_url,
            install_qr_code_png_bytes,
            connect_uri,
            qr_code_png_bytes,
            qr_code_ascii,
        })
    }

    /// 自定义 username 的唯一性检查 —— 撞上现有设备直接拒绝, UI / CLI
    /// 提示用户换一个。
    async fn ensure_username_available(
        &self,
        username: &str,
    ) -> Result<(), RegisterMobileShortcutDeviceError> {
        match self.find_by_username.find_by_username(username).await {
            Ok(Some(_)) => Err(RegisterMobileShortcutDeviceError::UsernameTaken(
                username.to_string(),
            )),
            Ok(None) => Ok(()),
            Err(err) => Err(translate_device_error(err)),
        }
    }
}

// ─── helpers ────────────────────────────────────────────────────────────

/// Validate and normalize a device label: trim surrounding whitespace, reject
/// empty as [`RegisterMobileShortcutDeviceError::LabelEmpty`] and anything
/// longer than [`MAX_LABEL_LEN`] chars as
/// [`RegisterMobileShortcutDeviceError::LabelTooLong`], otherwise return the
/// trimmed value. Single source of truth shared with `update_device`.
pub(super) fn validate_label(label: String) -> Result<String, RegisterMobileShortcutDeviceError> {
    let label = label.trim().to_string();
    if label.is_empty() {
        return Err(RegisterMobileShortcutDeviceError::LabelEmpty);
    }
    if label.chars().count() > MAX_LABEL_LEN {
        return Err(RegisterMobileShortcutDeviceError::LabelTooLong);
    }
    Ok(label)
}

/// 校验自定义 username 形态:
/// - 长度 [`MIN_USERNAME_LEN`]–[`MAX_USERNAME_LEN`]
/// - 必须以 ASCII 字母开头(避免 Basic Auth header 解析歧义)
/// - 只允许 `[A-Za-z0-9_]`
pub(super) fn validate_username_shape(
    username: &str,
) -> Result<(), RegisterMobileShortcutDeviceError> {
    let len = username.chars().count();
    if len < MIN_USERNAME_LEN {
        return Err(RegisterMobileShortcutDeviceError::UsernameTooShort {
            min: MIN_USERNAME_LEN,
            got: len,
        });
    }
    if len > MAX_USERNAME_LEN {
        return Err(RegisterMobileShortcutDeviceError::UsernameTooLong {
            max: MAX_USERNAME_LEN,
            got: len,
        });
    }
    let mut chars = username.chars();
    let first = chars.next().expect("len ≥ MIN_USERNAME_LEN > 0");
    if !first.is_ascii_alphabetic() {
        return Err(RegisterMobileShortcutDeviceError::UsernameMustStartWithLetter);
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(RegisterMobileShortcutDeviceError::UsernameContainsForbiddenChars);
    }
    Ok(())
}

/// 校验自定义 password 长度。**不**校验复杂度(用户选"宽松")。
pub(super) fn validate_password_length(
    password: &str,
) -> Result<(), RegisterMobileShortcutDeviceError> {
    let len = password.chars().count();
    if len < MIN_PASSWORD_LEN {
        return Err(RegisterMobileShortcutDeviceError::PasswordTooShort {
            min: MIN_PASSWORD_LEN,
        });
    }
    if len > MAX_PASSWORD_LEN {
        return Err(RegisterMobileShortcutDeviceError::PasswordTooLong {
            max: MAX_PASSWORD_LEN,
        });
    }
    Ok(())
}

/// 排序桶（规格 §5.3）:10/8 = 0,172.16/12 = 1,192.168/16 = 2,
/// 100.64/10（Tailscale CGNAT）= 3,其它 = 4(经 `may_advertise_interface`
/// 过滤后理论上不存在)。与 `list_lan_interfaces` 的下拉排序口径一致。
fn advertise_bucket(octets: &[u8; 4]) -> u8 {
    match octets {
        [10, _, _, _] => 0,
        [172, 16..=31, _, _] => 1,
        [192, 168, _, _] => 2,
        [100, b, _, _] if (b & 0xc0) == 0x40 => 3,
        _ => 4,
    }
}

fn translate_probe_error(err: LanInterfaceProbeError) -> RegisterMobileShortcutDeviceError {
    match err {
        LanInterfaceProbeError::Probe(msg) => {
            RegisterMobileShortcutDeviceError::LanInterfaceProbeFailed(msg)
        }
    }
}

/// 把任意 URI / URL 文本渲染为 PNG + ASCII 二维码。
///
/// PNG: `qrcode::QrCode::render::<Luma<u8>>` 出 `image::ImageBuffer` →
/// 写到 PNG cursor。ASCII: 调 `render::<unicode::Dense1x2>` 用 1×2 块
/// 字符渲染,适合 80 列终端。
///
/// 调用方在 v1 路径下传入 `connect_uri`(规范 §2); 旧 `install_url` 已降级
/// 为前端二级"首次安装"卡片显示, 不再走 QR。
fn render_qr_code(content: &str) -> Result<(Vec<u8>, String), RegisterMobileShortcutDeviceError> {
    use image::{ImageFormat, Luma};
    use qrcode::render::unicode::Dense1x2;
    use qrcode::QrCode;

    let code = QrCode::new(content.as_bytes())
        .map_err(|e| RegisterMobileShortcutDeviceError::QrRenderFailed(e.to_string()))?;

    let png_image = code.render::<Luma<u8>>().min_dimensions(256, 256).build();
    let mut png_bytes: Vec<u8> = Vec::new();
    png_image
        .write_to(&mut std::io::Cursor::new(&mut png_bytes), ImageFormat::Png)
        .map_err(|e| RegisterMobileShortcutDeviceError::QrRenderFailed(e.to_string()))?;

    let ascii = code
        .render::<Dense1x2>()
        .dark_color(Dense1x2::Light)
        .light_color(Dense1x2::Dark)
        .build();

    Ok((png_bytes, ascii))
}

fn translate_device_error(err: MobileDeviceError) -> RegisterMobileShortcutDeviceError {
    match err {
        MobileDeviceError::AlreadyExists(id) => {
            // device_id 由 minter 一次性生成,碰撞理论上不可能;走到这里
            // 说明 minter 实现有缺陷 —— 提示运维 + 翻译为 persistence 错误。
            warn!(
                ?id,
                "minter produced colliding device id; this should not happen"
            );
            RegisterMobileShortcutDeviceError::PersistenceFailed(
                "device id collision (minter contract violated)".to_string(),
            )
        }
        MobileDeviceError::UsernameCollision => {
            // 自动模式下 minter 8 hex 碰撞概率极低;custom 模式下我们已
            // 在 save 之前 check 过 find_by_username,这里只可能是 race
            // (并发 register)—— 翻译为 UsernameTaken 让 UI 提示用户换名。
            info!("username collision at save time (likely concurrent register race)");
            RegisterMobileShortcutDeviceError::UsernameTaken(
                "username taken at save time (concurrent registration)".to_string(),
            )
        }
        MobileDeviceError::Storage(msg) => {
            RegisterMobileShortcutDeviceError::PersistenceFailed(msg)
        }
    }
}

/// 把 connect URI 编码失败翻译为 use case 层错误。
///
/// 设计上 build 路径仅可能撞到 [`ConnectUriError::UriTooLong`] —— label
/// 过长导致 payload 超 800 字符。其余 6 个变体属于"上游契约保证不会发生":
/// - `MissingField`: `base_url` 要么由 `format!("http://{ip}:{port}")` 拼出,
///   要么是 update_settings 已校验非空的 `lan_advertise_base_url`;
///   `username` / `password` 走 minter 或 0.1 步前置校验, 都非空。
/// - `InvalidUrl`: base_url 永远以 `http://` 或 `https://` 开头
///   (LAN 形态硬编码 `http://`,override 形态已在 update_settings 校验)。
/// - `InvalidScheme` / `UnsupportedVersion` / `UnsupportedService` /
///   `PayloadDecodeFailed`: 仅 parse 路径出现, build 不调 parse。
///
/// 一旦走到这些"理论上不可能"的变体, 说明 minter 或上游契约破坏 ——
/// 仍翻译为 `QrRenderFailed` 让 UI 给用户可见的失败 + 日志保留原因, 而
/// 不是 panic 把整个进程拖垮。
fn translate_connect_uri_error(err: ConnectUriError) -> RegisterMobileShortcutDeviceError {
    match err {
        ConnectUriError::UriTooLong { len, max } => {
            RegisterMobileShortcutDeviceError::QrRenderFailed(format!(
                "connect uri too long ({len} chars, max {max}); shorten device label"
            ))
        }
        other => RegisterMobileShortcutDeviceError::QrRenderFailed(format!(
            "connect uri build failed (unexpected): {other}"
        )),
    }
}

fn translate_hasher_error(err: PasswordHasherError) -> RegisterMobileShortcutDeviceError {
    match err {
        PasswordHasherError::InvalidPhc(msg) => {
            // hash() 不应产生 InvalidPhc(那是 verify 路径才会有), 但 trait
            // 把两个变体合并; 走到这里说明 adapter 实现异常, 翻译为内部错误。
            RegisterMobileShortcutDeviceError::PasswordHashFailed(format!("invalid phc: {msg}"))
        }
        PasswordHasherError::Internal(msg) => {
            RegisterMobileShortcutDeviceError::PasswordHashFailed(msg)
        }
    }
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;

    use uc_core::mobile_sync::MobileDeviceId;
    use uc_core::settings::model::Settings;

    // 多个 use case 测试共用的 mock(DeviceRepo / Hasher / Minter)+
    // CapturingAnalyticsSink 集中在 test_support;register 独占的
    // SettingsPort / Clock / Probe 仍就近 mockall::mock! 定义。
    use super::super::connect_uri::parse_mobile_sync_connect_uri;
    use super::super::test_support::{
        CapturingAnalyticsSink, MockDeviceRepo, MockHasher, MockMinter,
    };

    mockall::mock! {
        SettingsPortImpl {}
        #[async_trait]
        impl SettingsPort for SettingsPortImpl {
            async fn load(&self) -> anyhow::Result<Settings>;
            async fn save(&self, settings: &Settings) -> anyhow::Result<()>;
        }
    }

    mockall::mock! {
        ClockImpl {}
        impl ClockPort for ClockImpl {
            fn now_ms(&self) -> i64;
        }
    }

    mockall::mock! {
        Probe {}
        #[async_trait]
        impl LanInterfaceProbePort for Probe {
            async fn list_interfaces(&self) -> Result<Vec<LanInterface>, LanInterfaceProbeError>;
        }
    }

    // ── helpers ────────────────────────────────────────────────────────

    /// minter 永远返回的 fixed MintedCredentials —— 测试断言里写的字面值。
    const MINTER_USERNAME: &str = "mobile_aabbccdd";
    const MINTER_PASSWORD: &str = "deterministic-password-22";
    const MINTER_PHC: &str = "$argon2id$v=19$m=64,t=1,p=1$AAAAAAAAAAAAAAAA$test";
    const MINTER_DEVICE_ID: &str = "did_aaaa";

    fn deterministic_minter() -> MockMinter {
        let mut m = MockMinter::new();
        m.expect_mint_credentials().returning(|| MintedCredentials {
            username: MINTER_USERNAME.into(),
            password: MINTER_PASSWORD.into(),
            password_hash: MINTER_PHC.into(),
            device_id: MobileDeviceId::new(MINTER_DEVICE_ID),
        });
        m
    }

    /// 把每次 hash 调用产出一个 `phc-of:<plain>` 形态的 PHC,便于断言 use
    /// case 真去 hash 了自定义 password(而不是回退用 minter 的 phc)。
    /// mockall 的 expect_hash() 没有 .times() 约束默认任意次数;调用方在
    /// 需要时显式加 .times(N) 即可。
    fn recording_hasher() -> MockHasher {
        let mut h = MockHasher::new();
        h.expect_hash().returning(|p| Ok(format!("phc-of:{p}")));
        h
    }

    fn failing_hasher() -> MockHasher {
        let mut h = MockHasher::new();
        h.expect_hash().returning(|_| {
            Err(PasswordHasherError::Internal(
                "simulated hash failure".into(),
            ))
        });
        h
    }

    /// 默认 device_repo:save 永远 OK,find_by_username 永远找不到。供大多数
    /// happy-path / 校验失败测试使用,测试想"定制 username 已占用"时单独构造。
    fn empty_device_repo() -> MockDeviceRepo {
        let mut r = MockDeviceRepo::new();
        r.expect_save().returning(|_| Ok(()));
        r.expect_find_by_username().returning(|_| Ok(None));
        r
    }

    /// 把指定 username 设为"已被占用",触发 `UsernameTaken` 校验路径。
    fn device_repo_with_existing_username(name: &'static str) -> MockDeviceRepo {
        let mut r = MockDeviceRepo::new();
        r.expect_save().returning(|_| Ok(()));
        r.expect_find_by_username().returning(move |u| {
            if u == name {
                Ok(Some(MobileDevice {
                    device_id: MobileDeviceId::new("did_existing"),
                    label: "existing".into(),
                    client_type: MobileClientType::IosShortcut,
                    username: u.to_string(),
                    password_hash: "phc:existing".into(),
                    created_at_ms: 0,
                    last_seen_at_ms: None,
                    last_seen_ip: None,
                    reported_name: None,
                    reported_os: None,
                }))
            } else {
                Ok(None)
            }
        });
        r
    }

    /// `lan_advertise_ip = Some("192.168.1.5") + lan_port = 42720`,加可控的
    /// `lan_listen_enabled` flag。base_url 推为 `http://192.168.1.5:42720`。
    fn settings_port_lan_advertise(lan_listen_enabled: bool) -> MockSettingsPortImpl {
        let mut s = MockSettingsPortImpl::new();
        s.expect_load().returning(move || {
            let mut settings = Settings::default();
            settings.mobile_sync.enabled = lan_listen_enabled;
            settings.mobile_sync.lan_listen_enabled = lan_listen_enabled;
            settings.mobile_sync.lan_advertise_ip = Some("192.168.1.5".into());
            settings.mobile_sync.lan_port = Some(42720);
            Ok(settings)
        });
        s
    }

    /// `lan_advertise_ip = None` —— 触发 use case 自己 auto-pick 的路径。
    fn settings_port_auto() -> MockSettingsPortImpl {
        let mut s = MockSettingsPortImpl::new();
        s.expect_load().returning(|| {
            let mut settings = Settings::default();
            settings.mobile_sync.enabled = true;
            settings.mobile_sync.lan_listen_enabled = true;
            settings.mobile_sync.lan_advertise_ip = None;
            settings.mobile_sync.lan_port = Some(42720);
            Ok(settings)
        });
        s
    }

    /// `lan_advertise_base_url = Some("https://clip.example.com")` 同时设了一个
    /// `lan_advertise_ip` + `lan_port` —— 用来证明 base_url override 优先。
    fn settings_port_base_url() -> MockSettingsPortImpl {
        let mut s = MockSettingsPortImpl::new();
        s.expect_load().returning(|| {
            let mut settings = Settings::default();
            settings.mobile_sync.enabled = true;
            settings.mobile_sync.lan_listen_enabled = true;
            settings.mobile_sync.lan_advertise_ip = Some("192.168.1.5".into());
            settings.mobile_sync.lan_advertise_base_url = Some("https://clip.example.com".into());
            settings.mobile_sync.lan_port = Some(42720);
            Ok(settings)
        });
        s
    }

    fn clock_at(ms: i64) -> MockClockImpl {
        let mut c = MockClockImpl::new();
        c.expect_now_ms().returning(move || ms);
        c
    }

    fn probe_returning(ifaces: Vec<LanInterface>) -> MockProbe {
        let mut p = MockProbe::new();
        p.expect_list_interfaces()
            .returning(move || Ok(ifaces.clone()));
        p
    }

    fn probe_failing() -> MockProbe {
        let mut p = MockProbe::new();
        p.expect_list_interfaces()
            .returning(|| Err(LanInterfaceProbeError::Probe("ifaddr crashed".into())));
        p
    }

    fn iface(name: &str, ip: [u8; 4]) -> LanInterface {
        LanInterface {
            name: name.into(),
            ipv4: std::net::Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3]),
            is_loopback: false,
        }
    }

    /// 标准 happy-path 装配:固定 minter / recording hasher / empty repo /
    /// 默认 LAN advertise settings / 1_000ms clock / 空 probe(LAN 已配置时
    /// 走不到 auto-pick,empty probe 即可)。`lan_listen_enabled` 控开关。
    fn build_uc(lan_listen_enabled: bool) -> RegisterMobileShortcutDeviceUseCase {
        let device_repo = Arc::new(empty_device_repo());
        RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(deterministic_minter()),
            Arc::new(recording_hasher()),
            device_repo.clone(), // find_by_username
            device_repo,         // save
            Arc::new(settings_port_lan_advertise(lan_listen_enabled)),
            Arc::new(clock_at(1_000)),
            Arc::new(probe_returning(vec![])),
            Arc::new(CapturingAnalyticsSink::default()),
        )
    }

    /// build_uc 的 capture-asserting 版本：返回 use case + sink，调用方可在
    /// happy path 上断言 emit，或在失败路径上断言"未 emit"。
    fn build_uc_with_sink(
        lan_listen_enabled: bool,
    ) -> (
        RegisterMobileShortcutDeviceUseCase,
        Arc<CapturingAnalyticsSink>,
    ) {
        let analytics = Arc::new(CapturingAnalyticsSink::default());
        let device_repo = Arc::new(empty_device_repo());
        let uc = RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(deterministic_minter()),
            Arc::new(recording_hasher()),
            device_repo.clone(), // find_by_username
            device_repo,         // save
            Arc::new(settings_port_lan_advertise(lan_listen_enabled)),
            Arc::new(clock_at(1_000)),
            Arc::new(probe_returning(vec![])),
            analytics.clone(),
        );
        (uc, analytics)
    }

    fn label_only(label: &str) -> RegisterMobileShortcutDeviceInput {
        RegisterMobileShortcutDeviceInput {
            label: label.into(),
            ..Default::default()
        }
    }

    // ── tests: label / lan listener (existing happy path) ──────────────

    #[tokio::test]
    async fn rejects_empty_label() {
        let uc = build_uc(true);
        let err = uc.execute(label_only("   ")).await.unwrap_err();
        assert!(matches!(err, RegisterMobileShortcutDeviceError::LabelEmpty));
    }

    #[tokio::test]
    async fn rejects_overlong_label() {
        let uc = build_uc(true);
        let err = uc
            .execute(label_only(&"x".repeat(MAX_LABEL_LEN + 1)))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::LabelTooLong
        ));
    }

    #[tokio::test]
    async fn rejects_when_lan_listener_disabled() {
        let uc = build_uc(false);
        let err = uc.execute(label_only("我的 iPhone")).await.unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::LanListenerDisabled
        ));
    }

    #[tokio::test]
    async fn auto_path_returns_minter_credentials_and_install_url() {
        let uc = build_uc(true);
        let out = uc
            .execute(label_only("我的 iPhone"))
            .await
            .expect("happy path must succeed");

        // 设备元信息
        assert_eq!(out.device.label, "我的 iPhone");
        assert_eq!(out.device.client_type, MobileClientType::IosShortcut);
        assert_eq!(out.device.created_at_ms, 1_000);
        assert_eq!(out.device.username, "mobile_aabbccdd");

        // 一次性回显的凭据(全自动 → 来自 minter)
        assert_eq!(out.username, "mobile_aabbccdd");
        assert_eq!(out.password, "deterministic-password-22");
        assert_eq!(out.base_url, "http://192.168.1.5:42720");
        // install_url 保留为"首次安装快捷指令"次要入口, 仍是常量。
        assert_eq!(out.install_url, SYNC_CLIPBOARD_EX_INSTALL_URL);

        // connect_uri 是 QR 主内容: scheme/host/envelope 字面值固定; payload
        // 解码后能还原出三栏 + label/did/proto 白名单字段。
        assert!(
            out.connect_uri
                .starts_with("uniclipboard://connect?v=1&svc=mobile-sync&p="),
            "unexpected prefix: {}",
            out.connect_uri
        );
        let payload = parse_mobile_sync_connect_uri(&out.connect_uri)
            .expect("connect URI must round-trip parse");
        assert_eq!(payload.url, "http://192.168.1.5:42720");
        assert_eq!(payload.user, "mobile_aabbccdd");
        assert_eq!(payload.pwd, "deterministic-password-22");
        assert_eq!(
            payload.o.get("label").map(String::as_str),
            Some("我的 iPhone")
        );
        assert_eq!(
            payload.o.get("did").map(String::as_str),
            Some(MINTER_DEVICE_ID)
        );
        assert_eq!(
            payload.o.get("proto").map(String::as_str),
            Some("syncclipboard")
        );
        // install 字段 v1 留空(规范 §3.2), 阶段 4 决定。
        assert!(payload.o.get("install").is_none());

        // 二维码必须非空,且 PNG 字节有 magic header `\x89PNG`。QR 渲染对象
        // 已从 install_url 切换到 connect_uri, 故 PNG 字节也会随凭据变化。
        assert!(out.qr_code_png_bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        assert!(!out.qr_code_ascii.is_empty());
        // install QR 是 install_url 的二维码(常量内容), 前端用于"安装快捷
        // 指令"次要 tab 让 iPhone 扫一下直接装。PNG magic 与 connect URI QR
        // 一致, 但字节不同 — 那是 qr_content_follows_connect_uri_not_install_url
        // 的回归保护范围。
        assert!(out
            .install_qr_code_png_bytes
            .starts_with(&[0x89, 0x50, 0x4E, 0x47]));
    }

    #[tokio::test]
    async fn qr_content_follows_connect_uri_not_install_url() {
        // 回归保护: 阶段 2 之前 QR 渲染的是常量 install_url, 不论凭据如何
        // 字节都不变。切换后每次 register 都会随 username/password/device_id
        // 变化产生不同 connect_uri, PNG 字节不再固定。本测试保证后续 PR
        // 不会误把 QR 退回去渲染 install_url(那是 LSP / find-refs 不能直接
        // 防住的语义回归)。
        //
        // 阶段 5 起 install QR 单独输出 `install_qr_code_png_bytes`,
        // 同时断言它**等于** install_url 编码 —— 防止字段串位 / 后端误把
        // 两个 QR 张冠李戴(命名相近, 类型相同, 容易复制粘贴出错)。
        //
        // 做法: build 出 install_url 的 QR(单独走一次 render_qr_code),
        // 断言 connect QR 字节与之不同, install QR 字节与之相同。
        let uc = build_uc(true);
        let out = uc
            .execute(label_only("Phone"))
            .await
            .expect("happy path must succeed");

        let install_url_qr = render_qr_code(SYNC_CLIPBOARD_EX_INSTALL_URL).expect("baseline qr ok");
        assert_ne!(
            out.qr_code_png_bytes, install_url_qr.0,
            "main QR PNG must encode connect_uri, not install_url"
        );
        assert_ne!(
            out.qr_code_ascii, install_url_qr.1,
            "main QR ASCII must encode connect_uri, not install_url"
        );
        assert_eq!(
            out.install_qr_code_png_bytes, install_url_qr.0,
            "install QR PNG must encode install_url byte-for-byte"
        );
    }

    #[tokio::test]
    async fn base_url_override_takes_precedence_over_advertise_ip() {
        // settings 同时有 lan_advertise_ip 和 lan_advertise_base_url —— base_url
        // 必须胜出, 出现在 out.base_url 与 connect_uri payload 里。
        let device_repo = Arc::new(empty_device_repo());
        let uc = RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(deterministic_minter()),
            Arc::new(recording_hasher()),
            device_repo.clone(), // find_by_username
            device_repo,         // save
            Arc::new(settings_port_base_url()),
            Arc::new(clock_at(1_000)),
            // probe 永远不该被调到(base_url 已定 → 不走 auto-pick)。给个空
            // probe, 若实现误调它只会得到 NoLanInterfaceAvailable 而 panic 断言。
            Arc::new(probe_returning(vec![])),
            Arc::new(CapturingAnalyticsSink::default()),
        );
        let out = uc
            .execute(label_only("我的 iPhone"))
            .await
            .expect("base_url override path must succeed");

        assert_eq!(out.base_url, "https://clip.example.com");
        let payload = parse_mobile_sync_connect_uri(&out.connect_uri)
            .expect("connect URI must round-trip parse");
        assert_eq!(payload.url, "https://clip.example.com");
    }

    // ── tests: custom username ─────────────────────────────────────────

    #[tokio::test]
    async fn accepts_custom_username() {
        let uc = build_uc(true);
        let out = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "iPhone".into(),
                username: Some("alice_001".into()),
                password: None,
            })
            .await
            .expect("custom username should pass");
        assert_eq!(out.username, "alice_001");
        assert_eq!(out.device.username, "alice_001");
        // password 走 minter,所以仍是 deterministic 那串。
        assert_eq!(out.password, "deterministic-password-22");
    }

    #[tokio::test]
    async fn trims_custom_username_before_validation() {
        let uc = build_uc(true);
        let out = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "iPhone".into(),
                username: Some("  alice_42  ".into()),
                password: None,
            })
            .await
            .expect("trim ok");
        assert_eq!(out.username, "alice_42");
    }

    #[tokio::test]
    async fn rejects_username_too_short() {
        let uc = build_uc(true);
        let err = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "iPhone".into(),
                username: Some("ali".into()), // 3 chars < MIN(6)
                password: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::UsernameTooShort { min: 6, got: 3 }
        ));
    }

    #[tokio::test]
    async fn rejects_username_too_long() {
        let uc = build_uc(true);
        let err = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "iPhone".into(),
                username: Some("a".repeat(MAX_USERNAME_LEN + 1)),
                password: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::UsernameTooLong { max: 32, got: 33 }
        ));
    }

    #[tokio::test]
    async fn rejects_username_starting_with_digit() {
        let uc = build_uc(true);
        let err = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "iPhone".into(),
                username: Some("1alice0".into()),
                password: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::UsernameMustStartWithLetter
        ));
    }

    #[tokio::test]
    async fn rejects_username_with_invalid_chars() {
        let uc = build_uc(true);
        let err = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "iPhone".into(),
                username: Some("alice-bob".into()), // hyphen not in [A-Za-z0-9_]
                password: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::UsernameContainsForbiddenChars
        ));
    }

    #[tokio::test]
    async fn rejects_username_already_taken() {
        let device_repo = Arc::new(device_repo_with_existing_username("alice_001"));
        let uc = RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(deterministic_minter()),
            Arc::new(recording_hasher()),
            device_repo.clone(), // find_by_username
            device_repo,         // save
            Arc::new(settings_port_lan_advertise(true)),
            Arc::new(clock_at(1_000)),
            Arc::new(probe_returning(vec![])),
            Arc::new(CapturingAnalyticsSink::default()),
        );
        let err = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "iPhone".into(),
                username: Some("alice_001".into()),
                password: None,
            })
            .await
            .unwrap_err();
        match err {
            RegisterMobileShortcutDeviceError::UsernameTaken(u) => assert_eq!(u, "alice_001"),
            other => panic!("expected UsernameTaken, got {other:?}"),
        }
    }

    // ── tests: custom password ─────────────────────────────────────────

    #[tokio::test]
    async fn accepts_custom_password() {
        // 通过 mockall expectation 直接断言 hasher 收到自定义明文 + 被调用
        // 恰好一次 —— 比手写 RecordingHasher 自己累积的 Vec 更精确。
        let mut hasher = MockHasher::new();
        hasher
            .expect_hash()
            .with(mockall::predicate::eq("correct horse battery staple"))
            .times(1)
            .returning(|p| Ok(format!("phc-of:{p}")));

        let device_repo = Arc::new(empty_device_repo());
        let uc = RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(deterministic_minter()),
            Arc::new(hasher),
            device_repo.clone(), // find_by_username
            device_repo,         // save
            Arc::new(settings_port_lan_advertise(true)),
            Arc::new(clock_at(1_000)),
            Arc::new(probe_returning(vec![])),
            Arc::new(CapturingAnalyticsSink::default()),
        );
        let out = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "iPhone".into(),
                username: None,
                password: Some("correct horse battery staple".into()),
            })
            .await
            .expect("custom password should pass");
        // password 字段仍是用户原值(一次性回显);phc 走 hasher。
        assert_eq!(out.password, "correct horse battery staple");
        assert_eq!(
            out.device.password_hash,
            "phc-of:correct horse battery staple"
        );
        // username 走 minter
        assert_eq!(out.username, MINTER_USERNAME);
        // hasher 调用次数由 mockall 在 drop 时自动 verify (.times(1) 上面已断言)
    }

    #[tokio::test]
    async fn rejects_password_too_short() {
        let uc = build_uc(true);
        let err = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "iPhone".into(),
                username: None,
                password: Some("a".repeat(MIN_PASSWORD_LEN - 1)),
            })
            .await
            .unwrap_err();
        match err {
            RegisterMobileShortcutDeviceError::PasswordTooShort { min } => {
                assert_eq!(min, MIN_PASSWORD_LEN)
            }
            other => panic!("expected PasswordTooShort, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rejects_password_too_long() {
        let uc = build_uc(true);
        let err = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "iPhone".into(),
                username: None,
                password: Some("a".repeat(MAX_PASSWORD_LEN + 1)),
            })
            .await
            .unwrap_err();
        match err {
            RegisterMobileShortcutDeviceError::PasswordTooLong { max } => {
                assert_eq!(max, MAX_PASSWORD_LEN)
            }
            other => panic!("expected PasswordTooLong, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn translates_hasher_internal_error() {
        let device_repo = Arc::new(empty_device_repo());
        let uc = RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(deterministic_minter()),
            Arc::new(failing_hasher()),
            device_repo.clone(), // find_by_username
            device_repo,         // save
            Arc::new(settings_port_lan_advertise(true)),
            Arc::new(clock_at(1_000)),
            Arc::new(probe_returning(vec![])),
            Arc::new(CapturingAnalyticsSink::default()),
        );
        let err = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "iPhone".into(),
                username: None,
                password: Some("a-strong-password".into()),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::PasswordHashFailed(_)
        ));
    }

    // ── tests: both custom (mixed) ────────────────────────────────────

    #[tokio::test]
    async fn accepts_both_custom_username_and_password() {
        let uc = build_uc(true);
        let out = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "iPhone".into(),
                username: Some("alice_pro".into()),
                password: Some("a-strong-password".into()),
            })
            .await
            .expect("both custom should pass");
        assert_eq!(out.username, "alice_pro");
        assert_eq!(out.password, "a-strong-password");
        assert_eq!(out.device.username, "alice_pro");
        assert_eq!(out.device.password_hash, "phc-of:a-strong-password");
        // device_id 永远来自 minter。
        assert_eq!(out.device.device_id.as_str(), MINTER_DEVICE_ID);
    }

    // ── tests: auto-pick advertise_ip ─────────────────────────────────

    fn build_uc_auto(probe: MockProbe) -> RegisterMobileShortcutDeviceUseCase {
        let device_repo = Arc::new(empty_device_repo());
        RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(deterministic_minter()),
            Arc::new(recording_hasher()),
            device_repo.clone(), // find_by_username
            device_repo,         // save
            Arc::new(settings_port_auto()),
            Arc::new(clock_at(1_000)),
            Arc::new(probe),
            Arc::new(CapturingAnalyticsSink::default()),
        )
    }

    #[tokio::test]
    async fn auto_picks_first_rfc1918_when_advertise_ip_unset() {
        // 故意打乱顺序,断言走"10/8 → 172.16/12 → 192.168/16,段内字典序"。
        // 期望挑 10.0.0.5(10.x 段最小)。
        let probe = probe_returning(vec![
            iface("en1", [192, 168, 1, 5]),
            iface("en2", [10, 0, 0, 5]),
            iface("en3", [172, 16, 0, 5]),
            iface("en4", [10, 1, 1, 1]),
        ]);
        let uc = build_uc_auto(probe);
        let out = uc.execute(label_only("iPhone")).await.expect("ok");
        assert_eq!(out.base_url, "http://10.0.0.5:42720");
    }

    #[tokio::test]
    async fn auto_accepts_cgnat_when_no_rfc1918_available() {
        // 多候选口径(规格 §5.2)纳入 Tailscale CGNAT:loopback / 公网 /
        // 链路本地仍被剔除,但 100.64/10 现在是合法候选 —— 只剩它时
        // 不再报 NoLanInterfaceAvailable,而是用它当 base_url。
        let probe = probe_returning(vec![
            LanInterface {
                name: "lo0".into(),
                ipv4: std::net::Ipv4Addr::new(127, 0, 0, 1),
                is_loopback: true,
            },
            iface("en_pub", [8, 8, 8, 8]),
            iface("en_cgnat", [100, 64, 1, 5]),
            iface("en_link", [169, 254, 1, 5]),
        ]);
        let uc = build_uc_auto(probe);
        let out = uc.execute(label_only("iPhone")).await.expect("ok");
        assert_eq!(out.base_url, "http://100.64.1.5:42720");
    }

    #[tokio::test]
    async fn auto_fails_when_no_candidate_at_all() {
        // 全是被剔除的接口 → 退化成"没有可用 LAN" → NoLanInterfaceAvailable。
        let probe = probe_returning(vec![
            LanInterface {
                name: "lo0".into(),
                ipv4: std::net::Ipv4Addr::new(127, 0, 0, 1),
                is_loopback: true,
            },
            iface("en_pub", [8, 8, 8, 8]),
            iface("en_link", [169, 254, 1, 5]),
        ]);
        let uc = build_uc_auto(probe);
        let err = uc.execute(label_only("iPhone")).await.unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::NoLanInterfaceAvailable
        ));
    }

    #[tokio::test]
    async fn auto_translates_probe_failure() {
        let uc = build_uc_auto(probe_failing());
        let err = uc.execute(label_only("iPhone")).await.unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::LanInterfaceProbeFailed(ref s) if s.contains("ifaddr crashed")
        ));
    }

    // ── tests: 多候选 urls(docs/planning/mobile-sync-qr-multi-url.md §9) ──────

    /// 任意 settings + probe 组合的装配,多候选测试专用。
    fn build_uc_with(
        settings: MockSettingsPortImpl,
        probe: MockProbe,
    ) -> RegisterMobileShortcutDeviceUseCase {
        let device_repo = Arc::new(empty_device_repo());
        RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(deterministic_minter()),
            Arc::new(recording_hasher()),
            device_repo.clone(), // find_by_username
            device_repo,         // save
            Arc::new(settings),
            Arc::new(clock_at(1_000)),
            Arc::new(probe),
            Arc::new(CapturingAnalyticsSink::default()),
        )
    }

    /// 从 execute 输出的 connect_uri 反解出 payload 的 urls 字段。
    fn parsed_urls(out: &RegisterMobileShortcutDeviceOutput) -> (String, Vec<String>) {
        let payload = parse_mobile_sync_connect_uri(&out.connect_uri).expect("parse ok");
        (payload.url, payload.urls)
    }

    #[tokio::test]
    async fn urls_carry_all_nics_in_bucket_order_and_url_is_first() {
        // 故意打乱顺序;期望桶序 10/8 → 172.16/12 → 192.168/16 → 100.64/10,
        // 且 payload.url == urls[0] == output.base_url。
        let probe = probe_returning(vec![
            iface("en1", [192, 168, 1, 5]),
            iface("utun3", [100, 64, 0, 5]),
            iface("en2", [10, 0, 0, 5]),
            iface("en3", [172, 16, 0, 5]),
        ]);
        let uc = build_uc_with(settings_port_auto(), probe);
        let out = uc.execute(label_only("iPhone")).await.expect("ok");
        let (url, urls) = parsed_urls(&out);
        assert_eq!(
            urls,
            vec![
                "http://10.0.0.5:42720",
                "http://172.16.0.5:42720",
                "http://192.168.1.5:42720",
                "http://100.64.0.5:42720",
            ]
        );
        assert_eq!(url, urls[0]);
        assert_eq!(out.base_url, urls[0]);
    }

    #[tokio::test]
    async fn urls_exclude_docker_virtual_interfaces() {
        // docker0 / veth* 按名剔除;br-* 仅在 172.16/12 段内剔除 ——
        // br- 命名的真实 192.168 网桥必须保留(规格 §10 风险收敛)。
        let probe = probe_returning(vec![
            iface("docker0", [172, 17, 0, 1]),
            iface("br-12af3c9d", [172, 18, 0, 1]),
            iface("veth1a2b", [10, 99, 0, 1]),
            iface("en0", [192, 168, 1, 5]),
            iface("br-lan", [192, 168, 2, 1]),
        ]);
        let uc = build_uc_with(settings_port_auto(), probe);
        let out = uc.execute(label_only("iPhone")).await.expect("ok");
        let (_, urls) = parsed_urls(&out);
        assert_eq!(
            urls,
            vec!["http://192.168.1.5:42720", "http://192.168.2.1:42720"]
        );
    }

    #[tokio::test]
    async fn urls_put_public_entry_first_then_pinned_ip_then_nics() {
        // settings_port_base_url: 公网入口 + 钉死 IP 192.168.1.5 同时存在。
        // 顺序必须是 公网入口 → 钉死 IP → 其余网卡(补充决策 2026-06-11)。
        let probe = probe_returning(vec![iface("en2", [10, 0, 0, 5])]);
        let uc = build_uc_with(settings_port_base_url(), probe);
        let out = uc.execute(label_only("iPhone")).await.expect("ok");
        assert_eq!(out.base_url, "https://clip.example.com");
        let (url, urls) = parsed_urls(&out);
        assert_eq!(
            urls,
            vec![
                "https://clip.example.com",
                "http://192.168.1.5:42720",
                "http://10.0.0.5:42720",
            ]
        );
        assert_eq!(url, urls[0]);
    }

    #[tokio::test]
    async fn urls_keep_pinned_ip_first_and_dedupe_matching_nic() {
        // 无公网入口时钉死 IP 即 urls[0](v1 url 语义不变);它与网卡列表
        // 重复时只留靠前一份。
        let probe = probe_returning(vec![
            iface("en1", [192, 168, 1, 5]), // 与钉死 IP 相同 → 去重
            iface("en2", [10, 0, 0, 5]),
        ]);
        let uc = build_uc_with(settings_port_lan_advertise(true), probe);
        let out = uc.execute(label_only("iPhone")).await.expect("ok");
        assert_eq!(out.base_url, "http://192.168.1.5:42720");
        let (_, urls) = parsed_urls(&out);
        assert_eq!(
            urls,
            vec!["http://192.168.1.5:42720", "http://10.0.0.5:42720"]
        );
    }

    #[tokio::test]
    async fn probe_failure_degrades_to_configured_entries_only() {
        // v1 在公网入口/钉死 IP 路径下根本不探测网卡 —— 探测失败不能把
        // 这两条老路径弄死,降级为只带已配置候选。
        let uc = build_uc_with(settings_port_base_url(), probe_failing());
        let out = uc.execute(label_only("iPhone")).await.expect("ok");
        assert_eq!(out.base_url, "https://clip.example.com");
        let (_, urls) = parsed_urls(&out);
        assert_eq!(
            urls,
            vec!["https://clip.example.com", "http://192.168.1.5:42720"]
        );
    }

    #[tokio::test]
    async fn urls_truncate_to_cap_20() {
        // 25 个合格网卡 → 去重后截断到 20(规格 §5.4;tracing 告警不在
        // 此断言范围)。
        let nics: Vec<LanInterface> = (1..=25u8)
            .map(|i| iface(&format!("en{i}"), [10, 0, 0, i]))
            .collect();
        let uc = build_uc_with(settings_port_auto(), probe_returning(nics));
        let out = uc.execute(label_only("iPhone")).await.expect("ok");
        let (_, urls) = parsed_urls(&out);
        assert_eq!(urls.len(), 20);
        assert_eq!(urls[0], "http://10.0.0.1:42720");
        assert_eq!(urls[19], "http://10.0.0.20:42720");
    }

    #[tokio::test]
    async fn single_candidate_emits_v1_byte_identical_payload() {
        // 只有一个候选时 payload 不写 urls 字段 —— 与 v1 字节完全一致
        // (向后兼容硬约束,规格 §4.3)。
        let probe = probe_returning(vec![iface("en1", [192, 168, 1, 5])]);
        let uc = build_uc_with(settings_port_auto(), probe);
        let out = uc.execute(label_only("iPhone")).await.expect("ok");
        let (_, urls) = parsed_urls(&out);
        assert!(urls.is_empty(), "single candidate must not emit urls");
    }

    // ── tests: connect URI error translation ──────────────────────────

    #[test]
    fn translates_uri_too_long_to_qr_render_failed_with_hint() {
        // 直接测翻译函数, 避开"全 use case 路径正好凑齐 800+ 字符"的脆弱
        // 算术。end-to-end 上, 一旦未来新增字段让 URI 超长, 用户都会拿到
        // 一个稳定可读的 QrRenderFailed 错误。
        let err = translate_connect_uri_error(ConnectUriError::UriTooLong {
            len: 1200,
            max: 800,
        });
        match err {
            RegisterMobileShortcutDeviceError::QrRenderFailed(msg) => {
                assert!(
                    msg.contains("connect uri too long"),
                    "expected uri-too-long phrasing, got: {msg}"
                );
                assert!(msg.contains("1200"));
                assert!(msg.contains("800"));
            }
            other => panic!("expected QrRenderFailed, got {other:?}"),
        }
    }

    #[test]
    fn translates_other_connect_uri_errors_to_qr_render_failed() {
        // 6 个"理论上不可能触发"的变体都翻译成 QrRenderFailed, 保留原始
        // 错误描述给日志/UI排障, 不让 use case panic。
        for err in [
            ConnectUriError::InvalidScheme,
            ConnectUriError::UnsupportedVersion,
            ConnectUriError::UnsupportedService,
            ConnectUriError::PayloadDecodeFailed("simulated".into()),
            ConnectUriError::MissingField("url"),
            ConnectUriError::InvalidUrl,
        ] {
            let original = err.to_string();
            let translated = translate_connect_uri_error(err);
            match translated {
                RegisterMobileShortcutDeviceError::QrRenderFailed(msg) => {
                    assert!(
                        msg.contains("unexpected"),
                        "translation should mark unexpected variant: {msg}"
                    );
                    assert!(
                        msg.contains(&original),
                        "translation should retain original error text: {msg}"
                    );
                }
                other => panic!("expected QrRenderFailed for {original:?}, got {other:?}"),
            }
        }
    }

    // ── tests: analytics emit (schema doc §7.6 / §12.2 P1) ────────────

    #[tokio::test]
    async fn happy_path_emits_mobile_device_registered() {
        let (uc, analytics) = build_uc_with_sink(true);
        uc.execute(label_only("iPhone"))
            .await
            .expect("happy path must succeed");
        assert_eq!(analytics.events(), vec![Event::MobileDeviceRegistered]);
    }

    #[tokio::test]
    async fn lan_listener_disabled_does_not_emit_registration() {
        // 任何"还没真正落地一台设备"的失败路径都不应 emit registration anchor。
        let (uc, analytics) = build_uc_with_sink(false);
        let err = uc.execute(label_only("iPhone")).await.unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::LanListenerDisabled
        ));
        assert!(analytics.events().is_empty(), "{:?}", analytics.events());
    }
}
