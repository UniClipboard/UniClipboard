//! `RegisterMobileShortcutDeviceUseCase` —— 在 daemon 上登记一台 iPhone
//! Shortcut 客户端,颁发其独立 (username, password) Basic Auth 凭据。
//!
//! v3 SyncClipboard 兼容路径(`.context/mobile-sync/SPEC.md` §14):
//!
//!   1. credentials minter 颁发 (username, password, password_hash, device_id)
//!   2. 探测当前 LAN endpoint —— 监听未启用直接拒绝(业务前置条件)
//!   3. 构造 [`MobileDevice`] 实体并通过 repository 持久化
//!   4. 把 install URL(SyncClipboard EX iCloud 共享链接,常量)渲染成 PNG +
//!      ASCII 二维码,让用户扫码安装该 shortcut
//!   5. 回传 base_url + username + password(明文,**仅这一次**) + install_url
//!      + 二维码 —— 用户在 SyncClipboard shortcut 里手动填这三项凭据
//!
//! 失败一律走 [`RegisterMobileShortcutDeviceError`] —— 把底层 port 错误
//! 翻译为用户/调用方能理解的语义(`uc-application/AGENTS.md` §13)。

use std::sync::Arc;

use tracing::{instrument, warn};

use uc_core::mobile_sync::{MintedCredentials, MobileClientType, MobileDevice, MobileDeviceError};
use uc_core::ports::{
    ClockPort, MobileCredentialsMinterPort, MobileDeviceRepositoryPort, SettingsPort,
};

// ─── public-shaped (input / output / error) ─────────────────────────────

/// 调用方提交的请求:仅一个用户可读的设备标签。
#[derive(Debug, Clone)]
pub struct RegisterMobileShortcutDeviceInput {
    pub label: String,
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
    pub username: String,
    /// 一次性回显:明文密码,用户在 SyncClipboard shortcut 里填进 `password` 框。
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
}

// ─── use case ───────────────────────────────────────────────────────────

/// 设备标签最大长度。
const MAX_LABEL_LEN: usize = 64;

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
    device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    settings: Arc<dyn SettingsPort>,
    clock: Arc<dyn ClockPort>,
}

impl RegisterMobileShortcutDeviceUseCase {
    pub(crate) fn new(
        credentials_minter: Arc<dyn MobileCredentialsMinterPort>,
        device_repo: Arc<dyn MobileDeviceRepositoryPort>,
        settings: Arc<dyn SettingsPort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self {
            credentials_minter,
            device_repo,
            settings,
            clock,
        }
    }

    /// 登记一台新 iPhone Shortcut 设备。
    ///
    /// base_url 完全由 settings 决定:
    /// `lan_listen_enabled=false` → `LanListenerDisabled`(用户没开 LAN);
    /// `lan_advertise_ip=None` → 退回 `127.0.0.1`(本机调试);
    /// 否则 `http://<advertise_ip>:<lan_port || 42720>`。
    ///
    /// 不依赖 `MobileSyncEndpointInfoPort`(那是 daemon 进程内运行时状
    /// 态, CLI 进程不可达)。
    ///
    /// happy path 不可中途部分提交:repository 写成功后, 后续二维码渲染
    /// 失败会留下"已登记但用户拿不到 install URL"的孤儿记录。v1 接受
    /// 该缺陷 —— 用户重新点"添加 iPhone"即可生成新设备;旧的孤儿设备
    /// 会被显示在列表里, 撤销即可清理。
    #[instrument(skip(self, input), fields(label_len = input.label.len()))]
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

        // 1. 读 settings 决定 base_url —— 没开 LAN 监听就直接拒绝, 避免
        //    颁发了凭据却没 base_url 给用户的尴尬中间态。
        let settings = self.settings.load().await.map_err(|err| {
            RegisterMobileShortcutDeviceError::SettingsLoadFailed(err.to_string())
        })?;
        if !settings.mobile_sync.lan_listen_enabled {
            return Err(RegisterMobileShortcutDeviceError::LanListenerDisabled);
        }
        let advertise_ip = settings
            .mobile_sync
            .lan_advertise_ip
            .as_deref()
            .unwrap_or("127.0.0.1");
        let port = settings.mobile_sync.lan_port.unwrap_or(42720);
        let base_url = format!("http://{advertise_ip}:{port}");

        // 2. 颁发凭据 —— 单次原子调用, 4 项产物来自同一次 minting。
        let MintedCredentials {
            username,
            password,
            password_hash,
            device_id,
        } = self.credentials_minter.mint_credentials();

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
}

// ─── helpers ────────────────────────────────────────────────────────────

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
            // 8 hex(4 字节)碰撞概率极低,但仍可能;翻译为 persistence,
            // UI 提示重试一次即可。
            warn!("minter produced colliding username; retry register to mint a new pair");
            RegisterMobileShortcutDeviceError::PersistenceFailed(
                "username collision; retry registration".to_string(),
            )
        }
        MobileDeviceError::Storage(msg) => {
            RegisterMobileShortcutDeviceError::PersistenceFailed(msg)
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

    #[derive(Default)]
    struct InMemoryDeviceRepo {
        saved: Mutex<Vec<MobileDevice>>,
    }
    #[async_trait]
    impl MobileDeviceRepositoryPort for InMemoryDeviceRepo {
        async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError> {
            self.saved.lock().unwrap().push(device.clone());
            Ok(())
        }
        async fn find_by_username(
            &self,
            _: &str,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            Ok(None)
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

    fn build_uc(lan_listen_enabled: bool) -> RegisterMobileShortcutDeviceUseCase {
        RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(DeterministicMinter),
            Arc::new(InMemoryDeviceRepo::default()),
            Arc::new(FixedSettings { lan_listen_enabled }),
            Arc::new(FixedClock(1_000)),
        )
    }

    // ── tests ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn rejects_empty_label() {
        let uc = build_uc(true);
        let err = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "   ".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, RegisterMobileShortcutDeviceError::LabelEmpty));
    }

    #[tokio::test]
    async fn rejects_overlong_label() {
        let uc = build_uc(true);
        let err = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "x".repeat(MAX_LABEL_LEN + 1),
            })
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
        let err = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "我的 iPhone".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            RegisterMobileShortcutDeviceError::LanListenerDisabled
        ));
    }

    #[tokio::test]
    async fn happy_path_returns_credentials_and_install_url() {
        let uc = build_uc(true);
        let out = uc
            .execute(RegisterMobileShortcutDeviceInput {
                label: "我的 iPhone".into(),
            })
            .await
            .expect("happy path must succeed");

        // 设备元信息
        assert_eq!(out.device.label, "我的 iPhone");
        assert_eq!(out.device.client_type, MobileClientType::IosShortcut);
        assert_eq!(out.device.created_at_ms, 1_000);
        assert_eq!(out.device.username, "mobile_aabbccdd");

        // 一次性回显的凭据
        assert_eq!(out.username, "mobile_aabbccdd");
        assert_eq!(out.password, "deterministic-password-22");
        assert_eq!(out.base_url, "http://192.168.1.5:42720");
        assert_eq!(out.install_url, SYNC_CLIPBOARD_EX_INSTALL_URL);

        // 二维码必须非空,且 PNG 字节有 magic header `\x89PNG`。
        assert!(out.qr_code_png_bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]));
        assert!(!out.qr_code_ascii.is_empty());
    }
}
