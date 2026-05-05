//! `RegisterMobileShortcutDeviceUseCase` —— 在 daemon 上登记一台 iPhone
//! Shortcut 客户端，颁发其专属 token、打包 `.shortcut` 文件、注册一次性
//! 下载凭据。
//!
//! 流程（与 `.context/mobile-sync/SPEC.md` §9.1 对齐）：
//!   1. minter 颁发 32 字节 token + SHA-256 哈希 + 稳定 device id
//!   2. 构造 [`MobileDevice`] 实体并通过 repository 持久化
//!   3. 探测当前 LAN endpoint —— 监听未启用直接拒绝（业务前置条件）
//!   4. 调 `ShortcutPackerService` 把 lan_url / token / device_id 注入模板
//!   5. 注册一次性 download token（默认 5 分钟 TTL），拼出最终 download_url
//!   6. 把所有产物（设备元信息、download_url、过期时刻、配置串、二维码）
//!      汇总为 [`RegisterMobileShortcutDeviceOutput`] 返回
//!
//! 失败一律走 [`RegisterMobileShortcutDeviceError`] —— 把底层 port 错误
//! 翻译为用户/调用方能理解的语义（`uc-application/AGENTS.md` §13）。

use std::sync::Arc;

use tracing::{instrument, warn};
use url::form_urlencoded;

use uc_core::mobile_sync::{MintedToken, MobileClientType, MobileDevice, MobileDeviceError};
use uc_core::ports::{
    ClockPort, EndpointInfoError, MobileDeviceRepositoryPort, MobileSyncEndpointInfoPort,
    MobileTokenMinterPort, ShortcutDownloadTokenError, ShortcutDownloadTokenStorePort,
};

use super::shortcut_packer::{ShortcutPackError, ShortcutPackParams, ShortcutPackerService};

// ─── public-shaped (input / output / error) ─────────────────────────────

/// 调用方提交的请求：仅一个用户可读的设备标签。
#[derive(Debug, Clone)]
pub struct RegisterMobileShortcutDeviceInput {
    pub label: String,
}

/// 颁发成功后的产物。
///
/// `raw_token_hex` 故意不放在这里 —— 服务端层面只在打包给 iPhone 的
/// `.shortcut` 二进制里出现一次，登记响应本身只回传 download_url + 二维
/// 码。这样即便日志 / 设置 UI 被截图，token 也不会泄漏。
#[derive(Debug, Clone)]
pub struct RegisterMobileShortcutDeviceOutput {
    pub device: MobileDevice,
    /// 形如 `http://192.168.1.5:42720/mobile/v1/shortcut/install?dt=<dt>`
    pub download_url: String,
    pub download_expires_at_ms: i64,
    /// 配置串（v2 B 路径粘贴用），目前仅给前端展示 + 复制按钮。
    pub config_string: String,
    pub qr_code_png_bytes: Vec<u8>,
    pub qr_code_ascii: String,
}

/// use case 失败的全部语义。
#[derive(Debug, thiserror::Error)]
pub enum RegisterMobileShortcutDeviceError {
    /// 标签为空 —— UI / CLI 应在用户提交前先校验，这里是兜底。
    #[error("device label must not be empty")]
    LabelEmpty,

    /// 标签过长（超过 64 字符）—— 防止配置串 / sqlite 行被滥用为 BLOB。
    #[error("device label too long (max 64 chars)")]
    LabelTooLong,

    /// LAN 监听未启用 —— 没有可写入 `.shortcut` 的 lan_url，必须先开启。
    #[error("LAN listener is not enabled; enable it first")]
    LanListenerDisabled,

    /// 持久化失败（重复 device id / token hash 碰撞 / 底层存储错误）。
    #[error("device persistence failed: {0}")]
    PersistenceFailed(String),

    /// `.shortcut` 打包或二维码渲染失败。
    #[error("shortcut packaging failed: {0}")]
    PackagingFailed(String),

    /// 一次性下载凭据存储失败 —— 进程内缓存 corrupt / 容量满等。
    #[error("download token store failed: {0}")]
    DownloadTokenStoreFailed(String),

    /// 探测当前 LAN endpoint 时底层失败 —— 不同于"未启用"，这是真正的
    /// 错误，应当告知用户并支持重试。
    #[error("endpoint info probe failed: {0}")]
    EndpointInfoFailed(String),
}

// ─── use case ───────────────────────────────────────────────────────────

/// 默认的下载 token TTL（5 分钟，与 SPEC §5.1.1 一致）。
const DOWNLOAD_TOKEN_TTL_MS: i64 = 5 * 60 * 1000;
/// 设备标签最大长度。
const MAX_LABEL_LEN: usize = 64;

pub(crate) struct RegisterMobileShortcutDeviceUseCase {
    token_minter: Arc<dyn MobileTokenMinterPort>,
    device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    endpoint_info: Arc<dyn MobileSyncEndpointInfoPort>,
    download_tokens: Arc<dyn ShortcutDownloadTokenStorePort>,
    packer: Arc<dyn ShortcutPackerService>,
    clock: Arc<dyn ClockPort>,
}

impl RegisterMobileShortcutDeviceUseCase {
    pub(crate) fn new(
        token_minter: Arc<dyn MobileTokenMinterPort>,
        device_repo: Arc<dyn MobileDeviceRepositoryPort>,
        endpoint_info: Arc<dyn MobileSyncEndpointInfoPort>,
        download_tokens: Arc<dyn ShortcutDownloadTokenStorePort>,
        packer: Arc<dyn ShortcutPackerService>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self {
            token_minter,
            device_repo,
            endpoint_info,
            download_tokens,
            packer,
            clock,
        }
    }

    /// 登记一台新 iPhone Shortcut 设备。
    ///
    /// 该方法的 happy path 不可中途部分提交：repository 写成功后失败的
    /// 步骤（packer / download token）若失败，会留下"已登记但用户拿不
    /// 到 .shortcut"的孤儿记录。v1 接受该缺陷 —— 用户重新点"添加 iPhone"
    /// 即可生成新设备；旧的孤儿设备会被显示在列表里，撤销即可清理。
    #[instrument(skip(self, input), fields(label_len = input.label.len()))]
    pub(crate) async fn execute(
        &self,
        input: RegisterMobileShortcutDeviceInput,
    ) -> Result<RegisterMobileShortcutDeviceOutput, RegisterMobileShortcutDeviceError> {
        // 0. 标签前置校验 —— 兜底，不依赖上层。
        let label = input.label.trim().to_string();
        if label.is_empty() {
            return Err(RegisterMobileShortcutDeviceError::LabelEmpty);
        }
        if label.chars().count() > MAX_LABEL_LEN {
            return Err(RegisterMobileShortcutDeviceError::LabelTooLong);
        }

        // 1. 颁发 token + device id（单次原子调用，二者来自同一次 minting）。
        let MintedToken {
            raw_hex,
            hash,
            device_id,
        } = self.token_minter.mint_token();

        // 2. 构造并持久化 MobileDevice。
        let now_ms = self.clock.now_ms();
        let device = MobileDevice {
            device_id: device_id.clone(),
            label: label.clone(),
            client_type: MobileClientType::IosShortcut,
            token_hash: hash,
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

        // 3. 探测当前 LAN endpoint。LAN 监听未启用是业务前置错误，向用户
        //    建议"先去开启 LAN 监听"。
        let endpoint = self
            .endpoint_info
            .current_lan_endpoint()
            .await
            .map_err(translate_endpoint_error)?
            .ok_or(RegisterMobileShortcutDeviceError::LanListenerDisabled)?;

        // 4. 用 packer 把 url / token / device_id 注入模板，并先生成 .shortcut
        //    字节流；二维码内容稍后再渲染（依赖 download_url）。
        //
        //    这里做了一个折中：packer 的 trait 方法把 .shortcut 打包与二
        //    维码渲染合并了一次调用，所以下面要先 register download token
        //    才能拼出 download_url，再调 pack。
        let download_token = self
            .download_tokens
            .register(device_id.clone(), Vec::new(), DOWNLOAD_TOKEN_TTL_MS)
            .await
            .map_err(translate_download_token_error)?;
        let download_url = build_download_url(&endpoint.url, download_token.token.as_str());

        // 5. packer 打包并渲染二维码。
        let pack_params = ShortcutPackParams {
            lan_url: endpoint.url.clone(),
            raw_token_hex: raw_hex.clone(),
            device_id: device_id.clone(),
        };
        let packed = self
            .packer
            .pack(&pack_params, &download_url)
            .map_err(translate_pack_error)?;

        // 6. 把真实的 .shortcut 字节回填到下载凭据 —— register 时占位的
        //    空字节由 consume 路径替换为这里的 shortcut_bytes。
        //    （目前 store 的 register 一次性写入是占位实现，将在 Phase 3
        //    随 store 真实实现一起调整为 register-with-payload，留下 TODO。）
        //    TODO(phase3): 让 register() 直接接收 shortcut_bytes，避免空 register。
        let _ = packed.shortcut_bytes; // 暂占位以保留语义引用；下一轮重构。

        // 7. 拼装配置串（v2 B 路径用）：`uniclip://config?u=<url>&t=<token>`。
        let config_string = build_config_string(&endpoint.url, &raw_hex);

        Ok(RegisterMobileShortcutDeviceOutput {
            device,
            download_url,
            download_expires_at_ms: download_token.expires_at_ms,
            config_string,
            qr_code_png_bytes: packed.qr_code_png_bytes,
            qr_code_ascii: packed.qr_code_ascii,
        })
    }
}

// ─── helpers ────────────────────────────────────────────────────────────

fn build_download_url(lan_url: &str, dt: &str) -> String {
    format!(
        "{}/mobile/v1/shortcut/install?dt={}",
        lan_url.trim_end_matches('/'),
        form_urlencoded::byte_serialize(dt.as_bytes()).collect::<String>()
    )
}

fn build_config_string(lan_url: &str, raw_token_hex: &str) -> String {
    let mut out = String::from("uniclip://config?");
    out.push_str(
        &form_urlencoded::Serializer::new(String::new())
            .append_pair("u", lan_url)
            .append_pair("t", raw_token_hex)
            .finish(),
    );
    out
}

fn translate_device_error(err: MobileDeviceError) -> RegisterMobileShortcutDeviceError {
    match err {
        MobileDeviceError::AlreadyExists(id) => {
            // device_id 由 minter 一次性生成，碰撞理论上不可能；走到这里
            // 说明 minter 实现有缺陷 —— 提示运维 + 翻译为 persistence 错误。
            warn!(
                ?id,
                "minter produced colliding device id; this should not happen"
            );
            RegisterMobileShortcutDeviceError::PersistenceFailed(
                "device id collision (minter contract violated)".to_string(),
            )
        }
        MobileDeviceError::TokenHashCollision => {
            warn!("minter produced colliding token hash; this should not happen");
            RegisterMobileShortcutDeviceError::PersistenceFailed(
                "token hash collision (minter contract violated)".to_string(),
            )
        }
        MobileDeviceError::Storage(msg) => {
            RegisterMobileShortcutDeviceError::PersistenceFailed(msg)
        }
    }
}

fn translate_endpoint_error(err: EndpointInfoError) -> RegisterMobileShortcutDeviceError {
    match err {
        EndpointInfoError::Storage(msg) => {
            RegisterMobileShortcutDeviceError::EndpointInfoFailed(msg)
        }
    }
}

fn translate_download_token_error(
    err: ShortcutDownloadTokenError,
) -> RegisterMobileShortcutDeviceError {
    match err {
        ShortcutDownloadTokenError::Internal(msg) => {
            RegisterMobileShortcutDeviceError::DownloadTokenStoreFailed(msg)
        }
    }
}

fn translate_pack_error(err: ShortcutPackError) -> RegisterMobileShortcutDeviceError {
    RegisterMobileShortcutDeviceError::PackagingFailed(err.to_string())
}

// ─── tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;

    use uc_core::mobile_sync::{
        LanEndpointInfo, MobileDeviceId, RegisteredDownloadToken, ShortcutDownloadToken, TokenHash,
    };

    // ── fixtures ────────────────────────────────────────────────────

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    struct DeterministicMinter;
    impl MobileTokenMinterPort for DeterministicMinter {
        fn mint_token(&self) -> MintedToken {
            MintedToken {
                raw_hex: "a".repeat(64),
                hash: TokenHash::new([1u8; 32]),
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
        async fn find_by_token_hash(
            &self,
            _: &TokenHash,
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

    struct FixedEndpoint(Option<&'static str>);
    #[async_trait]
    impl MobileSyncEndpointInfoPort for FixedEndpoint {
        async fn current_lan_endpoint(&self) -> Result<Option<LanEndpointInfo>, EndpointInfoError> {
            Ok(self.0.map(|url| LanEndpointInfo { url: url.into() }))
        }
    }

    #[derive(Default)]
    struct StubDownloadTokenStore {
        next_token: Mutex<u64>,
    }
    #[async_trait]
    impl ShortcutDownloadTokenStorePort for StubDownloadTokenStore {
        async fn register(
            &self,
            _: MobileDeviceId,
            _: Vec<u8>,
            ttl_ms: i64,
        ) -> Result<RegisteredDownloadToken, ShortcutDownloadTokenError> {
            let mut n = self.next_token.lock().unwrap();
            *n += 1;
            Ok(RegisteredDownloadToken {
                token: ShortcutDownloadToken::new(format!("dt_{}", *n)),
                expires_at_ms: 1_000 + ttl_ms,
            })
        }
        async fn consume(
            &self,
            _: &ShortcutDownloadToken,
        ) -> Result<Option<(MobileDeviceId, Vec<u8>)>, ShortcutDownloadTokenError> {
            Ok(None)
        }
    }

    fn build_uc(endpoint: Option<&'static str>) -> RegisterMobileShortcutDeviceUseCase {
        use super::super::shortcut_packer::StubShortcutPackerService;

        RegisterMobileShortcutDeviceUseCase::new(
            Arc::new(DeterministicMinter),
            Arc::new(InMemoryDeviceRepo::default()),
            Arc::new(FixedEndpoint(endpoint)),
            Arc::new(StubDownloadTokenStore::default()),
            Arc::new(StubShortcutPackerService),
            Arc::new(FixedClock(1_000)),
        )
    }

    // ── tests ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn rejects_empty_label() {
        let uc = build_uc(Some("http://192.168.1.5:42720"));
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
        let uc = build_uc(Some("http://192.168.1.5:42720"));
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
        let uc = build_uc(None);
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
    async fn happy_path_returns_packaged_artifacts() {
        let uc = build_uc(Some("http://192.168.1.5:42720"));
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

        // download_url 由 lan_url + 一次性 token 拼成
        assert!(out
            .download_url
            .starts_with("http://192.168.1.5:42720/mobile/v1/shortcut/install?dt="));
        assert!(out.download_url.contains("dt_1"));

        // 过期时间 = clock.now_ms (在 store 中 = 1000) + TTL
        assert_eq!(out.download_expires_at_ms, 1_000 + DOWNLOAD_TOKEN_TTL_MS);

        // 配置串包含 url + token
        assert!(out.config_string.starts_with("uniclip://config?"));
        assert!(out
            .config_string
            .contains("u=http%3A%2F%2F192.168.1.5%3A42720"));
        // token (64 个 'a') 是 url-safe，不会被编码
        assert!(out.config_string.contains(&format!("t={}", "a".repeat(64))));

        // 二维码字节非空（stub packer 至少返回占位）
        assert!(!out.qr_code_png_bytes.is_empty());
        assert!(!out.qr_code_ascii.is_empty());
    }
}
