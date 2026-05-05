//! [`MobileSyncFacade`] —— 移动端同步功能的应用层入口。
//!
//! 按 `uc-application/AGENTS.md` §11.4，外部 crate（bootstrap / daemon /
//! tauri / cli）只能通过本目录下的 [`MobileSyncFacade`] 访问 6 个 mobile
//! sync 用例；所有底层 `*UseCase` 类型保持 `pub(crate)`，不向外暴露。
//!
//! ## 暴露的动作
//!
//! 每个公开方法对应一个 use case：
//!
//! | 方法 | 对应 use case | 语义 |
//! |---|---|---|
//! | [`MobileSyncFacade::register_device`] | `RegisterMobileShortcutDeviceUseCase` | 颁发 token + 打包 `.shortcut` |
//! | [`MobileSyncFacade::revoke_device`] | `RevokeMobileDeviceUseCase` | 注销已登记设备 |
//! | [`MobileSyncFacade::list_devices`] | `ListMobileDevicesUseCase` | 列出已登记设备（不含 token_hash） |
//! | [`MobileSyncFacade::get_settings`] | `GetMobileSyncSettingsUseCase` | 读 enabled + LAN URL + install methods |
//! | [`MobileSyncFacade::update_settings`] | `UpdateMobileSyncSettingsUseCase` | 写 enabled，返回 restart_required |
//! | [`MobileSyncFacade::list_lan_interfaces`] | `ListLanInterfacesUseCase` | 列出可作为二维码 URL 的 RFC1918 网卡 |
//!
//! ## Phase 2 暂用 stub packer service
//!
//! [`crate::usecases::mobile_sync::shortcut_packer::ShortcutPackerService`]
//! 是 application 层内部 trait（不是 `uc-core` port），按 §11.4 保持
//! `pub(crate)`，因此不出现在 [`MobileSyncFacadeDeps`] 上。Phase 2 facade
//! 内部直接构造
//! [`StubShortcutPackerService`](crate::usecases::mobile_sync::shortcut_packer::StubShortcutPackerService)；
//! Phase 3 实现真实 plist + qrcode 时只需替换这一处构造。
//!
//! ## 错误暴露策略（Phase 2 简化）
//!
//! 每个 use case 自己的 `*Error` 类型直接通过 mod.rs 的 `pub use`
//! re-export，不做 mirror。这是有意为之的 YAGNI：
//!
//! 1. mobile sync 是新功能，没有"老 API 要保护"压力。
//! 2. Mirror 6 个 use case error 的工作量与读者收益不匹配 —— 所有错误都
//!    已经按 §13.1 用业务语义命名（`LabelEmpty` / `NotFound` /
//!    `LanListenerDisabled` 等），不会泄漏底层细节。
//! 3. 当 use case 内部错误真的开始演化破坏对外 API 时，再插入 mirror
//!    层即可（参考 `facade/upgrade/facade.rs` 的写法）。

use std::sync::Arc;

use uc_core::ports::{
    ClockPort, LanInterfaceProbePort, MobileDeviceRepositoryPort, MobileSyncEndpointInfoPort,
    MobileTokenMinterPort, NoncePort, SettingsPort, ShortcutDownloadTokenStorePort,
};

use crate::usecases::mobile_sync::{
    authenticate_request::AuthenticateMobileRequestUseCase,
    get_settings::GetMobileSyncSettingsUseCase, list_devices::ListMobileDevicesUseCase,
    list_lan_interfaces::ListLanInterfacesUseCase,
    register_device::RegisterMobileShortcutDeviceUseCase, revoke_device::RevokeMobileDeviceUseCase,
    shortcut_packer::StubShortcutPackerService, update_settings::UpdateMobileSyncSettingsUseCase,
};

// ── 对外类型 re-export（Phase 2 简化策略，详见模块文档）─────────────

pub use crate::usecases::mobile_sync::authenticate_request::{
    AuthenticateMobileRequestError, AuthenticateMobileRequestInput, MobileAuthHeaders,
};
pub use crate::usecases::mobile_sync::get_settings::{
    GetMobileSyncSettingsError, MobileSyncSettingsView, ShortcutInstallMethod,
    ShortcutInstallMethodOption,
};
pub use crate::usecases::mobile_sync::list_devices::{ListMobileDevicesError, MobileDeviceSummary};
pub use crate::usecases::mobile_sync::list_lan_interfaces::{
    LanInterfaceOption, ListLanInterfacesError,
};
pub use crate::usecases::mobile_sync::register_device::{
    RegisterMobileShortcutDeviceError, RegisterMobileShortcutDeviceInput,
    RegisterMobileShortcutDeviceOutput,
};
pub use crate::usecases::mobile_sync::revoke_device::{
    RevokeMobileDeviceError, RevokeMobileDeviceInput,
};
pub use crate::usecases::mobile_sync::update_settings::{
    UpdateMobileSyncSettingsError, UpdateMobileSyncSettingsInput, UpdateMobileSyncSettingsOutput,
};

// ─── Deps ───────────────────────────────────────────────────────────────

/// 构造 [`MobileSyncFacade`] 所需的端口集合。
///
/// 由 `uc-bootstrap` 在装配阶段填好；除字段顺序外没有"哪个 use case 用
/// 哪几个端口"的耦合 —— 那是 facade 内部决定的，外部只需提供全部端口。
pub struct MobileSyncFacadeDeps {
    pub clock: Arc<dyn ClockPort>,
    pub token_minter: Arc<dyn MobileTokenMinterPort>,
    pub device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    pub endpoint_info: Arc<dyn MobileSyncEndpointInfoPort>,
    pub download_tokens: Arc<dyn ShortcutDownloadTokenStorePort>,
    pub lan_interface_probe: Arc<dyn LanInterfaceProbePort>,
    pub settings: Arc<dyn SettingsPort>,
    /// LAN HTTP 鉴权防重放滑动窗口 —— 由 `AuthenticateMobileRequestUseCase`
    /// 在每次校验中调用一次。
    pub nonces: Arc<dyn NoncePort>,
}

// ─── Facade ─────────────────────────────────────────────────────────────

/// 移动端同步入口，线程安全，可放入 `Arc`。
///
/// 内部聚合 7 个 use case；所有方法都是 thin pass-through，不做跨 use
/// case 编排（按 §11.2 facade 不应再承载流程）。
pub struct MobileSyncFacade {
    register_device: RegisterMobileShortcutDeviceUseCase,
    revoke_device: RevokeMobileDeviceUseCase,
    list_devices: ListMobileDevicesUseCase,
    get_settings: GetMobileSyncSettingsUseCase,
    update_settings: UpdateMobileSyncSettingsUseCase,
    list_lan_interfaces: ListLanInterfacesUseCase,
    authenticate_request: AuthenticateMobileRequestUseCase,
}

impl MobileSyncFacade {
    /// 按 deps 构造 facade。每个 use case 独立持有它需要的端口子集 ——
    /// 端口都是 `Arc<dyn …>`，clone 不复制底层资源。
    pub fn new(deps: MobileSyncFacadeDeps) -> Self {
        let MobileSyncFacadeDeps {
            clock,
            token_minter,
            device_repo,
            endpoint_info,
            download_tokens,
            lan_interface_probe,
            settings,
            nonces,
        } = deps;

        // Phase 2 stub —— Phase 3 替换为 PlistShortcutPackerService。
        let packer = Arc::new(StubShortcutPackerService);

        Self {
            register_device: RegisterMobileShortcutDeviceUseCase::new(
                token_minter,
                device_repo.clone(),
                endpoint_info.clone(),
                download_tokens,
                packer,
                clock.clone(),
            ),
            revoke_device: RevokeMobileDeviceUseCase::new(device_repo.clone()),
            list_devices: ListMobileDevicesUseCase::new(device_repo.clone()),
            get_settings: GetMobileSyncSettingsUseCase::new(settings.clone(), endpoint_info),
            update_settings: UpdateMobileSyncSettingsUseCase::new(settings),
            list_lan_interfaces: ListLanInterfacesUseCase::new(lan_interface_probe),
            authenticate_request: AuthenticateMobileRequestUseCase::new(device_repo, nonces, clock),
        }
    }

    /// 登记一台 iPhone Shortcut 设备：颁发 token、打包 `.shortcut`、注册
    /// 一次性下载凭据。详见
    /// [`RegisterMobileShortcutDeviceUseCase`](crate::usecases::mobile_sync::register_device::RegisterMobileShortcutDeviceUseCase)。
    pub async fn register_device(
        &self,
        input: RegisterMobileShortcutDeviceInput,
    ) -> Result<RegisterMobileShortcutDeviceOutput, RegisterMobileShortcutDeviceError> {
        self.register_device.execute(input).await
    }

    /// 注销一台已登记设备。返回 `Ok(())` 表示成功；
    /// `Err(NotFound)` 表示该 device_id 已不在仓储里（UI 列表过期）。
    pub async fn revoke_device(
        &self,
        input: RevokeMobileDeviceInput,
    ) -> Result<(), RevokeMobileDeviceError> {
        self.revoke_device.execute(input).await
    }

    /// 列出已登记设备摘要。结果按"最近活跃 desc → 创建时间 desc"排序，
    /// 不包含 `token_hash`。
    pub async fn list_devices(&self) -> Result<Vec<MobileDeviceSummary>, ListMobileDevicesError> {
        self.list_devices.execute().await
    }

    /// 读移动端同步设置 + 当前 LAN URL + 可用 install methods 的合成视图。
    pub async fn get_settings(&self) -> Result<MobileSyncSettingsView, GetMobileSyncSettingsError> {
        self.get_settings.execute().await
    }

    /// 更新移动端同步设置。返回值的 `restart_required` 标记仅在 enabled
    /// 实际发生变化时为 `true`；同值重复保存为 `false` 且不写盘。
    pub async fn update_settings(
        &self,
        input: UpdateMobileSyncSettingsInput,
    ) -> Result<UpdateMobileSyncSettingsOutput, UpdateMobileSyncSettingsError> {
        self.update_settings.execute(input).await
    }

    /// 列出可作为二维码 URL 候选的本机 IPv4 LAN 接口。仅返回 RFC1918 私
    /// 有地址，按 10/8 → 172.16/12 → 192.168/16 排序。
    pub async fn list_lan_interfaces(
        &self,
    ) -> Result<Vec<LanInterfaceOption>, ListLanInterfacesError> {
        self.list_lan_interfaces.execute().await
    }

    /// 校验一条 LAN HTTP 业务请求的 Bearer + timestamp + nonce + signature
    /// 4 个 header；通过则返回对应已登记的 [`uc_core::mobile_sync::MobileDevice`]。
    /// 错误码与 SPEC §4.3 一一对应：
    /// - `InvalidTokenFormat` / `InvalidBodyHashFormat` / `InvalidNonceFormat`
    ///   / `InvalidSignatureFormat` → 400 bad_request（middleware 一般早期挡住）
    /// - `InvalidToken` → 401 invalid_token
    /// - `TimestampDrift` → 401 timestamp_drift
    /// - `NonceReplay` → 401 nonce_replay
    /// - `NonceCacheFull` → 503 nonce_cache_full
    /// - `InvalidSignature` → 401 invalid_signature
    /// - `Storage` → 500（adapter 异常，非协议错）
    pub async fn authenticate_request(
        &self,
        input: AuthenticateMobileRequestInput,
    ) -> Result<uc_core::mobile_sync::MobileDevice, AuthenticateMobileRequestError> {
        self.authenticate_request.execute(input).await
    }
}

#[cfg(test)]
mod tests {
    //! Facade 层集成测试 —— 用 in-memory port fakes 验证"deps → 6 个 use
    //! case → 对外方法"的接线没有错位。深层用例语义在各 use case 文件
    //! 已有覆盖，这里只跑一遍 happy path。

    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;

    use uc_core::mobile_sync::{
        LanEndpointInfo, LanInterface, MintedToken, MobileDevice, MobileDeviceError,
        MobileDeviceId, RegisteredDownloadToken, ShortcutDownloadToken, TokenHash,
    };
    use uc_core::ports::{
        EndpointInfoError, LanInterfaceProbeError, NonceError, ShortcutDownloadTokenError,
    };
    use uc_core::settings::model::Settings;

    // ── 复用所有 use case 已经写过的 fake 思路 ──────────────────────

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    struct StaticMinter;
    impl MobileTokenMinterPort for StaticMinter {
        fn mint_token(&self) -> MintedToken {
            MintedToken {
                raw_hex: "f".repeat(64),
                hash: TokenHash::new([7u8; 32]),
                device_id: MobileDeviceId::new("did_facade_test"),
            }
        }
    }

    #[derive(Default)]
    struct InMemoryDeviceRepo {
        devices: Mutex<Vec<MobileDevice>>,
    }
    #[async_trait]
    impl MobileDeviceRepositoryPort for InMemoryDeviceRepo {
        async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError> {
            self.devices.lock().unwrap().push(device.clone());
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
            id: &MobileDeviceId,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            Ok(self
                .devices
                .lock()
                .unwrap()
                .iter()
                .find(|d| d.device_id == *id)
                .cloned())
        }
        async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError> {
            Ok(self.devices.lock().unwrap().clone())
        }
        async fn delete(&self, id: &MobileDeviceId) -> Result<bool, MobileDeviceError> {
            let mut devs = self.devices.lock().unwrap();
            let before = devs.len();
            devs.retain(|d| d.device_id != *id);
            Ok(devs.len() < before)
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

    struct FixedEndpoint;
    #[async_trait]
    impl MobileSyncEndpointInfoPort for FixedEndpoint {
        async fn current_lan_endpoint(&self) -> Result<Option<LanEndpointInfo>, EndpointInfoError> {
            Ok(Some(LanEndpointInfo {
                url: "http://192.168.1.5:42720".into(),
            }))
        }
    }

    #[derive(Default)]
    struct StubDownloadTokens {
        next: Mutex<u64>,
    }
    #[async_trait]
    impl ShortcutDownloadTokenStorePort for StubDownloadTokens {
        async fn register(
            &self,
            _: MobileDeviceId,
            _: Vec<u8>,
            ttl_ms: i64,
        ) -> Result<RegisteredDownloadToken, ShortcutDownloadTokenError> {
            let mut n = self.next.lock().unwrap();
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

    struct StubLanProbe;
    #[async_trait]
    impl LanInterfaceProbePort for StubLanProbe {
        async fn list_interfaces(&self) -> Result<Vec<LanInterface>, LanInterfaceProbeError> {
            Ok(vec![LanInterface {
                name: "en0".into(),
                ipv4: std::net::Ipv4Addr::new(192, 168, 1, 5),
                is_loopback: false,
            }])
        }
    }

    /// 永远把任意 nonce 视为新 —— register / revoke / settings 等流程
    /// 不走鉴权，本测试中 facade 只是凑构造参数，不实际调到。
    struct AcceptAllNonces;
    #[async_trait]
    impl uc_core::ports::NoncePort for AcceptAllNonces {
        async fn record_if_new(
            &self,
            _nonce: &str,
            _observed_at_ms: i64,
        ) -> Result<bool, NonceError> {
            Ok(true)
        }
    }

    #[derive(Default)]
    struct InMemorySettings {
        current: Mutex<Option<Settings>>,
    }
    #[async_trait]
    impl SettingsPort for InMemorySettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(self
                .current
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(Settings::default))
        }
        async fn save(&self, settings: &Settings) -> anyhow::Result<()> {
            *self.current.lock().unwrap() = Some(settings.clone());
            Ok(())
        }
    }

    fn build_facade() -> MobileSyncFacade {
        MobileSyncFacade::new(MobileSyncFacadeDeps {
            clock: Arc::new(FixedClock(1_000)),
            token_minter: Arc::new(StaticMinter),
            device_repo: Arc::new(InMemoryDeviceRepo::default()),
            endpoint_info: Arc::new(FixedEndpoint),
            download_tokens: Arc::new(StubDownloadTokens::default()),
            lan_interface_probe: Arc::new(StubLanProbe),
            nonces: Arc::new(AcceptAllNonces),
            settings: Arc::new(InMemorySettings::default()),
        })
    }

    #[tokio::test]
    async fn end_to_end_register_then_list_then_revoke() {
        let facade = build_facade();

        // 0. 列设备：起始为空。
        assert!(facade.list_devices().await.unwrap().is_empty());

        // 1. 登记。
        let out = facade
            .register_device(RegisterMobileShortcutDeviceInput {
                label: "我的 iPhone".into(),
            })
            .await
            .expect("register ok");
        assert_eq!(out.device.label, "我的 iPhone");
        assert!(out
            .download_url
            .starts_with("http://192.168.1.5:42720/mobile/v1/shortcut/install?dt="));

        // 2. 列设备：拿到刚登记的那台。
        let listed = facade.list_devices().await.unwrap();
        assert_eq!(listed.len(), 1);
        let device_id = listed[0].device_id.clone();

        // 3. 注销。
        facade
            .revoke_device(RevokeMobileDeviceInput {
                device_id: device_id.clone(),
            })
            .await
            .expect("revoke ok");

        // 4. 注销之后再列：空了。
        assert!(facade.list_devices().await.unwrap().is_empty());

        // 5. 重复 revoke：返回 NotFound。
        let err = facade
            .revoke_device(RevokeMobileDeviceInput { device_id })
            .await
            .unwrap_err();
        assert!(matches!(err, RevokeMobileDeviceError::NotFound(_)));
    }

    #[tokio::test]
    async fn settings_round_trip_through_facade() {
        let facade = build_facade();

        // 默认 disabled。
        let v0 = facade.get_settings().await.unwrap();
        assert!(!v0.enabled);
        // FixedEndpoint 始终有 url，所以这里也能拿到 LAN URL。
        assert_eq!(
            v0.current_lan_url.as_deref(),
            Some("http://192.168.1.5:42720")
        );

        // 改 enabled = true → restart_required 应为 true。
        let upd = facade
            .update_settings(UpdateMobileSyncSettingsInput { enabled: true })
            .await
            .unwrap();
        assert!(upd.enabled);
        assert!(upd.restart_required);

        // 再读：enabled 已生效。
        let v1 = facade.get_settings().await.unwrap();
        assert!(v1.enabled);

        // 同值再保存：restart_required 应 false。
        let upd_noop = facade
            .update_settings(UpdateMobileSyncSettingsInput { enabled: true })
            .await
            .unwrap();
        assert!(!upd_noop.restart_required);
    }

    #[tokio::test]
    async fn list_lan_interfaces_returns_filtered_options() {
        let facade = build_facade();
        let opts = facade.list_lan_interfaces().await.unwrap();
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].ipv4, "192.168.1.5");
    }
}
