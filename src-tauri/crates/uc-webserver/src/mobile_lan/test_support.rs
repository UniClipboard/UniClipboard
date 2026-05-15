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

use anyhow::{anyhow, Result as AnyResult};
use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as BASE64_STD;
use base64::Engine;

use uc_application::facade::{
    IncomingMobileBuffer, MobileSyncFacade, MobileSyncFacadeDeps, MobileSyncSnapshotPorts,
};
use uc_application::{
    ApplyInboundClipboardUseCase, InboundCapture as ApplyInboundCapture,
    InboundWrite as ApplyInboundWrite,
};
use uc_core::blob::ports::BlobReaderPort;
use uc_core::clipboard::{
    ClipboardEntry, ClipboardSelectionDecision, PayloadAvailability,
    PersistedClipboardRepresentation,
};
use uc_core::ids::{EntryId, EventId, RepresentationId};
use uc_core::mobile_sync::{
    LanInterface, LanListenerStatus, MintedCredentials, MobileClientType, MobileDevice,
    MobileDeviceError, MobileDeviceId,
};
use uc_core::ports::clipboard::{
    ClipboardEntryRepositoryPort, ClipboardPayloadResolverPort,
    ClipboardRepresentationRepositoryPort, ClipboardSelectionRepositoryPort, PayloadResolveError,
    ProcessingUpdateOutcome, ResolvedClipboardPayload,
};
use uc_core::ports::{
    ClockPort, EndpointInfoError, LanInterfaceProbeError, LanInterfaceProbePort,
    MobileCredentialsMinterPort, MobileDeviceRepositoryPort, MobileSyncEndpointInfoPort,
    PasswordHasherError, PasswordHasherPort, SettingsPort,
};
use uc_core::settings::model::Settings;
use uc_core::{BlobId, SystemClipboardSnapshot};

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
            Ok(LanListenerStatus::Stopped)
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

    // P5a.6:facade 多了 3 个 deps —— `apply_inbound` / `incoming_buffer`
    // / `snapshot_ports`。webserver 的路由测试只跑 401 / 404 / wire DTO
    // 校验,从不需要"真捕获 + 真 OS 写"或"真读最近一条 entry",因此这里
    // 用 NoOp 实现塞过编译。GET 路径下 NoOp entry repo 永远返回空列表,
    // routes.rs 测试断言 404 即建立在这条事实上。
    let entry_repo: Arc<dyn ClipboardEntryRepositoryPort> = Arc::new(NoopEntryRepo);
    let apply_inbound = Arc::new(ApplyInboundClipboardUseCase::new(
        entry_repo.clone(),
        Arc::new(NoopInboundCapture),
        Arc::new(NoopInboundWrite),
    ));

    Arc::new(MobileSyncFacade::new(MobileSyncFacadeDeps {
        clock: Arc::new(FixedClock),
        credentials_minter: Arc::new(StaticMinter),
        password_hasher: Arc::new(FakeHasher),
        device_repo: repo,
        endpoint_info: Arc::new(FixedEndpoint),
        lan_interface_probe: Arc::new(StubLanProbe),
        settings: Arc::new(InMemorySettings::default()),
        apply_inbound,
        incoming_buffer: Arc::new(IncomingMobileBuffer::new()),
        file_staging: NoopFileStaging::new(),
        snapshot_ports: MobileSyncSnapshotPorts {
            entry_repo,
            selection_repo: Arc::new(NoopSelectionRepo),
            representation_repo: Arc::new(NoopRepRepo),
            payload_resolver: Arc::new(NoopResolver),
            blob_reader: Arc::new(NoopBlobReader),
        },
        file_transfer: None,
        clipboard_outbound: None,
        lan_lifecycle: None,
        // schema doc §7.6 / §12.2 P1：测试装配走 noop sink，不污染 capture。
        analytics: Arc::new(uc_observability::analytics::NoopAnalyticsSink::default()),
    }))
}

// ── P5a.6 NoOp adapters(本模块测试装配 facade 用) ──────────────────────

struct NoopEntryRepo;
#[async_trait]
impl ClipboardEntryRepositoryPort for NoopEntryRepo {
    async fn save_entry_and_selection(
        &self,
        _: &ClipboardEntry,
        _: &ClipboardSelectionDecision,
    ) -> AnyResult<()> {
        Err(anyhow!("noop"))
    }
    async fn get_entry(&self, _: &EntryId) -> AnyResult<Option<ClipboardEntry>> {
        Ok(None)
    }
    async fn list_entries(&self, _: usize, _: usize) -> AnyResult<Vec<ClipboardEntry>> {
        Ok(vec![])
    }
    async fn touch_entry(&self, _: &EntryId, _: i64) -> AnyResult<bool> {
        Ok(false)
    }
    async fn delete_entry(&self, _: &EntryId) -> AnyResult<()> {
        Ok(())
    }
    async fn find_entry_id_by_snapshot_hash(&self, _: &str) -> AnyResult<Option<EntryId>> {
        Ok(None)
    }
}

struct NoopSelectionRepo;
#[async_trait]
impl ClipboardSelectionRepositoryPort for NoopSelectionRepo {
    async fn get_selection(&self, _: &EntryId) -> AnyResult<Option<ClipboardSelectionDecision>> {
        Ok(None)
    }
    async fn delete_selection(&self, _: &EntryId) -> AnyResult<()> {
        Ok(())
    }
}

struct NoopRepRepo;
#[async_trait]
impl ClipboardRepresentationRepositoryPort for NoopRepRepo {
    async fn get_representation(
        &self,
        _: &EventId,
        _: &RepresentationId,
    ) -> AnyResult<Option<PersistedClipboardRepresentation>> {
        Ok(None)
    }
    async fn get_representation_by_id(
        &self,
        _: &RepresentationId,
    ) -> AnyResult<Option<PersistedClipboardRepresentation>> {
        Ok(None)
    }
    async fn get_representation_by_blob_id(
        &self,
        _: &BlobId,
    ) -> AnyResult<Option<PersistedClipboardRepresentation>> {
        Ok(None)
    }
    async fn update_blob_id(&self, _: &RepresentationId, _: &BlobId) -> AnyResult<()> {
        Ok(())
    }
    async fn update_blob_id_if_none(&self, _: &RepresentationId, _: &BlobId) -> AnyResult<bool> {
        Ok(false)
    }
    async fn update_processing_result(
        &self,
        _: &RepresentationId,
        _: &[PayloadAvailability],
        _: Option<&BlobId>,
        _: PayloadAvailability,
        _: Option<&str>,
    ) -> AnyResult<ProcessingUpdateOutcome> {
        Ok(ProcessingUpdateOutcome::NotFound)
    }
}

struct NoopResolver;
#[async_trait]
impl ClipboardPayloadResolverPort for NoopResolver {
    async fn resolve(
        &self,
        rep: &PersistedClipboardRepresentation,
    ) -> Result<ResolvedClipboardPayload, PayloadResolveError> {
        Err(PayloadResolveError::Integrity {
            rep_id: rep.id.clone(),
            reason: "noop".to_string(),
        })
    }
}

struct NoopBlobReader;
#[async_trait]
impl BlobReaderPort for NoopBlobReader {
    async fn get(&self, _: &BlobId) -> AnyResult<Vec<u8>> {
        Err(anyhow!("noop"))
    }
}

struct NoopInboundCapture;
#[async_trait]
impl ApplyInboundCapture for NoopInboundCapture {
    async fn capture(&self, _: EntryId, _: SystemClipboardSnapshot) -> AnyResult<Option<EntryId>> {
        Err(anyhow!(
            "test_support: NoOp InboundCapture should not be reached"
        ))
    }
}

struct NoopInboundWrite;
#[async_trait]
impl ApplyInboundWrite for NoopInboundWrite {
    async fn write(&self, _: SystemClipboardSnapshot) -> AnyResult<()> {
        Err(anyhow!(
            "test_support: NoOp InboundWrite should not be reached"
        ))
    }
}

// 内存版 NoOp staging,让 webserver 路由测试能跑通 PUT /file 的流式
// staging 流程而不真正落盘。`stage_file` / `read_by_uri` 仍返 Io 错(路由
// 测试不该走那两条),streaming 接口在内存里维护 sessions 表,finalize 时
// 返回固定形态的 fake `file:///` URI(URI 字符串本身不会被路由测试解引用,
// 后续 SyncDoc 路径才会消费它,但 webserver 单测只 PUT /file 不 PUT JSON)。
struct NoopFileStaging {
    sessions: tokio::sync::Mutex<std::collections::HashSet<uuid::Uuid>>,
}

impl NoopFileStaging {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            sessions: tokio::sync::Mutex::new(std::collections::HashSet::new()),
        })
    }
}

#[async_trait]
impl uc_core::ports::MobileFileStagingPort for NoopFileStaging {
    async fn stage_file(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: Vec<u8>,
    ) -> Result<uc_core::mobile_sync::StagedFile, uc_core::ports::MobileFileStagingError> {
        Err(uc_core::ports::MobileFileStagingError::Io(
            "test_support: NoOp file staging stage_file should not be reached".into(),
        ))
    }
    async fn read_by_uri(
        &self,
        _: &str,
    ) -> Result<Vec<u8>, uc_core::ports::MobileFileStagingError> {
        Err(uc_core::ports::MobileFileStagingError::Io(
            "test_support: NoOp file staging read_by_uri should not be reached".into(),
        ))
    }
    async fn begin_stage(
        &self,
        _scope_id: &str,
        _data_name: &str,
        _mime: &str,
    ) -> Result<uc_core::mobile_sync::StagingHandle, uc_core::ports::MobileFileStagingError> {
        let handle = uc_core::mobile_sync::StagingHandle::new();
        self.sessions.lock().await.insert(handle.token());
        Ok(handle)
    }
    async fn append_stage_chunk(
        &self,
        handle: &uc_core::mobile_sync::StagingHandle,
        _chunk: &[u8],
    ) -> Result<(), uc_core::ports::MobileFileStagingError> {
        if !self.sessions.lock().await.contains(&handle.token()) {
            return Err(uc_core::ports::MobileFileStagingError::Io(format!(
                "test_support NoopFileStaging: unknown handle {}",
                handle.token()
            )));
        }
        Ok(())
    }
    async fn finalize_stage(
        &self,
        handle: uc_core::mobile_sync::StagingHandle,
    ) -> Result<uc_core::mobile_sync::StagedFile, uc_core::ports::MobileFileStagingError> {
        let token = handle.token();
        if !self.sessions.lock().await.remove(&token) {
            return Err(uc_core::ports::MobileFileStagingError::Io(format!(
                "test_support NoopFileStaging: unknown handle {token} in finalize"
            )));
        }
        Ok(uc_core::mobile_sync::StagedFile {
            uri: uc_core::mobile_sync::StagedFileUri::new(format!(
                "file:///tmp/uc-webserver-test/{token}"
            )),
            sanitized_name: "test-staged".into(),
        })
    }
    async fn abort_stage(&self, handle: uc_core::mobile_sync::StagingHandle) {
        self.sessions.lock().await.remove(&handle.token());
    }
}

/// 拼一份 `Authorization: basic <base64(user:pass)>` header 值。
///
/// scheme 用 SyncClipboard 客户端实际下发的小写形式,验证 RFC 不区分
/// 大小写解析的行为(本模块两个测试模块共用)。
pub(crate) fn auth_header(username: &str, password: &str) -> String {
    let payload = BASE64_STD.encode(format!("{username}:{password}"));
    format!("basic {payload}")
}
