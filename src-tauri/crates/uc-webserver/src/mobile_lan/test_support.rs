//! `mobile_lan` 模块共享的测试装配。
//!
//! `routes.rs` 与 `middleware.rs` 都需要一份"已注入一台已知凭据设备"的
//! [`MobileSyncFacade`], 来跑 401 / 404 / happy path / extension 注入这
//! 些断言。`MobileSyncFacade::new` 的 7 个 ports 都用 in-process fake 实
//! 装,本模块集中维护这套最小 fake 装配 + Basic Auth 头工具,让两边的
//! 测试模块直接拿去用,不必各自重写。
//!
//! ## 设计取舍
//!
//! 1. **不依赖 `uc-infra`**。webserver crate 的依赖图禁止下沉到 infra
//!    具体实现(`uc-application/AGENTS.md` §6.1 等同适用), 所以这里用本
//!    地 `FakeHasher`(PHC 形态固定为 `phc:<password>`)。
//! 2. **PHC 形状故意可读**。`phc:<password>` 让真机调试 / 日志印 PHC
//!    时一眼能看出"测试桩 vs 真 Argon2 输出", 真生产 PHC 全是 base64,
//!    形态对比强烈。
//! 3. **device_id 固定为 `did_seed`**。让测试断言 device_id 时不必读出
//!    随机 minter 输出。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use base64::Engine;

use uc_application::facade::{MobileSyncFacade, MobileSyncFacadeDeps};
use uc_core::mobile_sync::{
    LanEndpointInfo, LanInterface, MintedCredentials, MobileClientType, MobileDevice,
    MobileDeviceError, MobileDeviceId,
};
use uc_core::ports::{
    ClockPort, EndpointInfoError, LanInterfaceProbeError, LanInterfaceProbePort,
    MobileCredentialsMinterPort, MobileDeviceRepositoryPort, MobileSyncEndpointInfoPort,
    PasswordHasherError, PasswordHasherPort, SettingsPort,
};
use uc_core::settings::model::Settings;

/// 构造一份只装 1 台已登记设备的 [`MobileSyncFacade`], 凭据是
/// `(username, password)`, PHC 形态固定为 `phc:{password}`。
///
/// 调用方拿到的 facade 已经过 register 流程,可以直接用真实
/// `Authorization: Basic <base64(username:password)>` 跑鉴权。
pub(crate) async fn build_facade_with_seeded_device(
    username: &str,
    password: &str,
) -> Arc<MobileSyncFacade> {
    use std::net::Ipv4Addr;

    struct FixedClock;
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            1_000
        }
    }

    struct StaticMinter;
    impl MobileCredentialsMinterPort for StaticMinter {
        fn mint_credentials(&self) -> MintedCredentials {
            MintedCredentials {
                username: "mobile_unused".into(),
                password: "unused".into(),
                password_hash: "phc:unused".into(),
                device_id: MobileDeviceId::new("did_unused"),
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
            _: &MobileDeviceId,
        ) -> Result<Option<MobileDevice>, MobileDeviceError> {
            Ok(None)
        }
        async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError> {
            Ok(self.devices.lock().unwrap().clone())
        }
        async fn delete(&self, _: &MobileDeviceId) -> Result<bool, MobileDeviceError> {
            Ok(false)
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
            Ok(None)
        }
    }

    struct StubLanProbe;
    #[async_trait]
    impl LanInterfaceProbePort for StubLanProbe {
        async fn list_interfaces(&self) -> Result<Vec<LanInterface>, LanInterfaceProbeError> {
            Ok(vec![LanInterface {
                name: "en0".into(),
                ipv4: Ipv4Addr::new(192, 168, 1, 5),
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

    let repo = Arc::new(InMemoryDeviceRepo::default());
    repo.save(&MobileDevice {
        device_id: MobileDeviceId::new("did_seed"),
        label: "iPhone".into(),
        client_type: MobileClientType::IosShortcut,
        username: username.into(),
        password_hash: format!("phc:{password}"),
        created_at_ms: 1,
        last_seen_at_ms: None,
        last_seen_ip: None,
        reported_name: None,
        reported_os: None,
    })
    .await
    .unwrap();

    Arc::new(MobileSyncFacade::new(MobileSyncFacadeDeps {
        clock: Arc::new(FixedClock),
        credentials_minter: Arc::new(StaticMinter),
        password_hasher: Arc::new(FakeHasher),
        device_repo: repo,
        endpoint_info: Arc::new(FixedEndpoint),
        lan_interface_probe: Arc::new(StubLanProbe),
        settings: Arc::new(InMemorySettings::default()),
    }))
}

/// 拼一份 `Authorization: basic <base64(user:pass)>` header 值。
///
/// scheme 用 SyncClipboard 客户端实际下发的小写形式,验证 RFC 不区分
/// 大小写解析的行为(本模块两个测试模块共用)。
pub(crate) fn auth_header(username: &str, password: &str) -> String {
    let payload = BASE64_STD.encode(format!("{username}:{password}"));
    format!("basic {payload}")
}
