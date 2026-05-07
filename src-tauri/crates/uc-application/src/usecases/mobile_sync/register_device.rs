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

use tracing::{instrument, warn};

use uc_core::mobile_sync::{
    LanInterface, MintedCredentials, MobileClientType, MobileDevice, MobileDeviceError,
};
use uc_core::ports::{
    ClockPort, LanInterfaceProbeError, LanInterfaceProbePort, MobileCredentialsMinterPort,
    MobileDeviceRepositoryPort, PasswordHasherError, PasswordHasherPort, SettingsPort,
};

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
    /// daemon 当前对外暴露的 LAN URL,用户在 SyncClipboard shortcut 里
    /// 填进 `url` 框,形如 `http://192.168.1.5:42720`。
    pub base_url: String,
    /// 一次性回显:用户在 SyncClipboard shortcut 里填进 `username` 框。
    /// 自定义模式下与 `input.username` 相同;自动模式下来自 minter。
    pub username: String,
    /// 一次性回显:明文密码,用户在 SyncClipboard shortcut 里填进 `password` 框。
    /// 自定义模式下与 `input.password` 相同;自动模式下来自 minter。
    pub password: String,
    /// SyncClipboard "Clipboard EX" iCloud 共享链接(常量) —— 用户扫描
    /// `qr_code_*` 后跳转此链接安装该 shortcut。
    pub install_url: String,
    /// `install_url` 的二维码 PNG 字节流,前端可走 base64 data URL 直接渲染。
    pub qr_code_png_bytes: Vec<u8>,
    /// `install_url` 的二维码 ASCII(块字符),CLI 直接 `println!`。
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

    /// 自定义 username 不符合形态规则(长度 / 字符集 / 必须字母开头)。
    #[error("invalid username shape: {0}")]
    UsernameInvalidShape(String),

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

    /// `lan_advertise_ip` 为 None(用户选了"自动"),但本机检测不到任何
    /// 可用的 RFC1918 私有 LAN IPv4 地址 —— iPhone 没有可达的 base_url。
    /// 用户需先连入 LAN 或在配置里手动指定 IP。
    #[error("no usable LAN interface for auto-pick base_url")]
    NoLanInterfaceAvailable,

    /// 探测 LAN 接口失败(底层 syscall 错误)。
    #[error("lan interface probe failed: {0}")]
    LanInterfaceProbeFailed(String),
}

// ─── use case ───────────────────────────────────────────────────────────

/// 设备标签最大长度。
const MAX_LABEL_LEN: usize = 64;

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
    "https://www.icloud.com/shortcuts/34404963b512432cb5672c8a95001b19";

pub(crate) struct RegisterMobileShortcutDeviceUseCase {
    credentials_minter: Arc<dyn MobileCredentialsMinterPort>,
    password_hasher: Arc<dyn PasswordHasherPort>,
    device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    settings: Arc<dyn SettingsPort>,
    clock: Arc<dyn ClockPort>,
    lan_interface_probe: Arc<dyn LanInterfaceProbePort>,
}

impl RegisterMobileShortcutDeviceUseCase {
    pub(crate) fn new(
        credentials_minter: Arc<dyn MobileCredentialsMinterPort>,
        password_hasher: Arc<dyn PasswordHasherPort>,
        device_repo: Arc<dyn MobileDeviceRepositoryPort>,
        settings: Arc<dyn SettingsPort>,
        clock: Arc<dyn ClockPort>,
        lan_interface_probe: Arc<dyn LanInterfaceProbePort>,
    ) -> Self {
        Self {
            credentials_minter,
            password_hasher,
            device_repo,
            settings,
            clock,
            lan_interface_probe,
        }
    }

    /// 在 `lan_advertise_ip = None`("自动")时,挑一个 RFC1918 LAN IPv4 地址
    /// 用作 iPhone base_url。daemon 永远绑 `0.0.0.0`,所以不影响 bind;但
    /// iPhone 必须看到一个真实可达的地址,否则 SyncClipboard 永远连不通。
    ///
    /// 排序口径与 [`ListLanInterfacesUseCase`] 保持一致:10/8 → 172.16/12
    /// → 192.168/16,段内字典序;取第一个即可。
    ///
    /// [`ListLanInterfacesUseCase`]: super::list_lan_interfaces::ListLanInterfacesUseCase
    async fn auto_pick_advertise_ip(&self) -> Result<String, RegisterMobileShortcutDeviceError> {
        let raw = self
            .lan_interface_probe
            .list_interfaces()
            .await
            .map_err(translate_probe_error)?;

        let mut candidates: Vec<LanInterface> =
            raw.into_iter().filter(is_rfc1918_lan_candidate).collect();
        candidates.sort_by(|a, b| {
            rfc1918_bucket(&a.ipv4.octets())
                .cmp(&rfc1918_bucket(&b.ipv4.octets()))
                .then_with(|| a.ipv4.cmp(&b.ipv4))
        });

        candidates
            .into_iter()
            .next()
            .map(|iface| iface.ipv4.to_string())
            .ok_or(RegisterMobileShortcutDeviceError::NoLanInterfaceAvailable)
    }

    /// 登记一台新 iPhone Shortcut 设备。
    ///
    /// base_url 由 settings 决定:
    /// `lan_listen_enabled=false` → `LanListenerDisabled`(用户没开 LAN);
    /// `lan_advertise_ip=Some(ip)` → 用该 IP;
    /// `lan_advertise_ip=None` → 自动挑一个 RFC1918 LAN IPv4
    ///   ([`auto_pick_advertise_ip`]),没候选时 → `NoLanInterfaceAvailable`。
    ///
    /// 不依赖 `MobileSyncEndpointInfoPort`(那是 daemon 进程内运行时状
    /// 态, CLI 进程不可达)。
    ///
    /// [`auto_pick_advertise_ip`]: Self::auto_pick_advertise_ip
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
        let label = input.label.trim().to_string();
        if label.is_empty() {
            return Err(RegisterMobileShortcutDeviceError::LabelEmpty);
        }
        if label.chars().count() > MAX_LABEL_LEN {
            return Err(RegisterMobileShortcutDeviceError::LabelTooLong);
        }

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
        // 用户选了"自动" → 让 daemon 替他挑一个 RFC1918 LAN IP。daemon 永远
        // bind `0.0.0.0:lan_port`,但 iPhone 得到的 base_url 必须是真实可达
        // 的 LAN 地址(0.0.0.0 / 127.0.0.1 在 iPhone 上都连不通)。
        let advertise_ip: String = match settings.mobile_sync.lan_advertise_ip.clone() {
            Some(ip) => ip,
            None => self.auto_pick_advertise_ip().await?,
        };
        let port = settings.mobile_sync.lan_port.unwrap_or(42720);
        let base_url = format!("http://{advertise_ip}:{port}");

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
        self.device_repo
            .save(&device)
            .await
            .map_err(translate_device_error)?;

        // 4. 渲染 install URL 的二维码(PNG + ASCII 双形态)。install_url 是
        //    常量(SyncClipboard 公开 iCloud 链接), 不取决于 device, 二维码
        //    内容对所有用户都一样;但每次仍各自渲染一次 —— 不引入全局缓存,
        //    保持 use case 无副作用易测试。
        let install_url = SYNC_CLIPBOARD_EX_INSTALL_URL.to_string();
        let (qr_code_png_bytes, qr_code_ascii) = render_install_qr(&install_url)?;

        Ok(RegisterMobileShortcutDeviceOutput {
            device,
            base_url,
            username,
            password,
            install_url,
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
        match self.device_repo.find_by_username(username).await {
            Ok(Some(_)) => Err(RegisterMobileShortcutDeviceError::UsernameTaken(
                username.to_string(),
            )),
            Ok(None) => Ok(()),
            Err(err) => Err(translate_device_error(err)),
        }
    }
}

// ─── helpers ────────────────────────────────────────────────────────────

/// 校验自定义 username 形态:
/// - 长度 [`MIN_USERNAME_LEN`]–[`MAX_USERNAME_LEN`]
/// - 必须以 ASCII 字母开头(避免 Basic Auth header 解析歧义)
/// - 只允许 `[A-Za-z0-9_]`
fn validate_username_shape(username: &str) -> Result<(), RegisterMobileShortcutDeviceError> {
    let len = username.chars().count();
    if len < MIN_USERNAME_LEN {
        return Err(RegisterMobileShortcutDeviceError::UsernameInvalidShape(
            format!("must be at least {MIN_USERNAME_LEN} characters (got {len})"),
        ));
    }
    if len > MAX_USERNAME_LEN {
        return Err(RegisterMobileShortcutDeviceError::UsernameInvalidShape(
            format!("must be at most {MAX_USERNAME_LEN} characters (got {len})"),
        ));
    }
    let mut chars = username.chars();
    let first = chars.next().expect("len ≥ MIN_USERNAME_LEN > 0");
    if !first.is_ascii_alphabetic() {
        return Err(RegisterMobileShortcutDeviceError::UsernameInvalidShape(
            "must start with an ASCII letter".to_string(),
        ));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(RegisterMobileShortcutDeviceError::UsernameInvalidShape(
            "only letters, digits, and underscore are allowed".to_string(),
        ));
    }
    Ok(())
}

/// 校验自定义 password 长度。**不**校验复杂度(用户选"宽松")。
fn validate_password_length(password: &str) -> Result<(), RegisterMobileShortcutDeviceError> {
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

/// 自动挑选用的 RFC1918 过滤口径(与 `list_lan_interfaces` use case 一致)。
fn is_rfc1918_lan_candidate(iface: &LanInterface) -> bool {
    if iface.is_loopback {
        return false;
    }
    let octets = iface.ipv4.octets();
    matches!(
        octets,
        [10, _, _, _] | [172, 16..=31, _, _] | [192, 168, _, _]
    )
}

/// 排序桶:10.x = 0,172.16.x = 1,192.168.x = 2,其它 = 3。
fn rfc1918_bucket(octets: &[u8; 4]) -> u8 {
    match octets {
        [10, _, _, _] => 0,
        [172, 16..=31, _, _] => 1,
        [192, 168, _, _] => 2,
        _ => 3,
    }
}

fn translate_probe_error(err: LanInterfaceProbeError) -> RegisterMobileShortcutDeviceError {
    match err {
        LanInterfaceProbeError::Probe(msg) => {
            RegisterMobileShortcutDeviceError::LanInterfaceProbeFailed(msg)
        }
    }
}

/// 把 install URL 渲染为 PNG + ASCII 二维码。
///
/// PNG: `qrcode::QrCode::render::<Luma<u8>>` 出 `image::ImageBuffer` →
/// 写到 PNG cursor。ASCII: 调 `render::<unicode::Dense1x2>` 用 1×2 块
/// 字符渲染,适合 80 列终端。
fn render_install_qr(
    install_url: &str,
) -> Result<(Vec<u8>, String), RegisterMobileShortcutDeviceError> {
    use image::{ImageFormat, Luma};
    use qrcode::render::unicode::Dense1x2;
    use qrcode::QrCode;

    let code = QrCode::new(install_url.as_bytes())
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
            warn!("username collision at save time (likely concurrent register race)");
            RegisterMobileShortcutDeviceError::UsernameTaken(
                "username taken at save time (concurrent registration)".to_string(),
            )
        }
        MobileDeviceError::Storage(msg) => {
            RegisterMobileShortcutDeviceError::PersistenceFailed(msg)
        }
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

    use std::sync::Mutex;

    use async_trait::async_trait;

    use uc_core::mobile_sync::MobileDeviceId;
    use uc_core::settings::model::Settings;

    // ── fixtures ────────────────────────────────────────────────────

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    struct DeterministicMinter;
    impl MobileCredentialsMinterPort for DeterministicMinter {
        fn mint_credentials(&self) -> MintedCredentials {
            MintedCredentials {
                username: "mobile_aabbccdd".into(),
                password: "deterministic-password-22".into(),
                password_hash: "$argon2id$v=19$m=64,t=1,p=1$AAAAAAAAAAAAAAAA$test".into(),
                device_id: MobileDeviceId::new("did_aaaa"),
            }
        }
    }

    /// 把每次 hash 调用记录下来,便于断言 use case 是否真去 hash 了自定义
    /// password(而不是回退用 minter 的 phc 字符串)。
    #[derive(Default)]
    struct RecordingHasher {
        hashed: Mutex<Vec<String>>,
    }
    #[async_trait]
    impl PasswordHasherPort for RecordingHasher {
        async fn hash(&self, password: &str) -> Result<String, PasswordHasherError> {
            self.hashed.lock().unwrap().push(password.to_string());
            Ok(format!("phc-of:{password}"))
        }
        async fn verify(&self, _password: &str, _phc: &str) -> Result<bool, PasswordHasherError> {
            unreachable!("register flow does not call verify")
        }
    }

    /// hash() 永远报内部错误的 fixture,断言 use case 把它翻成
    /// `PasswordHashFailed`。
    struct FailingHasher;
    #[async_trait]
    impl PasswordHasherPort for FailingHasher {
        async fn hash(&self, _password: &str) -> Result<String, PasswordHasherError> {
            Err(PasswordHasherError::Internal(
                "simulated hash failure".into(),
            ))
        }
        async fn verify(&self, _password: &str, _phc: &str) -> Result<bool, PasswordHasherError> {
            unreachable!()
        }
    }

    #[derive(Default)]
    struct InMemoryDeviceRepo {
        saved: Mutex<Vec<MobileDevice>>,
        /// 预置:这些 username 视为"已被占用",`find_by_username` 命中。
        preexisting: Mutex<Vec<String>>,
    }
    impl InMemoryDeviceRepo {
        fn with_existing_username(name: &str) -> Self {
            let s = Self::default();
            s.preexisting.lock().unwrap().push(name.to_string());
            s
        }
    }
    #[async_trait]
    impl MobileDeviceRepositoryPort for InMemoryDeviceRepo {
        async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError> {
            self.saved.lock().unwrap().push(device.clone());
            Ok(())
        }
        async fn find_by_username(
            &self,
            username: &str,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            // 真正存在的(saved 里)优先;之外再看 preexisting fixture 名单。
            let saved = self.saved.lock().unwrap();
            if let Some(d) = saved.iter().find(|d| d.username == username) {
                return Ok(Some(d.clone()));
            }
            drop(saved);
            if self
                .preexisting
                .lock()
                .unwrap()
                .iter()
                .any(|u| u == username)
            {
                Ok(Some(MobileDevice {
                    device_id: MobileDeviceId::new("did_existing"),
                    label: "existing".into(),
                    client_type: MobileClientType::IosShortcut,
                    username: username.to_string(),
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
        }
        async fn find_by_device_id(
            &self,
            _: &MobileDeviceId,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            Ok(None)
        }
        async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError> {
            Ok(self.saved.lock().unwrap().clone())
        }
        async fn delete(&self, _: &MobileDeviceId) -> Result<bool, MobileDeviceError> {
            Ok(true)
        }
        async fn record_activity(
            &self,
            _: &MobileDeviceId,
            _: i64,
            _: Option<String>,
            _: Option<String>,
            _: Option<String>,
        ) -> Result<(), MobileDeviceError> {
            Ok(())
        }
    }

    /// 内存 SettingsPort: `lan_listen_enabled` 由测试控制;`lan_advertise_ip`
    /// 固定 192.168.1.5 + 端口 42720, 让 base_url 推出 "http://192.168.1.5:42720"。
    struct FixedSettings {
        lan_listen_enabled: bool,
    }
    #[async_trait]
    impl SettingsPort for FixedSettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            let mut s = Settings::default();
            s.mobile_sync.enabled = self.lan_listen_enabled;
            s.mobile_sync.lan_listen_enabled = self.lan_listen_enabled;
            s.mobile_sync.lan_advertise_ip = Some("192.168.1.5".into());
            s.mobile_sync.lan_port = Some(42720);
            Ok(s)
        }
        async fn save(&self, _: &Settings) -> anyhow::Result<()> {
            unreachable!("register_device must not save settings")
        }
    }

    /// 默认测试 probe:返回空列表。原 happy path 测试都用 lan_advertise_ip
    /// = Some(...),不会走 auto-pick,所以空列表 probe 已够用;auto-pick 路径
    /// 由 `auto_picks_first_rfc1918_when_advertise_ip_unset` 等单独构造。
    struct EmptyLanProbe;
    #[async_trait]
    impl LanInterfaceProbePort for EmptyLanProbe {
        async fn list_interfaces(&self) -> Result<Vec<LanInterface>, LanInterfaceProbeError> {
            Ok(Vec::new())
        }
    }

    /// 测试 probe:返回固定的 LAN 接口列表(用于断言 auto-pick 排序口径)。
    struct FixedLanProbe(Vec<LanInterface>);
    #[async_trait]
    impl LanInterfaceProbePort for FixedLanProbe {
        async fn list_interfaces(&self) -> Result<Vec<LanInterface>, LanInterfaceProbeError> {
            Ok(self.0.clone())
        }
    }

    /// `lan_advertise_ip = None` 的 SettingsPort 变体:其它字段同
    /// `FixedSettings`,只有 advertise_ip 留空,触发 auto-pick 分支。
    struct AutoSettings;
    #[async_trait]
    impl SettingsPort for AutoSettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            let mut s = Settings::default();
            s.mobile_sync.enabled = true;
            s.mobile_sync.lan_listen_enabled = true;
            s.mobile_sync.lan_advertise_ip = None;
            s.mobile_sync.lan_port = Some(42720);
            Ok(s)
        }
        async fn save(&self, _: &Settings) -> anyhow::Result<()> {
            unreachable!("register_device must not save settings")
        }
    }

    fn iface(name: &str, ip: [u8; 4]) -> LanInterface {
        LanInterface {
            name: name.into(),
            ipv4: std::net::Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3]),
            is_loopback: false,
        }
    }

    fn build_uc(lan_listen_enabled: bool) -> RegisterMobileShortcutDeviceUseCase {
        RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(DeterministicMinter),
            Arc::new(RecordingHasher::default()),
            Arc::new(InMemoryDeviceRepo::default()),
            Arc::new(FixedSettings { lan_listen_enabled }),
            Arc::new(FixedClock(1_000)),
            Arc::new(EmptyLanProbe),
        )
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
        assert_eq!(out.install_url, SYNC_CLIPBOARD_EX_INSTALL_URL);

        // 二维码必须非空,且 PNG 字节有 magic header `\x89PNG`。
        assert!(out.qr_code_png_bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        assert!(!out.qr_code_ascii.is_empty());
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
            RegisterMobileShortcutDeviceError::UsernameInvalidShape(_)
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
            RegisterMobileShortcutDeviceError::UsernameInvalidShape(_)
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
            RegisterMobileShortcutDeviceError::UsernameInvalidShape(_)
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
            RegisterMobileShortcutDeviceError::UsernameInvalidShape(_)
        ));
    }

    #[tokio::test]
    async fn rejects_username_already_taken() {
        let uc = RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(DeterministicMinter),
            Arc::new(RecordingHasher::default()),
            Arc::new(InMemoryDeviceRepo::with_existing_username("alice_001")),
            Arc::new(FixedSettings {
                lan_listen_enabled: true,
            }),
            Arc::new(FixedClock(1_000)),
            Arc::new(EmptyLanProbe),
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
        let hasher = Arc::new(RecordingHasher::default());
        let uc = RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(DeterministicMinter),
            hasher.clone(),
            Arc::new(InMemoryDeviceRepo::default()),
            Arc::new(FixedSettings {
                lan_listen_enabled: true,
            }),
            Arc::new(FixedClock(1_000)),
            Arc::new(EmptyLanProbe),
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
        assert_eq!(out.username, "mobile_aabbccdd");

        // 断言 hasher 真被调用了 1 次。
        assert_eq!(hasher.hashed.lock().unwrap().len(), 1);
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
        let uc = RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(DeterministicMinter),
            Arc::new(FailingHasher),
            Arc::new(InMemoryDeviceRepo::default()),
            Arc::new(FixedSettings {
                lan_listen_enabled: true,
            }),
            Arc::new(FixedClock(1_000)),
            Arc::new(EmptyLanProbe),
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
        assert_eq!(out.device.device_id.as_str(), "did_aaaa");
    }

    // ── tests: auto-pick advertise_ip ─────────────────────────────────

    fn build_uc_auto(probe: Arc<dyn LanInterfaceProbePort>) -> RegisterMobileShortcutDeviceUseCase {
        RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(DeterministicMinter),
            Arc::new(RecordingHasher::default()),
            Arc::new(InMemoryDeviceRepo::default()),
            Arc::new(AutoSettings),
            Arc::new(FixedClock(1_000)),
            probe,
        )
    }

    #[tokio::test]
    async fn auto_picks_first_rfc1918_when_advertise_ip_unset() {
        // 故意打乱顺序,断言走"10/8 → 172.16/12 → 192.168/16,段内字典序"。
        // 期望挑 10.0.0.5(10.x 段最小)。
        let probe = Arc::new(FixedLanProbe(vec![
            iface("en1", [192, 168, 1, 5]),
            iface("en2", [10, 0, 0, 5]),
            iface("en3", [172, 16, 0, 5]),
            iface("en4", [10, 1, 1, 1]),
        ]));
        let uc = build_uc_auto(probe);
        let out = uc.execute(label_only("iPhone")).await.expect("ok");
        assert_eq!(out.base_url, "http://10.0.0.5:42720");
    }

    #[tokio::test]
    async fn auto_skips_loopback_and_non_rfc1918() {
        // 全是被剔除的接口 → 退化成"没有可用 LAN" → NoLanInterfaceAvailable。
        let probe = Arc::new(FixedLanProbe(vec![
            LanInterface {
                name: "lo0".into(),
                ipv4: std::net::Ipv4Addr::new(127, 0, 0, 1),
                is_loopback: true,
            },
            iface("en_pub", [8, 8, 8, 8]),
            iface("en_cgnat", [100, 64, 1, 5]),
            iface("en_link", [169, 254, 1, 5]),
        ]));
        let uc = build_uc_auto(probe);
        let err = uc.execute(label_only("iPhone")).await.unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::NoLanInterfaceAvailable
        ));
    }

    #[tokio::test]
    async fn auto_translates_probe_failure() {
        struct FailingProbe;
        #[async_trait]
        impl LanInterfaceProbePort for FailingProbe {
            async fn list_interfaces(&self) -> Result<Vec<LanInterface>, LanInterfaceProbeError> {
                Err(LanInterfaceProbeError::Probe("ifaddr crashed".into()))
            }
        }
        let uc = build_uc_auto(Arc::new(FailingProbe));
        let err = uc.execute(label_only("iPhone")).await.unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::LanInterfaceProbeFailed(ref s) if s.contains("ifaddr crashed")
        ));
    }
}
