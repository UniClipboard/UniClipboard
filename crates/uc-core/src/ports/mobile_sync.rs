//! 移动端同步所需的端口抽象(v3 SyncClipboard 兼容版)。
//!
//! 这些 trait 仅描述"应用层在颁发凭据 / 持久化设备 / 探测当前 LAN 端点 /
//! 验证密码时需要外部具备的能力",不涉及任何具体技术实现(OS RNG、SQLite、
//! 网卡探测、Argon2 等)。具体实现由 `uc-infra` / `uc-platform` /
//! `uc-application` 中的 adapter 承担。
//!
//! 设计参考 `.context/mobile-sync/SPEC.md` §14 / §15(v3 权威章节)。

use async_trait::async_trait;
use thiserror::Error;

use crate::mobile_sync::{
    LanEndpointInfo, LanInterface, LanListenerStatus, LatestPasteRepresentation, MintedCredentials,
    MobileDevice, MobileDeviceError, MobileDeviceId, StagedFile, StagingHandle,
};

// ─── credentials minter ──────────────────────────────────────────────────

/// 颁发 mobile 设备的 Basic Auth 凭据 + 稳定 device id。
///
/// 同步而非异步:底层只是 `OsRng + Argon2 + base64` 的纯计算,没必要扛上
/// `async` 的成本。
///
/// 把 username / password / password_hash / device_id 合并为同一个 minter 是
/// 有意为之 —— 四者都是"登记一台 mobile 设备时颁发的不可猜凭据",单一
/// 职责且来自同一熵源更易推理。
pub trait MobileCredentialsMinterPort: Send + Sync {
    /// 生成一对全新的凭据。
    ///
    /// 实现必须保证:
    /// 1. `username` 在所有已登记设备中唯一(典型实现:`mobile_<8hex>`)
    /// 2. `password` 是用户一次性可见的明文(典型:base64-url-safe 16 字节
    ///    OsRng,约 22 字符)
    /// 3. `password_hash` 是 `password` 的 Argon2id PHC 字符串
    /// 4. `device_id` 形如 `did_<32hex>`,与 `username` / `password` 相互独立
    ///    (不共享熵)
    fn mint_credentials(&self) -> MintedCredentials;
}

// ─── password hasher ─────────────────────────────────────────────────────

/// 密码哈希与验证能力。
///
/// 业务上只关心"这个明文密码能不能验证通过这个 hash",**不**关心具体算法。
/// adapter 内部固定用 Argon2id(uc-infra::mobile_sync::password_hasher),
/// 但 trait 不暴露算法名 —— 未来切换 algo 不需要改 use case。
///
/// `verify` 必须用 constant-time 比较(adapter 自己用 `subtle` 或 PHC 库内置
/// 实现,不让 use case 关心这个细节)。
#[async_trait]
pub trait PasswordHasherPort: Send + Sync {
    /// 把明文密码哈希成 PHC 字符串(`$argon2id$v=19$m=...,t=...,p=...$<salt>$<hash>`)。
    async fn hash(&self, password: &str) -> Result<String, PasswordHasherError>;

    /// 校验明文密码与已知的 PHC 字符串是否匹配。
    ///
    /// 返回 `Ok(true)` / `Ok(false)`;`Err` 仅在 PHC 字符串本身格式损坏 / 算法
    /// 库异常时返回。
    async fn verify(&self, password: &str, phc: &str) -> Result<bool, PasswordHasherError>;
}

#[derive(Debug, Error)]
pub enum PasswordHasherError {
    /// PHC 字符串格式不合法 / 解析失败。adapter 必须在写入 db 前自检,但
    /// 读出的 row 可能因升级 / 损坏而非法,这条让 use case 据此把记录视为
    /// "需要重新登记"。
    #[error("invalid phc string: {0}")]
    InvalidPhc(String),

    /// 哈希 / 校验调用本身失败(库内部错误 / 内存不足等)。
    #[error("password hasher internal failure: {0}")]
    Internal(String),
}

// ─── device store (inner aggregate) ──────────────────────────────────────

/// Inner aggregate persistence surface for registered mobile devices
/// (username-indexed).
///
/// This is the low-level store (ports.md §5.1/§12): adapters implement it once
/// and the narrow device intent ports below delegate to it. Application-layer
/// consumers depend on the narrow ports, never on this aggregate.
#[async_trait]
pub trait MobileDeviceStore: Send + Sync {
    /// 持久化一台新设备。重复 device_id / username 应返回对应的领域错误。
    async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError>;

    /// 鉴权热路径:根据 username 定位设备。
    async fn find_by_username(
        &self,
        username: &str,
    ) -> Result<Option<MobileDevice>, MobileDeviceError>;

    /// 列表 / 撤销 UI 用:按 device id 精确查询。
    async fn find_by_device_id(
        &self,
        device_id: &MobileDeviceId,
    ) -> Result<Option<MobileDevice>, MobileDeviceError>;

    /// 列出全部设备 —— v1 不分页,预期数量很小(个位数)。
    async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError>;

    /// 删除一条记录。返回 `true` 表示真实删掉了一行;`false` 表示原本就
    /// 不存在(撤销操作幂等)。
    async fn delete(&self, device_id: &MobileDeviceId) -> Result<bool, MobileDeviceError>;

    /// Replace the editable fields of one mobile device in a single write.
    ///
    /// Used by the device-management flow: label edits may keep credentials as-is,
    /// while username/password edits replace the persisted credential fields
    /// atomically. Implementations must preserve rows not matching `device_id`,
    /// return `Ok(false)` when the device is missing, and return
    /// `UsernameCollision` when `updated.username` is already held by another
    /// device.
    async fn update_mobile_device(&self, updated: &MobileDevice)
        -> Result<bool, MobileDeviceError>;
}

// ─── device repository intent ports ──────────────────────────────────────
//
// Narrow, single-responsibility views over the registered-device store. Each
// consumer depends only on the slice it actually uses; the concrete adapter
// implements every one of them (see ports.md §8.3). `MobileDeviceStore`
// above remains the inner aggregate store.

/// Locate a registered device by its username.
#[async_trait]
pub trait FindMobileDeviceByUsernamePort: Send + Sync {
    /// Return the device whose username matches exactly, or `None` if there is
    /// no such device.
    async fn find_by_username(
        &self,
        username: &str,
    ) -> Result<Option<MobileDevice>, MobileDeviceError>;
}

/// Locate a registered device by its stable device id.
#[async_trait]
pub trait FindMobileDeviceByIdPort: Send + Sync {
    /// Return the device with this id, or `None` if there is no such device.
    async fn find_by_device_id(
        &self,
        device_id: &MobileDeviceId,
    ) -> Result<Option<MobileDevice>, MobileDeviceError>;
}

/// Enumerate every registered device.
#[async_trait]
pub trait ListMobileDevicesPort: Send + Sync {
    /// Return all registered devices. The result is unordered and unpaged; the
    /// expected population is small.
    async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError>;
}

/// Persist a newly registered device.
#[async_trait]
pub trait SaveMobileDevicePort: Send + Sync {
    /// Persist a brand-new device. Returns `AlreadyExists` when the device id is
    /// already taken and `UsernameCollision` when the username is already held
    /// by another device.
    async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError>;
}

/// Remove a registered device.
#[async_trait]
pub trait DeleteMobileDevicePort: Send + Sync {
    /// Delete the device with this id. Returns `true` when a row was removed,
    /// `false` when no such device existed (the operation is idempotent).
    async fn delete(&self, device_id: &MobileDeviceId) -> Result<bool, MobileDeviceError>;
}

/// Replace the editable fields of an existing device.
#[async_trait]
pub trait UpdateMobileDevicePort: Send + Sync {
    /// Replace the editable fields of one device in a single write.
    ///
    /// Implementations must preserve rows not matching `updated.device_id`,
    /// return `Ok(false)` when the device is missing, and return
    /// `UsernameCollision` when `updated.username` is already held by another
    /// device.
    async fn update_mobile_device(&self, updated: &MobileDevice)
        -> Result<bool, MobileDeviceError>;
}

// ─── endpoint info ───────────────────────────────────────────────────────

/// 探测 daemon 当前对外暴露的 LAN 端点 / 启动状态。
///
/// 抽象出来是因为 daemon 启停 / 配置变更后端点会动;登记设备的 use case
/// 需要拿到"现在能用"的 URL,而不是配置里写的目标 URL。
///
/// `current_status` 是真相方法,返回三态 `LanListenerStatus`:
/// `Stopped` / `Listening{url}` / `BindFailed{reason}`。
///
/// `current_lan_endpoint` 是历史兼容入口,语义等价于"`Listening` 时返回
/// `Some(endpoint)`,其余返回 `None`",有 default 实现转发到 `current_status`。
/// 新接入点应直接使用 `current_status`,以便区分"没开启"和"开了但 bind
/// 失败"两种语义。
#[async_trait]
pub trait MobileSyncEndpointInfoPort: Send + Sync {
    async fn current_status(&self) -> Result<LanListenerStatus, EndpointInfoError>;

    async fn current_lan_endpoint(&self) -> Result<Option<LanEndpointInfo>, EndpointInfoError> {
        Ok(self.current_status().await?.endpoint().cloned())
    }
}

#[derive(Debug, Error)]
pub enum EndpointInfoError {
    #[error("endpoint info storage failure: {0}")]
    Storage(String),
}

// ─── lan interface probe ────────────────────────────────────────────────

/// 枚举本机当前的 LAN 网卡 IPv4 地址。
///
/// 用于"添加 iPhone"流程:UI 让用户从可用 IP 中挑一个,daemon 据此拼出
/// 二维码里的 LAN URL。返回的列表是"adapter 看到的全部 IPv4 接口"——是否
/// 排除 loopback / link-local / VPN-overlay / CGNAT 等由 application 层 use
/// case 按当前产品策略过滤,便于以后随设置(如
/// `NetworkSettings.allow_overlay_network_addrs`)调整而无需改 adapter。
///
/// 同步而非异步:实现里就是一次 syscall,没必要扛 async 成本。但保留
/// `async fn` 是因为某些平台需要起 tokio 任务读 sysctl —— 让 trait 形状
/// 适应所有合法实现。
#[async_trait]
pub trait LanInterfaceProbePort: Send + Sync {
    async fn list_interfaces(&self) -> Result<Vec<LanInterface>, LanInterfaceProbeError>;
}

#[derive(Debug, Error)]
pub enum LanInterfaceProbeError {
    /// 探测失败 —— OS 调用错误、权限不足等。adapter 层负责把底层错误的
    /// 文本带上来给排障。
    #[error("lan interface probe failed: {0}")]
    Probe(String),
}

// ─── latest paste representation ────────────────────────────────────────

/// 读取最近一条 clipboard entry 的 paste-priority representation,字节材
/// 化好后交给调用方。
///
/// mobile sync 出站(Mac → iPhone)的两条 HTTP 路由(`GET /SyncClipboard.json`
/// 与 `GET /file/{dataName}`)都靠这个 port 拿数据 —— 路由层和 facade 都不
/// 直接接触 `clipboard_entry` / `clipboard_event` / `clipboard_representation`
/// 表,只看一份"已选 + 已材化"的视图。
///
/// **方案 X**(P5a Decisions):本 port 永远返回**最新一条**记录,**不区分**
/// 来源(本地复制 / mobile sync 入站 / P2P 入站)。dedup 由 `ApplyInbound` 的
/// `content_hash` 在入站时已经处理;mobile sync 这条出站路径无需在 query
/// 阶段再做"过滤掉自己来源避免回环"的小聪明。
#[async_trait]
pub trait LatestClipboardSnapshotPort: Send + Sync {
    /// 拿到当前剪贴板 paste-priority rep。无任何 entry 时返回 `Ok(None)`,
    /// use case 据此翻成 `NotFound` → 路由 404。
    async fn latest_paste_representation(
        &self,
    ) -> Result<Option<LatestPasteRepresentation>, LatestClipboardSnapshotError>;

    /// 拿到当前 entry 中**plain-text 偏好**的 representation。
    ///
    /// 选择规则:在该 entry 的 selection 涉及的全部 rep(primary +
    /// secondary)中,优先返回 mime `text/plain` 或 format_id `text` 的 rep;
    /// 若都没有,fallback 到 paste-priority rep,与
    /// [`Self::latest_paste_representation`] 行为一致。
    ///
    /// 领域定位:与 [`Self::latest_paste_representation`] 的差异在于优先级
    /// 不再是"系统默认粘贴目标",而是"纯文本字节"。当 paste-priority rep
    /// 是富文本(`text/rtf` / `text/html`)而 entry 同时承载了 `text/plain`
    /// 备选时,本方法返回 plaintext 备选;反之,当 entry 不存在 plaintext
    /// 备选时,行为退化为 paste 入口。
    ///
    /// 无任何 entry 时返回 `Ok(None)`,与 paste 入口语义一致。
    async fn latest_plain_text_preferred_representation(
        &self,
    ) -> Result<Option<LatestPasteRepresentation>, LatestClipboardSnapshotError>;
}

#[derive(Debug, Error)]
pub enum LatestClipboardSnapshotError {
    /// 底层 storage 路径失败 —— sqlite 异常 / blob 读不出 / selection 与
    /// representation 行不一致等。adapter 把具体错文本带过来给排障用,但
    /// use case 不依赖错误细节,统一翻成应用层错误后路由层 → HTTP 500。
    #[error("latest clipboard snapshot resolution failed: {0}")]
    Resolution(String),
}

// ─── lan listener lifecycle ─────────────────────────────────────────────
//
// 为什么需要这一组类型:LAN 监听器的"开/关/换端口"过去靠装配期一次性决定,
// 任何设置变更都要求重启进程才能生效。把"目标状态"抽成一个可被运行时反复
// 调用的 port,才让设置层在不知道 adapter 细节的情况下推一次按钮就让监听
// 器状态立刻对齐到期望值。

/// 期望的 LAN 监听器运行时状态。
///
/// 这是 [`MobileLanLifecyclePort::apply`] 的入参类型 —— 调用方只说"我要它变成
/// 什么", 不说"具体怎么 bind / 绑哪个网卡"(那是 adapter 的实现细节)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobileLanTarget {
    /// 不对外暴露任何 LAN 监听器。若 adapter 当前有正在运行的监听器,必须把
    /// 它停掉并释放底层资源(端口、句柄等)。
    Disabled,

    /// 在指定端口上对外暴露监听器。`port` ∈ `1..=65535`。
    ///
    /// 若 adapter 当前无监听器, start;若已有同端口监听器, no-op;若端口不同,
    /// 先 stop 旧的再 start 新的。
    Enabled {
        /// 监听端口。adapter 自行决定 bind 哪些 IP(典型实现绑 `0.0.0.0`,
        /// 由 OS 路由分发)。
        port: u16,
    },
}

/// 把对外暴露的 LAN 监听器状态对齐到期望值。
///
/// # 这个 port 解决什么问题
///
/// 给"设置层 / 装配层"一个**单点、幂等**的切换入口:不管当前监听器是开/关/
/// 在哪个端口, 调用方只传"我要它变成什么", port 自己负责推进到那个状态。
/// 没有这个 port, 调用方就得自己跟踪当前监听器句柄、判断是该 start 还是
/// stop 还是 rebind, 错综复杂且易出现"两边状态不一致"的 bug。
///
/// # 语义
///
/// `apply` 是 **幂等** 的状态对齐:多次以同一 target 调用与单次调用等价。
/// 调用方不需要先查"现在是什么状态",直接传期望值即可。
///
/// # 状态机
///
/// 当前状态 × 目标状态的合法行为:
///
/// | 当前 \ 目标         | `Disabled` | `Enabled { port }` |
/// | ------------------- | ---------- | ------------------ |
/// | 未运行              | no-op      | start              |
/// | 运行中, 同端口      | stop       | no-op              |
/// | 运行中, 不同端口    | stop       | stop + start       |
///
/// # 错误语义
///
/// 本方法 **不返回错误**。adapter 内部 bind 失败(端口占用、IP 不可分配、
/// 权限不足等)必须经其它通道反馈给观察者(典型:`MobileSyncEndpointInfoPort`
/// 的 `BindFailed{reason}` 三态),而不是把错误回传给调用方 —— 因为调用方
/// 通常已经在持久化层提交了新设置, port 失败不应让设置回滚,只应让观察者
/// 看到"配置已生效但 bind 失败"。
///
/// # 并发
///
/// 同时刻只能有一个 `apply` 在生效;adapter 自行做串行化(典型:内部 mutex)。
#[async_trait]
pub trait MobileLanLifecyclePort: Send + Sync {
    async fn apply(&self, target: MobileLanTarget);
}

// ─── mobile file staging ────────────────────────────────────────────────

/// 把 mobile 入站(`PUT /file/{name}`)收到的裸字节物化到本机文件系统,
/// 返回一个可直接拼 `text/uri-list` rep 的 [`StagedFile`]。
///
/// 这是 mobile sync `File` 类型(SyncClipboard 协议)能走通入站管线的关
/// 键 —— 项目 file-list rep 的 wire 形态是 `\n` 分隔的 `file:///...` URI
/// list,接收端必须把 iPhone 上传的字节先落盘,才能拼出本机可寻址的 URI。
///
/// **scope_id**:由 use case 决定的"逻辑分组"标识 —— adapter 把同一次入
/// 站事件的所有文件都放进同一个 scope 子目录(典型:每次 PUT /SyncClipboard.json
/// 触发的 staging 用一个 nonce),便于运维 / 清理。
///
/// **跨平台 URI**:adapter 必须保证 `StagedFile.uri` 在 macOS / Linux 形如
/// `file:///path/...`,在 Windows 形如 `file:///C:/path/...`(参考
/// `url::Url::from_file_path`),含 spaces / non-ASCII 的文件名要 percent-
/// encode。use case 不关心细节, 单测用 mock 注入预期 URI 字符串即可。
///
/// **清理职责**:adapter 负责生命周期 —— 进程启动时清空残留 / 后台 sweep /
/// 进程退出时清理。use case 不持有路径,也不调用任何"释放"接口。
#[async_trait]
pub trait MobileFileStagingPort: Send + Sync {
    /// 把 `bytes` 写到 staging 区,产出可拼 file-list rep 的 [`StagedFile`]。
    ///
    /// `data_name` 来自 iPhone 上传的 wire 字段(可能含路径分隔符 / `..` /
    /// 控制字符等不安全片段),adapter 必须 sanitize 成安全的 basename;
    /// `mime` 仅用于排障日志,不参与文件落盘行为。
    async fn stage_file(
        &self,
        scope_id: &str,
        data_name: &str,
        mime: &str,
        bytes: Vec<u8>,
    ) -> Result<StagedFile, MobileFileStagingError>;

    /// 按 `file:///...` URI 读回字节(出站 `GET /file/{dataName}` 用)。
    ///
    /// 安全语义:信任来源是 OS 剪贴板 —— 任何能被 paste rep 携带的 file URI,
    /// 在桌面 OS 层面已对所有运行中 app 开放读权限(用户主动复制 = 主动授权)。
    /// 已配对的 iPhone 经 basic auth 通过后,语义上等价于一台已信任的设备,
    /// 可读这台机器剪贴板里的任意 file URI。adapter 不再做路径白名单 ——
    /// 真实用户场景下盘符 / 外接卷 / 用户自定义目录千奇百怪,白名单几乎不
    /// 可能穷举且静默拒绝带来的 0 字节体验比安全收益更糟。
    ///
    /// 错误形态:
    /// - URI 不合法 / 无法解析为 path → `Io` 变体(text 描述错误);
    /// - 文件不存在 → `NotFound`;
    /// - 读盘失败(权限 / 中途 IO 错) → `Io`。
    ///
    /// adapter **不**负责 mime 推断;use case 端按 dataName 扩展名 / SyncClipboard
    /// 协议默认 (`application/octet-stream`) 决定 wire mime。
    async fn read_by_uri(&self, uri: &str) -> Result<Vec<u8>, MobileFileStagingError>;

    /// 开启一段"分块写入"的 staging 会话,返回一个不透明 [`StagingHandle`]
    /// 供后续 `append_stage_chunk` / `finalize_stage` / `abort_stage` 使用。
    ///
    /// 与 [`Self::stage_file`] 的区别:全量字节模式要求调用方先把整个 payload
    /// 装入内存,本方法允许在收字节的过程中边收边落盘,内存占用与单个 chunk
    /// 同阶。`data_name` 的 sanitize 与 `scope_id` 的目录隔离语义与 `stage_file`
    /// 完全等价。`mime` 仅用于排障日志,不参与文件落盘行为。
    ///
    /// 错误形态:
    /// - `InvalidDataName`:sanitize 后兜底仍失败(实际很难触发);
    /// - `Io`:mkdir / 创建文件失败。
    ///
    /// 会话生命周期由 handle 唯一界定 —— adapter 保证:
    /// - 同一 handle 的资源(打开的 fd / 临时 path)在 `finalize_stage` 或
    ///   `abort_stage` 返回后必定释放;
    /// - 在 begin 与 finalize/abort 之间崩溃的 handle 视为 abandoned,
    ///   adapter 可在后续清理周期回收。
    async fn begin_stage(
        &self,
        scope_id: &str,
        data_name: &str,
        mime: &str,
    ) -> Result<StagingHandle, MobileFileStagingError>;

    /// 把 `chunk` 追加写入由 `handle` 标识的 staging 会话。
    ///
    /// 单笔会话只允许**串行** append;同一 handle 的并发 append 语义未定义。
    /// chunk 大小由调用方决定,adapter 不做窗口聚合;0 字节 chunk 合法且为 no-op。
    ///
    /// 错误形态:
    /// - `Io`:写盘 / 句柄无效(handle 已 finalize/abort 过 / handle 从未由本
    ///   adapter 颁发)。无独立"未知 handle"变体 —— 协议违规 ≈ IO 故障,
    ///   都翻成应用层 `Internal`。
    ///
    /// 失败后会话状态由 adapter 决定;调用方在收到 `Err` 后应立即调
    /// `abort_stage` 释放资源,不应再次调本方法。
    async fn append_stage_chunk(
        &self,
        handle: &StagingHandle,
        chunk: &[u8],
    ) -> Result<(), MobileFileStagingError>;

    /// 结束一段 staging 会话,消费 `handle`,产出可拼 file-list rep 的
    /// [`StagedFile`]。
    ///
    /// 语义:adapter 保证返回前已 `flush` + `sync_all`,落盘对后续 reader
    /// 可见。失败时 adapter 自行回收已落盘的部分数据。
    ///
    /// 错误形态:
    /// - `Io`:flush / fsync / URI 派生失败 / handle 已被消费过。
    async fn finalize_stage(
        &self,
        handle: StagingHandle,
    ) -> Result<StagedFile, MobileFileStagingError>;

    /// 放弃一段 staging 会话,消费 `handle`,best-effort 释放资源(关句柄、
    /// 删半写入的文件)。
    ///
    /// **不**返回 `Result`:调用本方法必然处在已经失败的路径上(客户端断流 /
    /// chunk 写盘失败 / 上层取消),二次失败只值得 warn 一行,不应再扰动
    /// 上层错误处理。
    ///
    /// 幂等性:重复 abort 同一 handle 行为 = 静默 no-op(adapter 内部 entry
    /// 已不存在),不报错。
    async fn abort_stage(&self, handle: StagingHandle);
}

#[derive(Debug, Error)]
pub enum MobileFileStagingError {
    /// 写盘 / mkdir / URI 派生 / URI 解析失败。adapter 把底层错误文本带过
    /// 来,use case 一律翻成应用层 `Internal` 后路由 → HTTP 500。
    #[error("mobile file staging IO failure: {0}")]
    Io(String),

    /// `data_name` sanitize 后落空(全是非法字符),adapter 已 fallback 到
    /// 兜底名仍失败时返回。实际场景几乎不会触发 —— 保留此变体让 use case
    /// 能按"业务输入不合法"语义翻译,不与 IO 错误混淆。
    #[error("staged data_name unusable after sanitize: {0}")]
    InvalidDataName(String),

    /// `read_by_uri` 专用:URI 指向的 path 不存在。use case 翻成应用层
    /// `NotFound` 后路由 → HTTP 404。
    #[error("staged URI not found")]
    NotFound,
}
