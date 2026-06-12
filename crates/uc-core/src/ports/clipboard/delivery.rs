//! Entry delivery 仓储端口。
//!
//! 为什么需要这个端口:
//! 投递结果是一类独立于 entry / event 内容的领域事实,它的写入路径(发送
//! 完成时落盘一条结果)、查询路径(按 entry 反查所有目标的状态)、清理
//! 路径(随 entry 删除 cascade)都与既有 entry / event 仓储无关。把它从既
//! 有仓储中分离,保持每个端口职责单一。

use async_trait::async_trait;

use crate::clipboard::{EntryDeliveryError, EntryDeliveryRecord};
use crate::ids::EntryId;

/// 投递结果的持久化端口。
///
/// 契约:
/// - `(entry_id, target_device_id)` 二元组在底层至多有一行;`record_attempt`
///   按"最新结果覆盖旧结果"的语义写入(upsert)。
/// - `list_by_entry` 返回该 entry 的全部已记录目标,顺序无保证,调用方需
///   要稳定顺序时自行排序。
/// - entry 被删除时,其关联的投递记录应一并被清理,具体由实现侧保证;
///   调用方不负责显式清理。
/// - `target_device_id` 不引用任何当前可信对端集合;即使对端关系后续
///   被解除,历史记录仍可保留,过滤"已离开对端"的视图层职责不在本端口。
#[async_trait]
pub trait EntryDeliveryRepositoryPort: Send + Sync {
    /// 写入或覆盖一次投递结果。幂等:相同 `(entry_id, target_device_id)`
    /// 的多次调用,最终保存的是最后一次传入的 `status` 与 `reason_detail`。
    async fn record_attempt(&self, record: &EntryDeliveryRecord) -> Result<(), EntryDeliveryError>;

    /// 列出某条 entry 已记录的所有目标投递结果。返回空集合表示该 entry
    /// 至今没有任何投递记录(可能尚未广播,也可能确实没有可投递目标)。
    async fn list_by_entry(
        &self,
        entry_id: &EntryId,
    ) -> Result<Vec<EntryDeliveryRecord>, EntryDeliveryError>;
}
