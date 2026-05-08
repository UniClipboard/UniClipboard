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
    MobileDevice, MobileDeviceError, MobileDeviceId, StagedFile,
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

// ─── device repository ───────────────────────────────────────────────────

/// 已登记 mobile 设备的持久化能力(v3 改用 username 索引)。
///
/// 鉴权热路径调用 `find_by_username` —— adapter 必须确保有 username 索引;
/// 删除路径在撤销 / 解绑时调用,需要立即生效(不能走异步队列)。
#[async_trait]
pub trait MobileDeviceRepositoryPort: Send + Sync {
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

    /// 鉴权链路成功后回写最近活跃信息 —— 仅运维 / UI 用。失败不应阻塞业
    /// 务请求,调用方决定是否吞错。
    ///
    /// `reported_name` / `reported_os` 在 SyncClipboard 协议下永远是 `None`
    /// (shortcut 不上报);保留参数以备 v2 ClipboardAuto 客户端扩展。
    async fn record_activity(
        &self,
        device_id: &MobileDeviceId,
        last_seen_at_ms: i64,
        last_seen_ip: Option<String>,
        reported_name: Option<String>,
        reported_os: Option<String>,
    ) -> Result<(), MobileDeviceError>;

    /// 替换某台设备的 `password_hash`。其它字段保持不变。
    ///
    /// 用于密码轮换(rotate)用例 —— 用户在 UI 上请求"换一份新密码",
    /// application 层先 hash 新明文,再调本方法把 PHC 字符串换掉。
    ///
    /// 返回 `Ok(true)` 表示真实更新了一行;`Ok(false)` 表示 device_id 不
    /// 存在(用户已撤销 / UI 列表过期 —— application 层据此提示刷新)。
    /// 这与 [`Self::delete`] 的"返回值表达存在性"语义保持一致,避免应用
    /// 层为这种 race 单独处理。
    async fn update_password_hash(
        &self,
        device_id: &MobileDeviceId,
        new_password_hash: String,
    ) -> Result<bool, MobileDeviceError>;
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
}

#[derive(Debug, Error)]
pub enum LatestClipboardSnapshotError {
    /// 底层 storage 路径失败 —— sqlite 异常 / blob 读不出 / selection 与
    /// representation 行不一致等。adapter 把具体错文本带过来给排障用,但
    /// use case 不依赖错误细节,统一翻成应用层错误后路由层 → HTTP 500。
    #[error("latest clipboard snapshot resolution failed: {0}")]
    Resolution(String),
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
