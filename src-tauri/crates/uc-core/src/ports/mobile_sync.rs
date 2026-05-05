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
    LanEndpointInfo, MintedToken, MobileDevice, MobileDeviceError, MobileDeviceId,
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
