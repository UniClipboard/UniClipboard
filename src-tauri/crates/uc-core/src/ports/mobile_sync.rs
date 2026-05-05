//! 移动端同步所需的端口抽象。
//!
//! 这些 trait 仅描述"应用层在颁发 token / 持久化设备 / 探测当前 LAN
//! 端点 / 管理一次性下载凭据时需要外部具备的能力"，不涉及任何具体技术
//! 实现（OS RNG、SQLite、网卡探测等）。具体实现由 `uc-infra` /
//! `uc-platform` / `uc-application` 中的 adapter 承担。
//!
//! 设计参考 `.context/mobile-sync/SPEC.md` §4 / §7。

use async_trait::async_trait;
use thiserror::Error;

use crate::mobile_sync::{
    LanEndpointInfo, LanInterface, MintedToken, MobileDevice, MobileDeviceError, MobileDeviceId,
    RegisteredDownloadToken, ShortcutDownloadToken, TokenHash,
};

// ─── token minter ────────────────────────────────────────────────────────

/// 颁发 mobile 设备的 token 与稳定 device id。
///
/// 同步而非异步：底层只是 `OsRng + SHA-256 + hex` 的纯计算，没必要扛上
/// `async` 的成本。
///
/// 把 token 与 device id 合并为同一个 minter 是有意为之 —— 二者都是"登
/// 记一台 mobile 设备时颁发的不可猜凭据"，单一职责且来自同一熵源更易
/// 推理。
pub trait MobileTokenMinterPort: Send + Sync {
    /// 生成一对全新的 token 信息。
    ///
    /// 实现必须保证：
    /// 1. `raw_hex` 是 64 字符的小写 hex（即 32 字节随机的 hex 编码）
    /// 2. `hash` 是 `raw_hex` 对应原始 32 字节的 SHA-256
    /// 3. `device_id` 形如 `did_<32hex>`，与 `raw_hex` 相互独立（不共享熵）
    fn mint_token(&self) -> MintedToken;
}

// ─── device repository ───────────────────────────────────────────────────

/// 已登记 mobile 设备的持久化能力。
///
/// 鉴权热路径调用 `find_by_token_hash` —— adapter 必须确保有 hash 索引；
/// 删除路径在撤销 / 解绑时调用，需要立即生效（不能走异步队列）。
#[async_trait]
pub trait MobileDeviceRepositoryPort: Send + Sync {
    /// 持久化一台新设备。重复 device_id / token_hash 应返回对应的领域错误。
    async fn save(&self, device: &MobileDevice) -> Result<(), MobileDeviceError>;

    /// 鉴权热路径：根据 token 哈希定位设备。
    async fn find_by_token_hash(
        &self,
        token_hash: &TokenHash,
    ) -> Result<Option<MobileDevice>, MobileDeviceError>;

    /// 列表 / 撤销 UI 用：按 device id 精确查询。
    async fn find_by_device_id(
        &self,
        device_id: &MobileDeviceId,
    ) -> Result<Option<MobileDevice>, MobileDeviceError>;

    /// 列出全部设备 —— v1 不分页，预期数量很小（个位数）。
    async fn list_all(&self) -> Result<Vec<MobileDevice>, MobileDeviceError>;

    /// 删除一条记录。返回 `true` 表示真实删掉了一行；`false` 表示原本就
    /// 不存在（撤销操作幂等）。
    async fn delete(&self, device_id: &MobileDeviceId) -> Result<bool, MobileDeviceError>;

    /// 鉴权链路成功后回写最近活跃信息 —— 仅运维 / UI 用。失败不应阻塞业
    /// 务请求，调用方决定是否吞错。
    async fn record_activity(
        &self,
        device_id: &MobileDeviceId,
        last_seen_at_ms: i64,
        last_seen_ip: Option<String>,
        reported_name: Option<String>,
        reported_os: Option<String>,
    ) -> Result<(), MobileDeviceError>;
}

// ─── endpoint info ───────────────────────────────────────────────────────

/// 探测 daemon 当前对外暴露的 LAN 端点。
///
/// 抽象出来是因为 daemon 启停 / 配置变更后端点会动；登记设备的 use case
/// 需要拿到"现在能用"的 URL，而不是配置里写的目标 URL。当 LAN 监听未
/// 启用时返回 `Ok(None)`，由 use case 翻译成业务错误。
#[async_trait]
pub trait MobileSyncEndpointInfoPort: Send + Sync {
    async fn current_lan_endpoint(&self) -> Result<Option<LanEndpointInfo>, EndpointInfoError>;
}

#[derive(Debug, Error)]
pub enum EndpointInfoError {
    #[error("endpoint info storage failure: {0}")]
    Storage(String),
}

// ─── shortcut download token store ───────────────────────────────────────

/// 一次性 `.shortcut` 下载凭据的短 TTL 缓存。
///
/// 用作"创建设备 → iPhone Safari 下载 .shortcut"中间的安全旁路：登记 use
/// case 把打包好的字节加注一份临时 token，iPhone Safari 去 `/install?dt=…`
/// 一次性领走。由 in-process adapter 维护即可（典型实现：`tokio::Mutex<
/// HashMap>` + 后台过期清理任务）。
///
/// 进程重启即丢失被认为是可接受的：未消费的 token 自然作废，用户重新点
/// 一次"添加 iPhone"即可。
#[async_trait]
pub trait ShortcutDownloadTokenStorePort: Send + Sync {
    /// 注册一份待领取的 .shortcut 字节流，返回带过期时间的 token。
    /// `payload` 由 use case 提前打包好，store 不解释其内容。
    async fn register(
        &self,
        device_id: MobileDeviceId,
        payload: Vec<u8>,
        ttl_ms: i64,
    ) -> Result<RegisteredDownloadToken, ShortcutDownloadTokenError>;

    /// 一次性消费：返回该 token 关联的 (device_id, payload) 并立即作废。
    /// `Ok(None)` 表示 token 不存在 / 已被消费 / 已过期 —— store 不区分，
    /// 上层只关心"能不能领"。
    async fn consume(
        &self,
        token: &ShortcutDownloadToken,
    ) -> Result<Option<(MobileDeviceId, Vec<u8>)>, ShortcutDownloadTokenError>;
}

#[derive(Debug, Error)]
pub enum ShortcutDownloadTokenError {
    #[error("download token store internal failure: {0}")]
    Internal(String),
}

// ─── nonce store（鉴权防重放滑动窗口）──────────────────────────────────

/// 鉴权链路的 nonce 滑动窗口缓存。
///
/// LAN HTTP 中间件每接到一个请求都会校验 `X-UC-Nonce` 头：先看是否在窗
/// 口内见过（重放），再插入并以"观测时间"作 TTL 起点（典型 60s）。窗口
/// 满时返回 `CacheFull`，由 middleware 翻成 503，避免被构造大量随机
/// nonce 拖死内存。
///
/// 这里只描述"见过没"——不关心 token / 设备身份；身份归
/// `MobileDeviceRepositoryPort`。能力切得小是因为 nonce 缓存的实现策略
/// （进程内 vs 跨进程 vs Redis）独立于设备仓储演进。
///
/// 采用 `record_if_new` 单原子操作而不是 `contains` + `insert` 两步：
/// 后者在并发中会出现"两个 worker 同时看到 false 然后双双 insert"的
/// 时间窗口，原子操作把"判断 + 写入"收口到 adapter 内部。
#[async_trait]
pub trait NoncePort: Send + Sync {
    /// 原子地"如果未见过则登记并返回 true；否则返回 false"。
    ///
    /// `observed_at_ms` 由调用方传入的"now"——adapter 用它驱动 lazy GC，
    /// 也用它作每条记录的过期参考。
    ///
    /// 返回 `Err(NonceError::CacheFull)` 表示窗口已满，调用方应放弃
    /// 当前请求（503 nonce_cache_full）；返回 `Err(NonceError::Storage)`
    /// 表示底层存储异常（同样 503 nonce_cache_full，详情走日志）。
    async fn record_if_new(&self, nonce: &str, observed_at_ms: i64) -> Result<bool, NonceError>;
}

#[derive(Debug, Error)]
pub enum NonceError {
    /// 滑动窗口已满。adapter 不再接受新条目，应让上层翻成 503。
    #[error("nonce cache full")]
    CacheFull,
    /// 底层存储异常（adapter-specific）。带文本以便排障。
    #[error("nonce storage failure: {0}")]
    Storage(String),
}

// ─── lan interface probe ────────────────────────────────────────────────

/// 枚举本机当前的 LAN 网卡 IPv4 地址。
///
/// 用于"添加 iPhone"流程：UI 让用户从可用 IP 中挑一个，daemon 据此拼出
/// 二维码里的 LAN URL。返回的列表是"adapter 看到的全部 IPv4 接口"——是否
/// 排除 loopback / link-local / VPN-overlay / CGNAT 等由 application 层 use
/// case 按当前产品策略过滤，便于以后随设置（如
/// `NetworkSettings.allow_overlay_network_addrs`）调整而无需改 adapter。
///
/// 同步而非异步：实现里就是一次 syscall，没必要扛 async 成本。但保留
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
