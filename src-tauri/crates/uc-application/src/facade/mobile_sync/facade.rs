//! [`MobileSyncFacade`] —— 移动端同步功能的应用层入口(P5a.6 起接真实现)。
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
//! | [`MobileSyncFacade::list_devices`] | `ListMobileDevicesUseCase` | 列出已登记设备(不含 password_hash) |
//! | [`MobileSyncFacade::rotate_password`] | `RotateMobilePasswordUseCase` | 给已登记设备换一份新密码(返回一次性明文) |
//! | [`MobileSyncFacade::get_settings`] | `GetMobileSyncSettingsUseCase` | 读 enabled + LAN URL + install methods |
//! | [`MobileSyncFacade::update_settings`] | `UpdateMobileSyncSettingsUseCase` | 写 enabled, 返回 restart_required |
//! | [`MobileSyncFacade::list_lan_interfaces`] | `ListLanInterfacesUseCase` | 列出可作为二维码 URL 的 RFC1918 网卡 |
//! | [`MobileSyncFacade::authenticate_basic`] | `AuthenticateBasicAuthUseCase` | LAN HTTP 路由用:校验 Basic Auth 头 |
//! | [`MobileSyncFacade::get_latest_sync_doc`] | `GetLatestMobileSyncDocUseCase` | `GET /SyncClipboard.json` |
//! | [`MobileSyncFacade::put_sync_doc`] | `ApplyIncomingMobileClipUseCase` (`SyncDoc`) | `PUT /SyncClipboard.json` |
//! | [`MobileSyncFacade::get_clipboard_file`] | `GetMobileSyncFileUseCase` | `GET /file/{name}` |
//! | [`MobileSyncFacade::put_clipboard_file`] | `ApplyIncomingMobileClipUseCase` (`BufferFile`) | `PUT /file/{name}` |
//!
//! ## 错误暴露策略
//!
//! 每个 use case 自己的 `*Error` 类型直接通过 mod.rs 的 `pub use`
//! re-export, 不做 mirror。错误都已经按 §13.1 用业务语义命名
//! (`LabelEmpty` / `NotFound` / `LanListenerDisabled` / `InvalidCredentials`
//! / `Inbound` / `EncodeFailed` / `DecodeFailed` 等), 不会泄漏底层细节。
//!
//! ## P5a.6 改动要点
//!
//! - 删除 Phase 3 子步骤 5f 的 `ClipboardDocStub`(进程内 Mutex 状态);
//! - 4 个 SyncClipboard 协议方法分别接到 `apply_incoming` /
//!   `get_latest_doc` / `get_file` 三个真实 use case;
//! - PUT 方法签名增加 `source_device_id: MobileDeviceId` 参数, 由
//!   webserver middleware 注入的 [`AuthenticatedDevice`] 提供;
//! - GET 路径通过 `LatestClipboardSnapshotAdapter` 组合 5 个剪贴板 port
//!   读最新 paste-priority rep,真业务,不再走桩。

use std::sync::Arc;

use uc_core::mobile_sync::MobileDeviceId;
use uc_core::ports::mobile_sync::LatestClipboardSnapshotPort;
use uc_core::ports::{
    ClockPort, LanInterfaceProbePort, MobileCredentialsMinterPort, MobileDeviceRepositoryPort,
    MobileFileStagingPort, MobileSyncEndpointInfoPort, PasswordHasherPort, SettingsPort,
};

use crate::usecases::clipboard_sync::apply_inbound::ApplyInboundClipboardUseCase;
use crate::usecases::mobile_sync::{
    apply_incoming::ApplyIncomingMobileClipUseCase,
    authenticate_basic::AuthenticateBasicAuthUseCase, get_file::GetMobileSyncFileUseCase,
    get_latest_doc::GetLatestMobileSyncDocUseCase, get_settings::GetMobileSyncSettingsUseCase,
    latest_snapshot_adapter::LatestClipboardSnapshotAdapter,
    list_devices::ListMobileDevicesUseCase, list_lan_interfaces::ListLanInterfacesUseCase,
    register_device::RegisterMobileShortcutDeviceUseCase, revoke_device::RevokeMobileDeviceUseCase,
    rotate_password::RotateMobilePasswordUseCase, update_settings::UpdateMobileSyncSettingsUseCase,
};

// ── 对外类型 re-export ─────────────────────────────────────────────────

pub use crate::usecases::mobile_sync::apply_incoming::{
    ApplyIncomingMobileClipError, ApplyIncomingMobileClipInput, ApplyIncomingMobileClipOutcome,
    IncomingMobileBuffer, IncomingMobileClipEvent,
};
pub use crate::usecases::mobile_sync::authenticate_basic::{
    AuthenticateBasicAuthError, AuthenticateBasicAuthInput, AuthenticatedDevice,
};
pub use crate::usecases::mobile_sync::clipboard_doc::{SyncClipboardItemType, SyncClipboardMeta};
pub use crate::usecases::mobile_sync::get_file::{GetMobileSyncFileError, GetMobileSyncFileOutput};
pub use crate::usecases::mobile_sync::get_latest_doc::GetLatestMobileSyncDocError;
pub use crate::usecases::mobile_sync::get_settings::{
    GetMobileSyncSettingsError, MobileSyncSettingsView, ShortcutInstallMethod,
    ShortcutInstallMethodOption,
};
pub use crate::usecases::mobile_sync::latest_snapshot_adapter::MobileSyncSnapshotPorts;
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
pub use crate::usecases::mobile_sync::rotate_password::{
    RotateMobilePasswordError, RotateMobilePasswordInput, RotateMobilePasswordOutput,
};
pub use crate::usecases::mobile_sync::update_settings::{
    UpdateMobileSyncSettingsError, UpdateMobileSyncSettingsInput, UpdateMobileSyncSettingsOutput,
};

// ─── Deps ───────────────────────────────────────────────────────────────

/// 构造 [`MobileSyncFacade`] 所需的端口集合。
///
/// 由 `uc-bootstrap` 在装配阶段填好。除字段顺序外没有"哪个 use case 用
/// 哪几个端口"的耦合 —— 那是 facade 内部决定的, 外部只需提供全部端口。
///
/// `apply_inbound` 与 `incoming_buffer` / `snapshot_ports` 是 P5a.6 引入
/// 的新字段:
/// - `apply_inbound`:PUT 路径的真实剪贴板入站 use case 实例。bootstrap
///   把它装配一份后,同时喂给本 facade 与 `InboundClipboardFacade` 共享。
/// - `incoming_buffer`:两步 PUT 协议(file → json)之间的字节暂存 ——
///   bootstrap 端 `Arc::new(IncomingMobileBuffer::new())` 即可,无外部资源。
/// - `snapshot_ports`:GET 路径用 `LatestClipboardSnapshotAdapter` 组合的
///   5 个剪贴板 port,facade 内部装配成 `LatestClipboardSnapshotPort`。
pub struct MobileSyncFacadeDeps {
    pub clock: Arc<dyn ClockPort>,
    pub credentials_minter: Arc<dyn MobileCredentialsMinterPort>,
    pub password_hasher: Arc<dyn PasswordHasherPort>,
    pub device_repo: Arc<dyn MobileDeviceRepositoryPort>,
    pub endpoint_info: Arc<dyn MobileSyncEndpointInfoPort>,
    pub lan_interface_probe: Arc<dyn LanInterfaceProbePort>,
    pub settings: Arc<dyn SettingsPort>,
    pub apply_inbound: Arc<ApplyInboundClipboardUseCase>,
    pub incoming_buffer: Arc<IncomingMobileBuffer>,
    /// `MobileFileStagingPort` 实例(P5a.3.5):File 类型入站时把裸字节物
    /// 化到 cache_dir,产出可拼 file-list rep 的 `file:///...` URI。
    /// daemon / CLI fallback 都注入 `FilesystemMobileFileStaging`(uc-infra),
    /// 测试场景可注入内存 fake。
    pub file_staging: Arc<dyn MobileFileStagingPort>,
    pub snapshot_ports: MobileSyncSnapshotPorts,
}

// ─── Facade ─────────────────────────────────────────────────────────────

/// 移动端同步入口, 线程安全, 可放入 `Arc`。
///
/// 内部聚合 10 个 use case;所有方法都是 thin pass-through, 不做跨
/// use case 编排(按 §11.2 facade 不应再承载流程)。
pub struct MobileSyncFacade {
    register_device: RegisterMobileShortcutDeviceUseCase,
    revoke_device: RevokeMobileDeviceUseCase,
    list_devices: ListMobileDevicesUseCase,
    rotate_password: RotateMobilePasswordUseCase,
    get_settings: GetMobileSyncSettingsUseCase,
    update_settings: UpdateMobileSyncSettingsUseCase,
    list_lan_interfaces: ListLanInterfacesUseCase,
    authenticate_basic: AuthenticateBasicAuthUseCase,
    apply_incoming: ApplyIncomingMobileClipUseCase,
    get_latest_doc: GetLatestMobileSyncDocUseCase,
    get_file: GetMobileSyncFileUseCase,
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
            apply_inbound,
            incoming_buffer,
            file_staging,
            snapshot_ports,
        } = deps;

        let snapshot_port: Arc<dyn LatestClipboardSnapshotPort> =
            Arc::new(LatestClipboardSnapshotAdapter::new(snapshot_ports));

        Self {
            register_device: RegisterMobileShortcutDeviceUseCase::new(
                credentials_minter.clone(),
                password_hasher.clone(),
                device_repo.clone(),
                settings.clone(),
                clock.clone(),
                lan_interface_probe.clone(),
            ),
            revoke_device: RevokeMobileDeviceUseCase::new(device_repo.clone()),
            list_devices: ListMobileDevicesUseCase::new(device_repo.clone()),
            rotate_password: RotateMobilePasswordUseCase::new(
                device_repo.clone(),
                password_hasher.clone(),
                credentials_minter,
            ),
            get_settings: GetMobileSyncSettingsUseCase::new(settings.clone(), endpoint_info),
            update_settings: UpdateMobileSyncSettingsUseCase::new(settings),
            list_lan_interfaces: ListLanInterfacesUseCase::new(lan_interface_probe),
            authenticate_basic: AuthenticateBasicAuthUseCase::new(device_repo, password_hasher),
            apply_incoming: ApplyIncomingMobileClipUseCase::new(
                apply_inbound,
                incoming_buffer,
                file_staging.clone(),
                clock,
            ),
            get_latest_doc: GetLatestMobileSyncDocUseCase::new(snapshot_port.clone()),
            get_file: GetMobileSyncFileUseCase::new(snapshot_port, file_staging),
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
    /// 不包含 `password_hash`(`username` 透传给 UI 作为辅助识别字段)。
    pub async fn list_devices(&self) -> Result<Vec<MobileDeviceSummary>, ListMobileDevicesError> {
        self.list_devices.execute().await
    }

    /// 给一台已登记设备换一份新密码。`input.password = None` 走 minter 自动
    /// 颁发;`Some(p)` 走自定义路径(校验长度)。返回值的 `password` 字段是
    /// **唯一一次**面向用户的明文回显, 之后只以 PHC 形式存在于服务端 sqlite。
    /// 旧密码立即失效,UI 必须提示用户同步更新 iPhone shortcut 里的 password
    /// 字段, 否则下次同步将收到 401。
    pub async fn rotate_password(
        &self,
        input: RotateMobilePasswordInput,
    ) -> Result<RotateMobilePasswordOutput, RotateMobilePasswordError> {
        self.rotate_password.execute(input).await
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

    // ─── SyncClipboard 协议 4 路由(P5a.6 真实接入) ─────────────────────

    /// `GET /SyncClipboard.json` 业务出口:通过 `LatestClipboardSnapshotPort`
    /// 取最新一条 paste-priority rep,翻成 SyncClipboard 协议元数据。
    pub async fn get_latest_sync_doc(
        &self,
    ) -> Result<SyncClipboardMeta, GetLatestMobileSyncDocError> {
        self.get_latest_doc.execute().await
    }

    /// `PUT /SyncClipboard.json` 业务出口:接收元数据(Text/Image/File 类型),
    /// 通过 `ApplyIncomingMobileClipUseCase` 喂给真实入站管线
    /// (capture → OS 写回 → 60s 写回环防御自动适用)。
    ///
    /// `source_device_id` 由 webserver 中间件注入的 [`AuthenticatedDevice`]
    /// 提供, 决定本次入站的伪 `DeviceId("mobile_sync:<id>")`。
    pub async fn put_sync_doc(
        &self,
        meta: SyncClipboardMeta,
        source_device_id: MobileDeviceId,
    ) -> Result<ApplyIncomingMobileClipOutcome, ApplyIncomingMobileClipError> {
        self.apply_incoming
            .execute(ApplyIncomingMobileClipInput {
                source_device_id,
                event: IncomingMobileClipEvent::SyncDoc {
                    item_type: meta.item_type,
                    text: meta.text,
                    data_name: meta.data_name,
                },
            })
            .await
    }

    /// `GET /file/{dataName}` 业务出口:按 dataName 命中最新 entry 的 paste
    /// rep,返回 `(mime, bytes)`。
    pub async fn get_clipboard_file(
        &self,
        data_name: &str,
    ) -> Result<GetMobileSyncFileOutput, GetMobileSyncFileError> {
        self.get_file.execute(data_name).await
    }

    /// `PUT /file/{dataName}` 业务出口:把 (mime, bytes) 暂存进
    /// `IncomingMobileBuffer`,等待 `PUT /SyncClipboard.json` 触发组装。
    /// 返回 `Buffered` outcome —— 路由层应回 HTTP 200。
    ///
    /// `source_device_id` 不被本步消费(BufferFile 阶段还没确定要应用),
    /// 但仍按 use case 契约一并传入,避免 PUT /SyncClipboard.json 时再
    /// 重新 lookup。
    pub async fn put_clipboard_file(
        &self,
        data_name: String,
        mime: String,
        bytes: Vec<u8>,
        source_device_id: MobileDeviceId,
    ) -> Result<ApplyIncomingMobileClipOutcome, ApplyIncomingMobileClipError> {
        self.apply_incoming
            .execute(ApplyIncomingMobileClipInput {
                source_device_id,
                event: IncomingMobileClipEvent::BufferFile {
                    data_name,
                    mime,
                    bytes,
                },
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    //! Facade 层集成测试 —— 用 in-memory port fakes 验证"deps → 各 use
    //! case → 对外方法"的接线没有错位。深层用例语义在各 use case 文件
    //! 已有覆盖, 这里只跑 happy path。
    //!
    //! P5a.6 增量:facade 多了 3 个 deps(`apply_inbound` / `incoming_buffer`
    //! / `snapshot_ports`),但本模块的 4 个端到端 happy-path 测试都不调
    //! SyncClipboard 协议方法,所以这 3 个新 deps 用最简 fake 装配 ——
    //! `apply_inbound` 用永远不会被调用的 dummy `ApplyInboundClipboardUseCase`,
    //! `snapshot_ports` 用 5 个 unimplemented stub。

    use super::*;

    use std::sync::Mutex;

    use anyhow::Result as AnyResult;
    use async_trait::async_trait;
    use base64::Engine;

    use uc_core::clipboard::{
        ClipboardEntry, ClipboardSelectionDecision, PayloadAvailability,
        PersistedClipboardRepresentation,
    };
    use uc_core::ids::{EntryId, EventId, RepresentationId};
    use uc_core::mobile_sync::{
        LanEndpointInfo, LanInterface, LanListenerStatus, MintedCredentials, MobileDevice,
        MobileDeviceError, MobileDeviceId,
    };
    use uc_core::ports::clipboard::{
        ClipboardEntryRepositoryPort, ClipboardPayloadResolverPort,
        ClipboardRepresentationRepositoryPort, ClipboardSelectionRepositoryPort,
        ProcessingUpdateOutcome, ResolvedClipboardPayload,
    };
    use uc_core::ports::{EndpointInfoError, LanInterfaceProbeError, PasswordHasherError};
    use uc_core::settings::model::Settings;
    use uc_core::BlobId;

    use crate::usecases::clipboard_sync::apply_inbound::{InboundCapture, InboundWrite};
    use uc_core::blob::ports::BlobReaderPort;
    use uc_core::SystemClipboardSnapshot;

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
        async fn update_password_hash(
            &self,
            id: &MobileDeviceId,
            new_hash: String,
        ) -> Result<bool, MobileDeviceError> {
            let mut devs = self.devices.lock().unwrap();
            match devs.iter_mut().find(|d| d.device_id == *id) {
                Some(d) => {
                    d.password_hash = new_hash;
                    Ok(true)
                }
                None => Ok(false),
            }
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
        async fn current_status(&self) -> Result<LanListenerStatus, EndpointInfoError> {
            Ok(LanListenerStatus::Listening(LanEndpointInfo {
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

    // ── 永远 unimplemented! 的 port stubs(本测试模块所有 happy-path 都
    // ── 不触发 SyncClipboard 4 路由,因此 capture/write/entry_repo/snapshot
    // ── 链路上的方法都不会被调用) ─────────────────────────────────────
    struct UnusedEntryRepo;
    #[async_trait]
    impl ClipboardEntryRepositoryPort for UnusedEntryRepo {
        async fn save_entry_and_selection(
            &self,
            _: &ClipboardEntry,
            _: &ClipboardSelectionDecision,
        ) -> AnyResult<()> {
            unimplemented!("not used by facade-level happy-path tests")
        }
        async fn get_entry(&self, _: &EntryId) -> AnyResult<Option<ClipboardEntry>> {
            unimplemented!()
        }
        async fn list_entries(&self, _: usize, _: usize) -> AnyResult<Vec<ClipboardEntry>> {
            unimplemented!()
        }
        async fn touch_entry(&self, _: &EntryId, _: i64) -> AnyResult<bool> {
            unimplemented!()
        }
        async fn delete_entry(&self, _: &EntryId) -> AnyResult<()> {
            unimplemented!()
        }
        async fn find_entry_id_by_snapshot_hash(&self, _: &str) -> AnyResult<Option<EntryId>> {
            unimplemented!()
        }
    }

    struct UnusedSelectionRepo;
    #[async_trait]
    impl ClipboardSelectionRepositoryPort for UnusedSelectionRepo {
        async fn get_selection(
            &self,
            _: &EntryId,
        ) -> AnyResult<Option<ClipboardSelectionDecision>> {
            unimplemented!()
        }
        async fn delete_selection(&self, _: &EntryId) -> AnyResult<()> {
            unimplemented!()
        }
    }

    struct UnusedRepRepo;
    #[async_trait]
    impl ClipboardRepresentationRepositoryPort for UnusedRepRepo {
        async fn get_representation(
            &self,
            _: &EventId,
            _: &RepresentationId,
        ) -> AnyResult<Option<PersistedClipboardRepresentation>> {
            unimplemented!()
        }
        async fn get_representation_by_id(
            &self,
            _: &RepresentationId,
        ) -> AnyResult<Option<PersistedClipboardRepresentation>> {
            unimplemented!()
        }
        async fn get_representation_by_blob_id(
            &self,
            _: &BlobId,
        ) -> AnyResult<Option<PersistedClipboardRepresentation>> {
            unimplemented!()
        }
        async fn update_blob_id(&self, _: &RepresentationId, _: &BlobId) -> AnyResult<()> {
            unimplemented!()
        }
        async fn update_blob_id_if_none(
            &self,
            _: &RepresentationId,
            _: &BlobId,
        ) -> AnyResult<bool> {
            unimplemented!()
        }
        async fn update_processing_result(
            &self,
            _: &RepresentationId,
            _: &[PayloadAvailability],
            _: Option<&BlobId>,
            _: PayloadAvailability,
            _: Option<&str>,
        ) -> AnyResult<ProcessingUpdateOutcome> {
            unimplemented!()
        }
    }

    struct UnusedResolver;
    #[async_trait]
    impl ClipboardPayloadResolverPort for UnusedResolver {
        async fn resolve(
            &self,
            _: &PersistedClipboardRepresentation,
        ) -> AnyResult<ResolvedClipboardPayload> {
            unimplemented!()
        }
    }

    struct UnusedBlobReader;
    #[async_trait]
    impl BlobReaderPort for UnusedBlobReader {
        async fn get(&self, _: &BlobId) -> AnyResult<Vec<u8>> {
            unimplemented!()
        }
    }

    struct UnusedCapture;
    #[async_trait]
    impl InboundCapture for UnusedCapture {
        async fn capture(
            &self,
            _: EntryId,
            _: SystemClipboardSnapshot,
        ) -> AnyResult<Option<EntryId>> {
            unimplemented!()
        }
    }

    struct UnusedWrite;
    #[async_trait]
    impl InboundWrite for UnusedWrite {
        async fn write(&self, _: SystemClipboardSnapshot) -> AnyResult<()> {
            unimplemented!()
        }
    }

    struct UnusedStaging;
    #[async_trait]
    impl uc_core::ports::MobileFileStagingPort for UnusedStaging {
        async fn stage_file(
            &self,
            _: &str,
            _: &str,
            _: &str,
            _: Vec<u8>,
        ) -> Result<uc_core::mobile_sync::StagedFile, uc_core::ports::MobileFileStagingError>
        {
            unimplemented!("facade smoke tests do not exercise File PUT path")
        }
        async fn read_by_uri(
            &self,
            _: &str,
        ) -> Result<Vec<u8>, uc_core::ports::MobileFileStagingError> {
            unimplemented!("facade smoke tests do not exercise File GET path")
        }
    }

    fn build_facade() -> MobileSyncFacade {
        let entry_repo: Arc<dyn ClipboardEntryRepositoryPort> = Arc::new(UnusedEntryRepo);
        let apply_inbound = Arc::new(ApplyInboundClipboardUseCase::new(
            entry_repo.clone(),
            Arc::new(UnusedCapture),
            Arc::new(UnusedWrite),
        ));
        MobileSyncFacade::new(MobileSyncFacadeDeps {
            clock: Arc::new(FixedClock(1_000)),
            credentials_minter: Arc::new(StaticMinter),
            password_hasher: Arc::new(FakeHasher),
            device_repo: Arc::new(InMemoryDeviceRepo::default()),
            endpoint_info: Arc::new(FixedEndpoint),
            lan_interface_probe: Arc::new(StubLanProbe),
            settings: Arc::new(InMemorySettings::default()),
            apply_inbound,
            incoming_buffer: Arc::new(IncomingMobileBuffer::new()),
            file_staging: Arc::new(UnusedStaging),
            snapshot_ports: MobileSyncSnapshotPorts {
                entry_repo,
                selection_repo: Arc::new(UnusedSelectionRepo),
                representation_repo: Arc::new(UnusedRepRepo),
                payload_resolver: Arc::new(UnusedResolver),
                blob_reader: Arc::new(UnusedBlobReader),
            },
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

        // 1. 登记。auto path: username/password 都走 minter。
        let out = facade
            .register_device(RegisterMobileShortcutDeviceInput {
                label: "我的 iPhone".into(),
                username: None,
                password: None,
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
        // current_lan_url 已从 view 移除 —— UI 自行从 lan_advertise_ip + lan_port 拼接。
        assert!(v0.lan_listener_error.is_none());

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
        // 与 build_facade() 不同 —— 此测试需要在 repo 里塞一台 FakeHasher 兼容
        // (PHC 形态 `phc:<password>`)的设备。直接用 FixedEndpoint + 共享 repo
        // 重新拼装一份 facade, 不走 register flow。
        let direct_device = MobileDevice {
            device_id: MobileDeviceId::new("did_auth"),
            label: "iPhone".into(),
            client_type: uc_core::mobile_sync::MobileClientType::IosShortcut,
            username: "mobile_alice".into(),
            password_hash: "phc:wonderland".into(),
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        };
        let repo = Arc::new(InMemoryDeviceRepo::default());
        repo.save(&direct_device).await.unwrap();

        let entry_repo: Arc<dyn ClipboardEntryRepositoryPort> = Arc::new(UnusedEntryRepo);
        let apply_inbound = Arc::new(ApplyInboundClipboardUseCase::new(
            entry_repo.clone(),
            Arc::new(UnusedCapture),
            Arc::new(UnusedWrite),
        ));

        let local = MobileSyncFacade::new(MobileSyncFacadeDeps {
            clock: Arc::new(FixedClock(1_000)),
            credentials_minter: Arc::new(StaticMinter),
            password_hasher: Arc::new(FakeHasher),
            device_repo: repo,
            endpoint_info: Arc::new(FixedEndpoint),
            lan_interface_probe: Arc::new(StubLanProbe),
            settings: Arc::new(InMemorySettings::default()),
            apply_inbound,
            incoming_buffer: Arc::new(IncomingMobileBuffer::new()),
            file_staging: Arc::new(UnusedStaging),
            snapshot_ports: MobileSyncSnapshotPorts {
                entry_repo,
                selection_repo: Arc::new(UnusedSelectionRepo),
                representation_repo: Arc::new(UnusedRepRepo),
                payload_resolver: Arc::new(UnusedResolver),
                blob_reader: Arc::new(UnusedBlobReader),
            },
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
    }

    #[tokio::test]
    async fn rotate_password_invalidates_old_password_and_keeps_username() {
        // 端到端验证:rotate 之后旧密码 401, 新密码 200, username/device_id
        // 不变。把整条 register → rotate → authenticate 链路串起来跑。
        let direct_device = MobileDevice {
            device_id: MobileDeviceId::new("did_rot"),
            label: "iPhone".into(),
            client_type: uc_core::mobile_sync::MobileClientType::IosShortcut,
            username: "mobile_rotme".into(),
            password_hash: "phc:original-pass".into(),
            created_at_ms: 1,
            last_seen_at_ms: None,
            last_seen_ip: None,
            reported_name: None,
            reported_os: None,
        };
        let repo = Arc::new(InMemoryDeviceRepo::default());
        repo.save(&direct_device).await.unwrap();

        let entry_repo: Arc<dyn ClipboardEntryRepositoryPort> = Arc::new(UnusedEntryRepo);
        let apply_inbound = Arc::new(ApplyInboundClipboardUseCase::new(
            entry_repo.clone(),
            Arc::new(UnusedCapture),
            Arc::new(UnusedWrite),
        ));

        let facade = MobileSyncFacade::new(MobileSyncFacadeDeps {
            clock: Arc::new(FixedClock(1_000)),
            credentials_minter: Arc::new(StaticMinter),
            password_hasher: Arc::new(FakeHasher),
            device_repo: repo.clone(),
            endpoint_info: Arc::new(FixedEndpoint),
            lan_interface_probe: Arc::new(StubLanProbe),
            settings: Arc::new(InMemorySettings::default()),
            apply_inbound,
            incoming_buffer: Arc::new(IncomingMobileBuffer::new()),
            file_staging: Arc::new(UnusedStaging),
            snapshot_ports: MobileSyncSnapshotPorts {
                entry_repo,
                selection_repo: Arc::new(UnusedSelectionRepo),
                representation_repo: Arc::new(UnusedRepRepo),
                payload_resolver: Arc::new(UnusedResolver),
                blob_reader: Arc::new(UnusedBlobReader),
            },
        });

        // 1. 旧密码可用
        let old_header = format!(
            "basic {}",
            base64::engine::general_purpose::STANDARD.encode("mobile_rotme:original-pass")
        );
        facade
            .authenticate_basic(AuthenticateBasicAuthInput {
                authorization_header: old_header.clone(),
            })
            .await
            .expect("old password ok before rotate");

        // 2. rotate 到自定义新密码
        let out = facade
            .rotate_password(RotateMobilePasswordInput {
                device_id: MobileDeviceId::new("did_rot"),
                password: Some("brand-new-pass".into()),
            })
            .await
            .expect("rotate ok");
        assert_eq!(out.username, "mobile_rotme");
        assert_eq!(out.password, "brand-new-pass");
        assert_eq!(out.device_id.as_str(), "did_rot");

        // 3. 旧密码立即失效
        let err = facade
            .authenticate_basic(AuthenticateBasicAuthInput {
                authorization_header: old_header,
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AuthenticateBasicAuthError::InvalidCredentials
        ));

        // 4. 新密码可用
        let new_header = format!(
            "basic {}",
            base64::engine::general_purpose::STANDARD.encode("mobile_rotme:brand-new-pass")
        );
        let auth = facade
            .authenticate_basic(AuthenticateBasicAuthInput {
                authorization_header: new_header,
            })
            .await
            .expect("new password ok after rotate");
        assert_eq!(auth.device.username, "mobile_rotme");

        // 5. rotate 不存在的 device → NotFound
        let err = facade
            .rotate_password(RotateMobilePasswordInput {
                device_id: MobileDeviceId::new("did_ghost"),
                password: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, RotateMobilePasswordError::NotFound(_)));
    }
}
