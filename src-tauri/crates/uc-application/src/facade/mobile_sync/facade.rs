//! [`MobileSyncFacade`] —— 移动端同步功能的应用层入口(v3 SyncClipboard 兼容版)。
//!
//! 按 `uc-application/AGENTS.md` §11.4, 外部 crate(bootstrap / daemon /
//! tauri / cli)只能通过本目录下的 [`MobileSyncFacade`] 访问 mobile sync
//! 用例;所有底层 `*UseCase` 类型保持 `pub(crate)`, 不向外暴露。
//!
//! ## 暴露的动作
//!
//! 每个公开方法对应一个 use case:
//!
//! | 方法 | 对应 use case | 语义 |
//! |---|---|---|
//! | [`MobileSyncFacade::register_device`] | `RegisterMobileShortcutDeviceUseCase` | 颁发 (username,password) + 渲染 install URL 二维码 |
//! | [`MobileSyncFacade::revoke_device`] | `RevokeMobileDeviceUseCase` | 注销已登记设备 |
//! | [`MobileSyncFacade::list_devices`] | `ListMobileDevicesUseCase` | 列出已登记设备(不含 password_hash / username) |
//! | [`MobileSyncFacade::get_settings`] | `GetMobileSyncSettingsUseCase` | 读 enabled + LAN URL + install methods |
//! | [`MobileSyncFacade::update_settings`] | `UpdateMobileSyncSettingsUseCase` | 写 enabled, 返回 restart_required |
//! | [`MobileSyncFacade::list_lan_interfaces`] | `ListLanInterfacesUseCase` | 列出可作为二维码 URL 的 RFC1918 网卡 |
//! | [`MobileSyncFacade::authenticate_basic`] | `AuthenticateBasicAuthUseCase` | LAN HTTP 路由用:校验 Basic Auth 头 |
//!
//! ## 错误暴露策略
//!
//! 每个 use case 自己的 `*Error` 类型直接通过 mod.rs 的 `pub use`
//! re-export, 不做 mirror。错误都已经按 §13.1 用业务语义命名
//! (`LabelEmpty` / `NotFound` / `LanListenerDisabled` / `InvalidCredentials`
//! 等), 不会泄漏底层细节。

use std::sync::Arc;

use uc_core::ports::{
    ClockPort, LanInterfaceProbePort, MobileCredentialsMinterPort, MobileDeviceRepositoryPort,
    MobileSyncEndpointInfoPort, PasswordHasherPort, SettingsPort,
};

use crate::usecases::mobile_sync::{
    authenticate_basic::AuthenticateBasicAuthUseCase, clipboard_doc::ClipboardDocStub,
    get_settings::GetMobileSyncSettingsUseCase, list_devices::ListMobileDevicesUseCase,
    list_lan_interfaces::ListLanInterfacesUseCase,
    register_device::RegisterMobileShortcutDeviceUseCase, revoke_device::RevokeMobileDeviceUseCase,
    update_settings::UpdateMobileSyncSettingsUseCase,
};

// ── 对外类型 re-export ─────────────────────────────────────────────────

pub use crate::usecases::mobile_sync::authenticate_basic::{
    AuthenticateBasicAuthError, AuthenticateBasicAuthInput, AuthenticatedDevice,
};
pub use crate::usecases::mobile_sync::clipboard_doc::{
    SyncClipboardError, SyncClipboardItemType, SyncClipboardMeta,
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
// `SYNC_CLIPBOARD_EX_INSTALL_URL` 是 const 值, rustc 在 `pub use {}` 组里
// 与类型混合时会误报 unused; 单独一行 re-export 让它独立绑定, 避免 warning
// (功能上等价)。
pub use crate::usecases::mobile_sync::register_device::SYNC_CLIPBOARD_EX_INSTALL_URL;
pub use crate::usecases::mobile_sync::revoke_device::{
    RevokeMobileDeviceError, RevokeMobileDeviceInput,
};
pub use crate::usecases::mobile_sync::update_settings::{
    UpdateMobileSyncSettingsError, UpdateMobileSyncSettingsInput, UpdateMobileSyncSettingsOutput,
};

// ─── Deps ───────────────────────────────────────────────────────────────

/// 构造 [`MobileSyncFacade`] 所需的端口集合。
///
/// 由 `uc-bootstrap` 在装配阶段填好;除字段顺序外没有"哪个 use case 用
/// 哪几个端口"的耦合 —— 那是 facade 内部决定的, 外部只需提供全部端口。
pub struct MobileSyncFacadeDeps {
    pub clock: Arc<dyn ClockPort>,
    pub credentials_minter: Arc<dyn MobileCredentialsMinterPort>,
    pub password_hasher: Arc<dyn PasswordHasherPort>,
    pub device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    pub endpoint_info: Arc<dyn MobileSyncEndpointInfoPort>,
    pub lan_interface_probe: Arc<dyn LanInterfaceProbePort>,
    pub settings: Arc<dyn SettingsPort>,
}

// ─── Facade ─────────────────────────────────────────────────────────────

/// 移动端同步入口, 线程安全, 可放入 `Arc`。
///
/// 内部聚合 7 个 use case + 1 个 Phase 3 stub clipboard 状态;所有方法都
/// 是 thin pass-through, 不做跨 use case 编排(按 §11.2 facade 不应再承载
/// 流程)。
pub struct MobileSyncFacade {
    register_device: RegisterMobileShortcutDeviceUseCase,
    revoke_device: RevokeMobileDeviceUseCase,
    list_devices: ListMobileDevicesUseCase,
    get_settings: GetMobileSyncSettingsUseCase,
    update_settings: UpdateMobileSyncSettingsUseCase,
    list_lan_interfaces: ListLanInterfacesUseCase,
    authenticate_basic: AuthenticateBasicAuthUseCase,
    /// Phase 3 stub: 进程内 Mutex 状态承接 SyncClipboard 协议读写;Phase 5
    /// 替换为对接 `ApplicationFacade::clipboard_*` 的实装。
    clipboard_doc_stub: ClipboardDocStub,
}

impl MobileSyncFacade {
    /// 按 deps 构造 facade。每个 use case 独立持有它需要的端口子集 ——
    /// 端口都是 `Arc<dyn …>`, clone 不复制底层资源。
    pub fn new(deps: MobileSyncFacadeDeps) -> Self {
        let MobileSyncFacadeDeps {
            clock,
            credentials_minter,
            password_hasher,
            device_repo,
            endpoint_info,
            lan_interface_probe,
            settings,
        } = deps;

        Self {
            register_device: RegisterMobileShortcutDeviceUseCase::new(
                credentials_minter,
                device_repo.clone(),
                settings.clone(),
                clock,
            ),
            revoke_device: RevokeMobileDeviceUseCase::new(device_repo.clone()),
            list_devices: ListMobileDevicesUseCase::new(device_repo.clone()),
            get_settings: GetMobileSyncSettingsUseCase::new(settings.clone(), endpoint_info),
            update_settings: UpdateMobileSyncSettingsUseCase::new(settings),
            list_lan_interfaces: ListLanInterfacesUseCase::new(lan_interface_probe),
            authenticate_basic: AuthenticateBasicAuthUseCase::new(device_repo, password_hasher),
            clipboard_doc_stub: ClipboardDocStub::new(),
        }
    }

    /// 登记一台 iPhone Shortcut 设备:颁发 (username, password) Basic Auth
    /// 凭据 + 渲染 SyncClipboard install URL 的二维码。详见
    /// [`RegisterMobileShortcutDeviceUseCase`](crate::usecases::mobile_sync::register_device::RegisterMobileShortcutDeviceUseCase)。
    pub async fn register_device(
        &self,
        input: RegisterMobileShortcutDeviceInput,
    ) -> Result<RegisterMobileShortcutDeviceOutput, RegisterMobileShortcutDeviceError> {
        self.register_device.execute(input).await
    }

    /// 注销一台已登记设备。返回 `Ok(())` 表示成功;
    /// `Err(NotFound)` 表示该 device_id 已不在仓储里(UI 列表过期)。
    pub async fn revoke_device(
        &self,
        input: RevokeMobileDeviceInput,
    ) -> Result<(), RevokeMobileDeviceError> {
        self.revoke_device.execute(input).await
    }

    /// 列出已登记设备摘要。结果按"最近活跃 desc → 创建时间 desc"排序,
    /// 不包含 `password_hash` / `username`。
    pub async fn list_devices(&self) -> Result<Vec<MobileDeviceSummary>, ListMobileDevicesError> {
        self.list_devices.execute().await
    }

    /// 读移动端同步设置 + 当前 LAN URL + 可用 install methods 的合成视图。
    pub async fn get_settings(&self) -> Result<MobileSyncSettingsView, GetMobileSyncSettingsError> {
        self.get_settings.execute().await
    }

    /// 更新移动端同步设置。返回值的 `restart_required` 标记仅在 enabled
    /// 实际发生变化时为 `true`;同值重复保存为 `false` 且不写盘。
    pub async fn update_settings(
        &self,
        input: UpdateMobileSyncSettingsInput,
    ) -> Result<UpdateMobileSyncSettingsOutput, UpdateMobileSyncSettingsError> {
        self.update_settings.execute(input).await
    }

    /// 列出可作为二维码 URL 候选的本机 IPv4 LAN 接口。仅返回 RFC1918 私
    /// 有地址, 按 10/8 → 172.16/12 → 192.168/16 排序。
    pub async fn list_lan_interfaces(
        &self,
    ) -> Result<Vec<LanInterfaceOption>, ListLanInterfacesError> {
        self.list_lan_interfaces.execute().await
    }

    /// 校验 LAN HTTP 请求的 `Authorization: basic ...` 头。详见
    /// [`AuthenticateBasicAuthUseCase`](crate::usecases::mobile_sync::authenticate_basic::AuthenticateBasicAuthUseCase)。
    pub async fn authenticate_basic(
        &self,
        input: AuthenticateBasicAuthInput,
    ) -> Result<AuthenticatedDevice, AuthenticateBasicAuthError> {
        self.authenticate_basic.execute(input).await
    }

    // ─── Phase 3 stub: SyncClipboard 协议业务方法(Phase 5 接真实现) ───

    /// `GET /SyncClipboard.json` 业务出口:返回最近一次 PUT 的元数据。
    /// 没任何 PUT 历史返回 `NotFound`(路由翻 404)。
    pub async fn get_latest_sync_doc(&self) -> Result<SyncClipboardMeta, SyncClipboardError> {
        self.clipboard_doc_stub.get_latest_doc().await
    }

    /// `PUT /SyncClipboard.json` 业务出口:接收新元数据, daemon 自动算
    /// SHA-256 填进 hash, 返回最终入库版本。
    pub async fn put_sync_doc(
        &self,
        meta: SyncClipboardMeta,
    ) -> Result<SyncClipboardMeta, SyncClipboardError> {
        self.clipboard_doc_stub.put_doc(meta).await
    }

    /// `GET /file/{dataName}` 业务出口:返回 (mime, bytes) 或 `NotFound`。
    pub async fn get_clipboard_file(
        &self,
        data_name: &str,
    ) -> Result<(String, Vec<u8>), SyncClipboardError> {
        self.clipboard_doc_stub.get_file(data_name).await
    }

    /// `PUT /file/{dataName}` 业务出口:接收附件二进制 + 它的 mime。
    pub async fn put_clipboard_file(
        &self,
        data_name: String,
        mime: String,
        bytes: Vec<u8>,
    ) -> Result<(), SyncClipboardError> {
        self.clipboard_doc_stub
            .put_file(data_name, mime, bytes)
            .await
    }
}

#[cfg(test)]
mod tests {
    //! Facade 层集成测试 —— 用 in-memory port fakes 验证"deps → 7 个 use
    //! case → 对外方法"的接线没有错位。深层用例语义在各 use case 文件
    //! 已有覆盖, 这里只跑一遍 happy path。

    use super::*;

    use std::sync::Mutex;

    use async_trait::async_trait;
    use base64::Engine;

    use uc_core::mobile_sync::{
        LanEndpointInfo, LanInterface, MintedCredentials, MobileDevice, MobileDeviceError,
        MobileDeviceId,
    };
    use uc_core::ports::{EndpointInfoError, LanInterfaceProbeError, PasswordHasherError};
    use uc_core::settings::model::Settings;

    struct FixedClock(i64);
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

    struct StaticMinter;
    impl MobileCredentialsMinterPort for StaticMinter {
        fn mint_credentials(&self) -> MintedCredentials {
            MintedCredentials {
                username: "mobile_facade01".into(),
                password: "facade-test-password-22".into(),
                password_hash: "$argon2id$test$facade".into(),
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
        async fn find_by_username(
            &self,
            username: &str,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            Ok(self
                .devices
                .lock()
                .unwrap()
                .iter()
                .find(|d| d.username == username)
                .cloned())
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

    /// fake hasher: 把 password_hash 视为 `phc:<password>`, verify 比较即可。
    /// happy path 用真值 `$argon2id$test$facade` 不会 verify 通过,
    /// 测试里直接用 fake 重写 password_hash 的设备来验证 authenticate。
    struct FakeHasher;
    #[async_trait]
    impl PasswordHasherPort for FakeHasher {
        async fn hash(&self, password: &str) -> Result<String, PasswordHasherError> {
            Ok(format!("phc:{password}"))
        }
        async fn verify(&self, password: &str, phc: &str) -> Result<bool, PasswordHasherError> {
            Ok(phc == format!("phc:{password}"))
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
            credentials_minter: Arc::new(StaticMinter),
            password_hasher: Arc::new(FakeHasher),
            device_repo: Arc::new(InMemoryDeviceRepo::default()),
            endpoint_info: Arc::new(FixedEndpoint),
            lan_interface_probe: Arc::new(StubLanProbe),
            settings: Arc::new(InMemorySettings::default()),
        })
    }

    #[tokio::test]
    async fn end_to_end_register_then_list_then_revoke() {
        let facade = build_facade();

        // 0. 列设备:起始为空。
        assert!(facade.list_devices().await.unwrap().is_empty());

        // 0.5. 先把 LAN advertise 配好, 否则 register_device 会拒绝。
        facade
            .update_settings(UpdateMobileSyncSettingsInput {
                enabled: Some(true),
                lan_listen_enabled: Some(true),
                lan_advertise_ip: Some(Some("192.168.1.5".into())),
                lan_port: Some(Some(42720)),
            })
            .await
            .expect("update_settings ok");

        // 1. 登记。
        let out = facade
            .register_device(RegisterMobileShortcutDeviceInput {
                label: "我的 iPhone".into(),
            })
            .await
            .expect("register ok");
        assert_eq!(out.device.label, "我的 iPhone");
        assert_eq!(out.username, "mobile_facade01");
        assert_eq!(out.base_url, "http://192.168.1.5:42720");
        assert_eq!(out.install_url, SYNC_CLIPBOARD_EX_INSTALL_URL);
        assert!(!out.qr_code_png_bytes.is_empty());

        // 2. 列设备:拿到刚登记的那台。
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

        // 4. 注销之后再列:空了。
        assert!(facade.list_devices().await.unwrap().is_empty());

        // 5. 重复 revoke:返回 NotFound。
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
        // FixedEndpoint 始终有 url, 所以这里也能拿到 LAN URL。
        assert_eq!(
            v0.current_lan_url.as_deref(),
            Some("http://192.168.1.5:42720")
        );

        // 改 enabled = true → restart_required 应为 true。
        let upd = facade
            .update_settings(UpdateMobileSyncSettingsInput {
                enabled: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(upd.enabled);
        assert!(upd.restart_required);

        // 再读:enabled 已生效。
        let v1 = facade.get_settings().await.unwrap();
        assert!(v1.enabled);

        // 同值再保存:restart_required 应 false。
        let upd_noop = facade
            .update_settings(UpdateMobileSyncSettingsInput {
                enabled: Some(true),
                ..Default::default()
            })
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

    #[tokio::test]
    async fn authenticate_basic_round_trips_through_facade() {
        let facade = build_facade();

        // 先把一台用 FakeHasher 能验证通过的设备塞进 repo —— 不能用
        // register flow, 因为 register 用 StaticMinter 出的 password_hash
        // 是 "$argon2id$test$facade" 不可能被 FakeHasher 通过。直接构造一
        // 台带 phc:<pw> 形态的设备, 通过 in-process 仓储插入。
        //
        // 这里我们重新构造一个独立 facade, 共享同一个 repo, 让 device_repo
        // 在两个 use case 之间能"看到同一份数据"。
        //
        // 实际部署时 register 用真 minter + 真 hasher, 出来的 phc 能被
        // verify 通过 —— 这一对配对契约由 OsRngCredentialsMinter +
        // Argon2idPasswordHasher 在 uc-infra 层联合保证(infra crate 已有
        // round-trip 测试)。

        // 把 FakeHasher 兼容的设备直接 push。
        let direct_device = MobileDevice {
            device_id: MobileDeviceId::new("did_auth"),
            label: "iPhone".into(),
            client_type: uc_core::mobile_sync::MobileClientType::IosShortcut,
            username: "mobile_alice".into(),
            password_hash: "phc:wonderland".into(), // FakeHasher 兼容
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        };
        let repo = Arc::new(InMemoryDeviceRepo::default());
        repo.save(&direct_device).await.unwrap();
        let local = MobileSyncFacade::new(MobileSyncFacadeDeps {
            clock: Arc::new(FixedClock(1_000)),
            credentials_minter: Arc::new(StaticMinter),
            password_hasher: Arc::new(FakeHasher),
            device_repo: repo,
            endpoint_info: Arc::new(FixedEndpoint),
            lan_interface_probe: Arc::new(StubLanProbe),
            settings: Arc::new(InMemorySettings::default()),
        });

        // happy path
        let header = format!(
            "basic {}",
            base64::engine::general_purpose::STANDARD.encode("mobile_alice:wonderland")
        );
        let out = local
            .authenticate_basic(AuthenticateBasicAuthInput {
                authorization_header: header,
            })
            .await
            .unwrap();
        assert_eq!(out.device.username, "mobile_alice");

        // 错密码 → 401
        let bad = format!(
            "basic {}",
            base64::engine::general_purpose::STANDARD.encode("mobile_alice:wrongpw")
        );
        let err = local
            .authenticate_basic(AuthenticateBasicAuthInput {
                authorization_header: bad,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AuthenticateBasicAuthError::InvalidCredentials
        ));

        // 静默掉 facade 顶层 register 没用过的字段(避免 dead code 警告)。
        let _ = facade;
    }
}
