//! `FirstSyncStatePort` —— 持久化"是否已上报过 first_clipboard_sync_* / first_file_sync_succeeded"
//! 的去重 flag。
//!
//! 这是产品 telemetry（issue #549）Activation 漏斗的领域端口。outbound
//! `dispatch_entry` use case 在 fan-out 每个 peer 的 spawn 内会先 mark 再决定
//! 是否额外 fire 三个 `first_*` 事件——`Ok(true)` 表示本次为首次置位（调用方
//! 应 fire 事件），`Ok(false)` 表示已被 mark（不 fire）。
//!
//! 端口语义保持极薄：
//! * 三个 method 是同一持久化资源的不同 fact（`attempted` / `succeeded` /
//!   `file_succeeded`），单 port 合理；不拆三个 port。
//! * 调用方无须先 read 再 write——`mark_*` 一次原语即返回是否首次置位，
//!   把 race 防护责任明确推到 port impl。
//! * 持久化格式与具体落点（独立小文件 / settings 子键 / DB）属于 infra 实现细节。
//!
//! Race 模型：fan-out N 个 peer 同时 spawn，每个都可能进入"我是不是首次"判断。
//! port impl 必须保证 read-check-write 是 critical section（典型实现：
//! `tokio::sync::Mutex` + 文件原子写）；调用方不需要在 use case 层套 atomic CAS。

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FirstSyncStateError {
    /// 读取失败（IO、权限、文件系统）。语义上等价于"无可信来源"，
    /// 调用方一般可以保守地 short-circuit 不 fire 事件，但仍要打日志。
    #[error("read first-sync state failed: {0}")]
    Read(String),
    /// 写入失败。flag 未持久化——下次进程启动还会被认为"未首次"，
    /// 可能导致同一事件被多次上报。调用方应记录日志便于排查。
    #[error("write first-sync state failed: {0}")]
    Write(String),
    /// 文件存在但内容损坏（非 UTF-8、JSON 不合法、schema 不识别等）。
    /// 与 `Read` 区分开是为了让 use case 可以选择"清理后重写"或仅日志告警。
    #[error("first-sync state content is corrupt: {0}")]
    Corrupt(String),
}

/// 首次同步事件去重端口。"是否已 fire 过 first_*"是 profile 范围内的事实，
/// 实现应落在与 `SetupStatusPort` / `AppVersionStatePort` 同等粒度的 profile 数据目录下。
///
/// 三个 method 互相独立：`mark_first_sync_attempted` 与
/// `mark_first_sync_succeeded` 各自管理一个 flag，failure 路径上首次 attempt
/// 也会留 attempted=true 的 funnel 漏点信号；`mark_first_file_sync_succeeded`
/// 仅在 `payload_type=File` 的首次成功路径上调用。
#[async_trait]
pub trait FirstSyncStatePort: Send + Sync {
    /// Mark "首次同步尝试" flag。
    ///
    /// * `Ok(true)` —— 本次为首次置位，调用方应 fire `first_clipboard_sync_attempted`。
    /// * `Ok(false)` —— flag 之前已被置位，调用方不 fire。
    /// * `Err(_)` —— IO / 损坏，调用方决定如何降级（推荐：log + 不 fire）。
    async fn mark_first_sync_attempted(&self) -> Result<bool, FirstSyncStateError>;

    /// Mark "首次同步成功" flag。
    ///
    /// * `Ok(true)` —— 本次为首次置位，调用方应 fire `first_clipboard_sync_succeeded`。
    /// * `Ok(false)` —— flag 之前已被置位，调用方不 fire。
    /// * `Err(_)` —— IO / 损坏，调用方决定如何降级。
    async fn mark_first_sync_succeeded(&self) -> Result<bool, FirstSyncStateError>;

    /// Mark "首次文件同步成功" flag。仅在 `payload_type=File` 的首次成功路径上调用。
    ///
    /// * `Ok(true)` —— 本次为首次置位，调用方应 fire `first_file_sync_succeeded`。
    /// * `Ok(false)` —— flag 之前已被置位，调用方不 fire。
    /// * `Err(_)` —— IO / 损坏，调用方决定如何降级。
    async fn mark_first_file_sync_succeeded(&self) -> Result<bool, FirstSyncStateError>;
}
