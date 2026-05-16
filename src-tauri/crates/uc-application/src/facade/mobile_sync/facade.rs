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
//! | [`MobileSyncFacade::update_settings`] | `UpdateMobileSyncSettingsUseCase` | 写 enabled / lan 字段, 装入 lifecycle 时 listener 即时生效 |
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

use uc_core::mobile_sync::LanListenerStatus;
use uc_core::mobile_sync::MobileDeviceId;
use uc_core::ports::mobile_sync::LatestClipboardSnapshotPort;
use uc_core::ports::{
    ClockPort, LanInterfaceProbePort, MobileCredentialsMinterPort, MobileDeviceRepositoryPort,
    MobileFileStagingPort, MobileLanLifecyclePort, MobileLanTarget, MobileSyncEndpointInfoPort,
    PasswordHasherPort, SettingsPort,
};
use uc_observability::analytics::AnalyticsPort;

use crate::facade::clipboard_outbound::ClipboardOutboundFacade;
use crate::facade::file_transfer::FileTransferFacade;
use crate::facade::mobile_sync::outbound_adapter::ClipboardOutboundFanOutAdapter;
use crate::usecases::clipboard_sync::apply_inbound::ApplyInboundClipboardUseCase;
use crate::usecases::mobile_sync::apply_incoming::MobileInboundFanOutPort;
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

// ─── Helpers ────────────────────────────────────────────────────────────

/// 默认 LAN 监听端口。与 daemon 装配期一次性读 settings 时的兜底完全一致,
/// 也与前端 `EnableMobileSyncDialog` 展示给用户的文案保持一致。
const DEFAULT_LAN_PORT: u16 = 42720;

/// 从写盘后的 settings 派生出 [`MobileLanTarget`]。
///
/// 单点真相:"两个开关都 on" → 起监听器,其它组合 → 不监听。port 取
/// 配置值或默认 [`DEFAULT_LAN_PORT`]。
///
/// 深度防御:`Some(0)` 在 use case 入口已被 `InvalidLanParameter` 拒绝,
/// 理论上跑到这里只剩 `None` 或合法值;但若磁盘 settings 文件被外部工具
/// 直接写入 `Some(0)`(绕过 use case 校验),也按 `None` fallback,避免把
/// 0 当 ephemeral 端口传给 adapter 导致"用户配置端口"与"实际监听端口"
/// 永久不一致。
fn lan_target_from_settings(out: &UpdateMobileSyncSettingsOutput) -> MobileLanTarget {
    if out.enabled && out.lan_listen_enabled {
        let port = out.lan_port.filter(|&p| p != 0).unwrap_or(DEFAULT_LAN_PORT);
        MobileLanTarget::Enabled { port }
    } else {
        MobileLanTarget::Disabled
    }
}

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
    /// 可选 file-transfer lifecycle facade。装配处提供时,SyncDoc apply
    /// 后自动 `link_transfer_to_entry` + `complete`,失败路径 `fail`,让
    /// mobile_lan 路径产生的 transfer 在 file_transfer 表里有完整 lifecycle;
    /// `None` 时静默降级(unit / 测试装配)。`PUT /file` handler 端
    /// (`webserver`)在收 body 期间自己调 `start` / `Progress` —— 本字段
    /// 仅用于 apply 阶段收尾。
    pub file_transfer: Option<Arc<FileTransferFacade>>,
    /// 可选剪贴板出站 facade。装配处提供时,移动端 `PUT /SyncClipboard.json`
    /// 成功落地本机后,本 facade 内部会异步把同一份 snapshot 走"本机捕获
    /// → 出站"完整管线 fan-out 给 Space 内其他已配对设备 ——
    ///
    /// - 文本 / 小图 inline 进 V3 envelope;
    /// - 大图自动剥成 iroh-blobs ref(避免撞 2 MiB wire 上限);
    /// - 文件用 `BlobTransferFacade::publish_blob_path` 流式发布到
    ///   iroh-blobs, 构造 free-file V3BlobRef,接收端拉回并改写 file-list
    ///   rep 成本机 URI ——
    ///   "手机文件 → 任一桌面 → 所有桌面"的真正传输靠这条路径成立。
    ///
    /// 同样受 `OutboundSyncPlanner` 控制 —— 用户在 settings 关了某个类型
    /// 的同步,mobile fan-out 与本机复制 fan-out 一同被 suppress, 没有
    /// "mobile 上传可以绕过同步开关"的旁路。
    ///
    /// `None` 时静默降级(facade 自测装配 / CLI fallback 等不接 P2P 出站
    /// 的场景):mobile 上传仅落地本机,不传播 —— 与本字段引入前的行为
    /// 完全一致, 不退化。
    pub clipboard_outbound: Option<Arc<ClipboardOutboundFacade>>,
    /// 可选 LAN 监听器生命周期 port,让 `update_settings` 在写盘后立即把
    /// listener 状态对齐到新设置(开/关/换端口),无需重启进程。
    ///
    /// 装配语义:
    /// - GUI daemon 模式(`uc-desktop`)装入 [`MobileLanLifecyclePort`] 的
    ///   in-process adapter, update_settings 即时生效;
    /// - CLI fallback / 单元测试装入 `None`, update_settings 仅写盘 ——
    ///   等下一次 daemon 进程启动时一次性读 settings 起 listener,
    ///   与本字段引入前的现有行为完全一致, 不退化。
    pub lan_lifecycle: Option<Arc<dyn MobileLanLifecyclePort>>,
    /// schema doc §7.6 / §12.2 P1：产品 analytics sink。流向
    /// `register_device` / `authenticate_basic` / `apply_incoming` 三个
    /// use case，分别 emit `mobile_device_registered` /
    /// `mobile_auth_failed` / `mobile_clipboard_synced`。
    ///
    /// 装配处直接复用 `AppDeps.analytics`（bootstrap 已包了一层
    /// `GatedAnalyticsSink`，运行时按用户 `usage_analytics_enabled` 切换
    /// noop / 真实 sink）。测试装配传 `NoopAnalyticsSink`。
    pub analytics: Arc<dyn AnalyticsPort>,
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
    /// `PUT /file` 流式上传入口持有的 staging port 引用。webserver 端
    /// `put_clipboard_file` handler 在收 body 期间通过 facade 的
    /// `begin_file_upload` / `append_file_chunk` / `finalize_file_upload` /
    /// `abort_file_upload` 转发到本 port,字节流不再绕道 use case 层的内存
    /// buffer。与 `apply_incoming` / `get_file` 共用同一份 Arc。
    file_staging: Arc<dyn MobileFileStagingPort>,
    /// 见 [`MobileSyncFacadeDeps::lan_lifecycle`]。`None` 表示当前装配不要求
    /// 即时生效(CLI fallback / 单测),`update_settings` 写盘后不调 apply。
    lan_lifecycle: Option<Arc<dyn MobileLanLifecyclePort>>,
    /// `update_settings` 在调完 `lifecycle.apply(target)` 后读这个 port 判断
    /// adapter 是否报了 `BindFailed`,据此把 reason 透传进 output 的
    /// `lan_listener_bind_error`。与 `get_settings` use case 共用同一份 Arc,
    /// 无额外资源开销。
    endpoint_info: Arc<dyn MobileSyncEndpointInfoPort>,
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
            file_transfer,
            clipboard_outbound,
            lan_lifecycle,
            analytics,
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
                analytics.clone(),
            ),
            revoke_device: RevokeMobileDeviceUseCase::new(device_repo.clone()),
            list_devices: ListMobileDevicesUseCase::new(device_repo.clone()),
            rotate_password: RotateMobilePasswordUseCase::new(
                device_repo.clone(),
                password_hasher.clone(),
                credentials_minter,
            ),
            get_settings: GetMobileSyncSettingsUseCase::new(
                settings.clone(),
                endpoint_info.clone(),
            ),
            update_settings: UpdateMobileSyncSettingsUseCase::new(settings),
            list_lan_interfaces: ListLanInterfacesUseCase::new(lan_interface_probe),
            authenticate_basic: AuthenticateBasicAuthUseCase::new(
                device_repo,
                password_hasher,
                analytics.clone(),
            ),
            // facade 装配处只能拿到 `ClipboardOutboundFacade`(由 daemon
            // runtime_assembly 装好),但 use case 依赖的是 use-case-local 的
            // `MobileInboundFanOutPort` trait。这里就是 facade 层的薄装配点:
            // 把 facade 包成 adapter, 让 use case 的依赖 surface 不必随出站
            // 管线演化而膨胀。详见 [`ClipboardOutboundFanOutAdapter`] 与
            // [`MobileInboundFanOutPort`] 的设计文档。
            apply_incoming: ApplyIncomingMobileClipUseCase::new(
                apply_inbound,
                incoming_buffer,
                file_staging.clone(),
                clock,
                file_transfer,
                clipboard_outbound.map(|outbound| {
                    Arc::new(ClipboardOutboundFanOutAdapter::new(outbound))
                        as Arc<dyn MobileInboundFanOutPort>
                }),
                analytics,
            ),
            get_latest_doc: GetLatestMobileSyncDocUseCase::new(snapshot_port.clone()),
            get_file: GetMobileSyncFileUseCase::new(snapshot_port, file_staging.clone()),
            file_staging,
            lan_lifecycle,
            endpoint_info,
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

    /// 更新移动端同步设置。
    ///
    /// 装入了 [`MobileLanLifecyclePort`] 的装配下(GUI daemon),写盘成功后立即
    /// 把 LAN listener 状态对齐到新设置 —— 用户不再需要重启进程才能让"开关
    /// 移动同步 / 改监听端口"生效。返回值的 `restart_required` 在这条路径下
    /// 永远是 `false`(字段保留,见
    /// [`UpdateMobileSyncSettingsOutput::restart_required`] 的字段 doc)。
    ///
    /// `apply` 完成后读一次 `MobileSyncEndpointInfoPort.current_status()`:
    /// 若 adapter 报 `BindFailed{reason}`(端口占用 / 权限 / IP 不可分配),
    /// 把 reason 透传进 output 的 `lan_listener_bind_error`,让调用方
    /// (典型:首次添加移动设备的 GUI 引导对话框)在导航到下一步前就能
    /// 看到 listener 没起来,而不是用户填完 label 才发现 iPhone 连不上。
    ///
    /// 没装入 lifecycle port 的装配(CLI fallback / 单测)只写盘不通知,
    /// `restart_required` 仍按"任一字段实际变化"返回,
    /// `lan_listener_bind_error` 保持 `None`(use case 默认值)。
    pub async fn update_settings(
        &self,
        input: UpdateMobileSyncSettingsInput,
    ) -> Result<UpdateMobileSyncSettingsOutput, UpdateMobileSyncSettingsError> {
        let mut out = self.update_settings.execute(input).await?;
        if let Some(lifecycle) = self.lan_lifecycle.as_ref() {
            let target = lan_target_from_settings(&out);
            lifecycle.apply(target).await;
            // 即时生效路径下"重启"语义不再适用,字段拍 false 让 UI 跳过
            // restart banner。详见 update_settings use case 文档。
            out.restart_required = false;
            // apply 后读 endpoint_info 一次。bind 失败时 adapter 已经把
            // reason 写进 BindFailed,这里把它透传到 output 字段,让前端
            // 在 happy-path 流程里就能感知到"设置落盘但 listener 没起来"。
            // endpoint_info 自身读失败(底层 storage 异常)按"无报错"处理 ——
            // 不让"探测错误"覆盖"实际是否成功",避免给前端虚假信号。
            if let Ok(LanListenerStatus::BindFailed { reason }) =
                self.endpoint_info.current_status().await
            {
                out.lan_listener_bind_error = Some(reason);
            }
        }
        Ok(out)
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

    /// `PUT /file/{dataName}` 全量字节出口(CLI debug / 测试用)。
    ///
    /// 生产路径(uc-webserver)走 [`Self::begin_file_upload`] →
    /// [`Self::append_file_chunk`] → [`Self::finalize_file_upload`] 三段式
    /// 流式上传,不进本入口。本入口内部仍走相同的 streaming staging API,
    /// 把整个 bytes 一次 append 进去, 再 finalize → 喂 BufferFile event,
    /// 这样应用层只剩"已 staged 文件"这一种路径(无两套并行)。
    ///
    /// 失败时调 [`Self::abort_file_upload`] 释放半写入的 staging 资源。
    pub async fn put_clipboard_file(
        &self,
        data_name: String,
        mime: String,
        bytes: Vec<u8>,
        source_device_id: MobileDeviceId,
        transfer_id: String,
    ) -> Result<ApplyIncomingMobileClipOutcome, ApplyIncomingMobileClipError> {
        let scope_id = streaming_scope_nonce();
        let handle = self
            .file_staging
            .begin_stage(&scope_id, &data_name, &mime)
            .await
            .map_err(|err| {
                ApplyIncomingMobileClipError::Internal(format!(
                    "put_clipboard_file: begin_stage failed: {err}"
                ))
            })?;
        if let Err(err) = self.file_staging.append_stage_chunk(&handle, &bytes).await {
            self.file_staging.abort_stage(handle).await;
            return Err(ApplyIncomingMobileClipError::Internal(format!(
                "put_clipboard_file: append_stage_chunk failed: {err}"
            )));
        }
        let staged = match self.file_staging.finalize_stage(handle).await {
            Ok(staged) => staged,
            Err(err) => {
                return Err(ApplyIncomingMobileClipError::Internal(format!(
                    "put_clipboard_file: finalize_stage failed: {err}"
                )));
            }
        };
        self.apply_incoming
            .execute(ApplyIncomingMobileClipInput {
                source_device_id,
                event: IncomingMobileClipEvent::BufferFile {
                    data_name,
                    mime,
                    staged,
                    transfer_id,
                },
            })
            .await
    }

    /// 开启一次 `PUT /file/{dataName}` 流式上传。返回的 [`StagingHandle`]
    /// 用于后续 chunk append / finalize / abort。`scope_id` 由调用方按
    /// "每次入站事件取一段独立 nonce"语义生成,典型用 [`streaming_scope_nonce`]。
    pub async fn begin_file_upload(
        &self,
        scope_id: &str,
        data_name: &str,
        mime: &str,
    ) -> Result<uc_core::mobile_sync::StagingHandle, uc_core::ports::MobileFileStagingError> {
        self.file_staging
            .begin_stage(scope_id, data_name, mime)
            .await
    }

    /// 把一个 body chunk 喂进流式 staging 会话。0 字节 chunk 合法且为 no-op。
    /// 单笔会话只允许**串行** append,并发同 handle 行为未定义。
    pub async fn append_file_chunk(
        &self,
        handle: &uc_core::mobile_sync::StagingHandle,
        chunk: &[u8],
    ) -> Result<(), uc_core::ports::MobileFileStagingError> {
        self.file_staging.append_stage_chunk(handle, chunk).await
    }

    /// 收齐字节后调用,消费 `handle` 完成 staging 并把"已 staged 文件"
    /// 挂进 IncomingMobileBuffer 等 SyncDoc 配对。返回 `Buffered` 或
    /// `DecodeFailed` 等 use case outcome,路由层据此映射 HTTP 响应。
    pub async fn finalize_file_upload(
        &self,
        handle: uc_core::mobile_sync::StagingHandle,
        data_name: String,
        mime: String,
        source_device_id: MobileDeviceId,
        transfer_id: String,
    ) -> Result<ApplyIncomingMobileClipOutcome, ApplyIncomingMobileClipError> {
        let staged = self
            .file_staging
            .finalize_stage(handle)
            .await
            .map_err(|err| {
                ApplyIncomingMobileClipError::Internal(format!(
                    "finalize_file_upload: staging finalize failed: {err}"
                ))
            })?;
        self.apply_incoming
            .execute(ApplyIncomingMobileClipInput {
                source_device_id,
                event: IncomingMobileClipEvent::BufferFile {
                    data_name,
                    mime,
                    staged,
                    transfer_id,
                },
            })
            .await
    }

    /// 放弃一次 `PUT /file/{dataName}` 流式上传 —— body 中断 / 客户端断流 /
    /// 任一 append 失败时调用,释放半写入的 staging 资源。fire-and-forget,
    /// 二次失败由 adapter 内部 log,不向上抛。
    pub async fn abort_file_upload(&self, handle: uc_core::mobile_sync::StagingHandle) {
        self.file_staging.abort_stage(handle).await;
    }
}

/// 生成 12 hex 字符的 staging scope nonce(uuid v4 simple 形态前 12 位)。
/// 调用方按"每次入站 PUT /file 取一段独立 nonce"语义使用,与 entry_id
/// 解耦(entry_id 在 ApplyInbound 内部生成,本阶段还不知道)。
pub fn streaming_scope_nonce() -> String {
    let id = uuid::Uuid::new_v4();
    let s = id.simple().to_string();
    s[..12].to_string()
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
        PayloadResolveError, ProcessingUpdateOutcome, ResolvedClipboardPayload,
    };
    use uc_core::ports::{EndpointInfoError, LanInterfaceProbeError, PasswordHasherError};
    use uc_core::settings::model::Settings;
    use uc_core::BlobId;
    use uc_core::DeviceId;

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
        ) -> Result<ResolvedClipboardPayload, PayloadResolveError> {
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
            _: DeviceId,
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

    // Facade smoke 测试不触达任何 staging 入口 —— 用 mockall strict mode 的
    // 未配置 panic 行为承担"防回归",任何方法被意外调到立刻可见。
    mockall::mock! {
        Staging {}
        #[async_trait]
        impl uc_core::ports::MobileFileStagingPort for Staging {
            async fn stage_file(
                &self,
                scope_id: &str,
                data_name: &str,
                mime: &str,
                bytes: Vec<u8>,
            ) -> Result<
                uc_core::mobile_sync::StagedFile,
                uc_core::ports::MobileFileStagingError,
            >;
            async fn read_by_uri(
                &self,
                uri: &str,
            ) -> Result<Vec<u8>, uc_core::ports::MobileFileStagingError>;
            async fn begin_stage(
                &self,
                scope_id: &str,
                data_name: &str,
                mime: &str,
            ) -> Result<
                uc_core::mobile_sync::StagingHandle,
                uc_core::ports::MobileFileStagingError,
            >;
            async fn append_stage_chunk(
                &self,
                handle: &uc_core::mobile_sync::StagingHandle,
                chunk: &[u8],
            ) -> Result<(), uc_core::ports::MobileFileStagingError>;
            async fn finalize_stage(
                &self,
                handle: uc_core::mobile_sync::StagingHandle,
            ) -> Result<
                uc_core::mobile_sync::StagedFile,
                uc_core::ports::MobileFileStagingError,
            >;
            async fn abort_stage(&self, handle: uc_core::mobile_sync::StagingHandle);
        }
    }

    fn staging_unused() -> Arc<MockStaging> {
        Arc::new(MockStaging::new())
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
            file_staging: staging_unused(),
            snapshot_ports: MobileSyncSnapshotPorts {
                entry_repo,
                selection_repo: Arc::new(UnusedSelectionRepo),
                representation_repo: Arc::new(UnusedRepRepo),
                payload_resolver: Arc::new(UnusedResolver),
                blob_reader: Arc::new(UnusedBlobReader),
            },
            file_transfer: None,
            clipboard_outbound: None,
            lan_lifecycle: None,
            analytics: Arc::new(uc_observability::analytics::NoopAnalyticsSink::default()),
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
        // build_facade() 不装 lan_lifecycle → 仍走 use case 旧语义,
        // 任一字段变化即 restart_required = true。
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
            file_staging: staging_unused(),
            snapshot_ports: MobileSyncSnapshotPorts {
                entry_repo,
                selection_repo: Arc::new(UnusedSelectionRepo),
                representation_repo: Arc::new(UnusedRepRepo),
                payload_resolver: Arc::new(UnusedResolver),
                blob_reader: Arc::new(UnusedBlobReader),
            },
            file_transfer: None,
            clipboard_outbound: None,
            lan_lifecycle: None,
            analytics: Arc::new(uc_observability::analytics::NoopAnalyticsSink::default()),
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
            file_staging: staging_unused(),
            snapshot_ports: MobileSyncSnapshotPorts {
                entry_repo,
                selection_repo: Arc::new(UnusedSelectionRepo),
                representation_repo: Arc::new(UnusedRepRepo),
                payload_resolver: Arc::new(UnusedResolver),
                blob_reader: Arc::new(UnusedBlobReader),
            },
            file_transfer: None,
            clipboard_outbound: None,
            lan_lifecycle: None,
            analytics: Arc::new(uc_observability::analytics::NoopAnalyticsSink::default()),
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

    // ── lan_lifecycle wire-up 测试 ─────────────────────────────────────────
    //
    // 验证装入了 [`MobileLanLifecyclePort`] 的装配下,update_settings 写盘后
    // 立刻调 apply(),且 restart_required 永远 false。

    /// 记录每次 apply 被调时的 target,供单测断言。
    #[derive(Default)]
    struct RecordingLanLifecycle {
        calls: Mutex<Vec<MobileLanTarget>>,
    }

    #[async_trait]
    impl MobileLanLifecyclePort for RecordingLanLifecycle {
        async fn apply(&self, target: MobileLanTarget) {
            self.calls.lock().unwrap().push(target);
        }
    }

    fn build_facade_with_lifecycle(lifecycle: Arc<RecordingLanLifecycle>) -> MobileSyncFacade {
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
            file_staging: staging_unused(),
            snapshot_ports: MobileSyncSnapshotPorts {
                entry_repo,
                selection_repo: Arc::new(UnusedSelectionRepo),
                representation_repo: Arc::new(UnusedRepRepo),
                payload_resolver: Arc::new(UnusedResolver),
                blob_reader: Arc::new(UnusedBlobReader),
            },
            file_transfer: None,
            clipboard_outbound: None,
            lan_lifecycle: Some(lifecycle),
            analytics: Arc::new(uc_observability::analytics::NoopAnalyticsSink::default()),
        })
    }

    #[tokio::test]
    async fn update_settings_with_lifecycle_applies_target_and_clears_restart_required() {
        let lifecycle = Arc::new(RecordingLanLifecycle::default());
        let facade = build_facade_with_lifecycle(lifecycle.clone());

        // 1. enable + lan_listen + 自定义 port → Enabled{port}
        let upd = facade
            .update_settings(UpdateMobileSyncSettingsInput {
                enabled: Some(true),
                lan_listen_enabled: Some(true),
                lan_advertise_ip: Some(Some("192.168.1.5".into())),
                lan_port: Some(Some(43210)),
            })
            .await
            .unwrap();
        assert!(upd.enabled);
        assert!(
            !upd.restart_required,
            "lifecycle 注入路径下 restart_required 永远 false"
        );

        // 2. lifecycle.apply 调一次,target = Enabled{43210}
        {
            let calls = lifecycle.calls.lock().unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0], MobileLanTarget::Enabled { port: 43210 });
        }

        // 3. 关 lan_listen → Disabled
        let upd2 = facade
            .update_settings(UpdateMobileSyncSettingsInput {
                lan_listen_enabled: Some(false),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(!upd2.restart_required);

        {
            let calls = lifecycle.calls.lock().unwrap();
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[1], MobileLanTarget::Disabled);
        }
    }

    #[tokio::test]
    async fn update_settings_with_lifecycle_defaults_port_to_42720_when_unset() {
        let lifecycle = Arc::new(RecordingLanLifecycle::default());
        let facade = build_facade_with_lifecycle(lifecycle.clone());

        // 只开两个开关, 不设 lan_port → adapter 应收到 Enabled{port: 42720}
        let _ = facade
            .update_settings(UpdateMobileSyncSettingsInput {
                enabled: Some(true),
                lan_listen_enabled: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();

        let calls = lifecycle.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], MobileLanTarget::Enabled { port: 42720 });
    }

    /// endpoint_info mock,可被测试替换为"apply 后我已经写了 BindFailed"。
    /// 用 Mutex<LanListenerStatus> 持当前状态;facade 调 current_status 直接读。
    struct BindFailureEndpoint {
        status: Mutex<LanListenerStatus>,
    }

    #[async_trait]
    impl MobileSyncEndpointInfoPort for BindFailureEndpoint {
        async fn current_status(&self) -> Result<LanListenerStatus, EndpointInfoError> {
            Ok(self.status.lock().unwrap().clone())
        }
    }

    /// 类似 RecordingLanLifecycle,但 apply 时把预设 BindFailed reason 写进
    /// endpoint_info,模拟生产 controller 在 bind 失败时的行为。
    struct FailingLanLifecycle {
        endpoint: Arc<BindFailureEndpoint>,
        reason: String,
    }

    #[async_trait]
    impl MobileLanLifecyclePort for FailingLanLifecycle {
        async fn apply(&self, target: MobileLanTarget) {
            // 只有 Enabled 时模拟 bind 失败;Disabled 写 Stopped。
            let next = match target {
                MobileLanTarget::Enabled { .. } => LanListenerStatus::BindFailed {
                    reason: self.reason.clone(),
                },
                MobileLanTarget::Disabled => LanListenerStatus::Stopped,
            };
            *self.endpoint.status.lock().unwrap() = next;
        }
    }

    #[tokio::test]
    async fn update_settings_with_lifecycle_propagates_bind_failure_to_output() {
        // 构造一份 facade,endpoint_info 与 lifecycle 共享同一份"模拟 daemon
        // 状态"的 Arc<BindFailureEndpoint>:lifecycle.apply 写 BindFailed,
        // facade 紧接着读出来填 lan_listener_bind_error。
        let endpoint = Arc::new(BindFailureEndpoint {
            status: Mutex::new(LanListenerStatus::Stopped),
        });
        let lifecycle = Arc::new(FailingLanLifecycle {
            endpoint: endpoint.clone(),
            reason: "Address already in use (os error 48)".into(),
        });

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
            device_repo: Arc::new(InMemoryDeviceRepo::default()),
            // 关键:endpoint_info 装的是 BindFailureEndpoint, lifecycle 也持
            // 同一份 Arc, apply 写完 facade 立刻能读到。
            endpoint_info: endpoint.clone(),
            lan_interface_probe: Arc::new(StubLanProbe),
            settings: Arc::new(InMemorySettings::default()),
            apply_inbound,
            incoming_buffer: Arc::new(IncomingMobileBuffer::new()),
            file_staging: staging_unused(),
            snapshot_ports: MobileSyncSnapshotPorts {
                entry_repo,
                selection_repo: Arc::new(UnusedSelectionRepo),
                representation_repo: Arc::new(UnusedRepRepo),
                payload_resolver: Arc::new(UnusedResolver),
                blob_reader: Arc::new(UnusedBlobReader),
            },
            file_transfer: None,
            clipboard_outbound: None,
            lan_lifecycle: Some(lifecycle),
            analytics: Arc::new(uc_observability::analytics::NoopAnalyticsSink::default()),
        });

        // enable 两开关 → lifecycle.apply(Enabled) → endpoint = BindFailed
        let out = facade
            .update_settings(UpdateMobileSyncSettingsInput {
                enabled: Some(true),
                lan_listen_enabled: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(
            out.lan_listener_bind_error.as_deref(),
            Some("Address already in use (os error 48)"),
            "facade 必须把 endpoint_info 的 BindFailed reason 透传到 output"
        );
        // restart_required 仍然在 lifecycle 路径下被拍 false ——
        // 不让前端在 bind 失败时还弹 restart banner(那是误导)。
        assert!(!out.restart_required);

        // 关掉 lan_listen → Disabled → endpoint = Stopped → 字段回 None
        let out2 = facade
            .update_settings(UpdateMobileSyncSettingsInput {
                lan_listen_enabled: Some(false),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(
            out2.lan_listener_bind_error.is_none(),
            "Disabled 目标下 endpoint=Stopped, bind_error 必须清零"
        );
    }

    #[tokio::test]
    async fn update_settings_with_lifecycle_zero_port_falls_back_to_default() {
        // 深度防御:若 settings 文件被外部写入 lan_port=Some(0)(绕过 use case
        // 校验),facade 把它当 None 处理, target 走默认 42720。
        let lifecycle = Arc::new(RecordingLanLifecycle::default());
        let facade = build_facade_with_lifecycle(lifecycle.clone());

        // 注意 lan_port=Some(Some(0)) 会被 use case 拒绝, 这里通过
        // lan_target_from_settings 单元函数直接验证 fallback 即可。
        // (use case 边界已被 update_settings.rs::rejects_zero_port 钉死)
        let synthetic_out = UpdateMobileSyncSettingsOutput {
            enabled: true,
            lan_listen_enabled: true,
            lan_advertise_ip: None,
            lan_port: Some(0),
            restart_required: false,
            lan_listener_bind_error: None,
        };
        let target = lan_target_from_settings(&synthetic_out);
        assert_eq!(
            target,
            MobileLanTarget::Enabled { port: 42720 },
            "Some(0) 必须 fallback 到默认端口,不能透传给 adapter"
        );

        // 静默 unused —— facade 在本测试不被实际调用, 只是为了证明 helper
        // 是 facade 模块内可见且可单元测试的。
        let _ = facade;
    }

    #[tokio::test]
    async fn update_settings_without_lifecycle_preserves_legacy_restart_required() {
        // 没装 lifecycle port → 走 build_facade() 路径, 不调 apply,
        // restart_required 保持 use case 原始判定(改了字段 → true)。
        // 与 settings_round_trip_through_facade 互补:那个测试覆盖
        // 同值不写盘 → restart_required = false 的现有行为,本测试
        // 钉死"实际变化 → 仍为 true"的现有行为不退化。
        let facade = build_facade();
        let upd = facade
            .update_settings(UpdateMobileSyncSettingsInput {
                enabled: Some(true),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(upd.enabled);
        assert!(
            upd.restart_required,
            "no-lifecycle 装配下保留旧的 restart_required 语义"
        );
    }
}
