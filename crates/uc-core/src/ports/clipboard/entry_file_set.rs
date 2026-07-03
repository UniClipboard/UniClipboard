//! Entry file-set 仓储端口。
//!
//! 为什么需要这个端口:
//! 一条文件类 entry 的逐行清单是独立于 entry / event 内容的领域事实,
//! 它的写入路径(身份确定后落盘一份完整清单)、查询路径(按 entry 取回
//! 整份清单)、清理路径(随 entry 删除 cascade)都与既有 entry / event
//! 仓储无关。把它从既有仓储中分离,保持每个端口职责单一。

use async_trait::async_trait;

use crate::clipboard::{EntryFileSet, EntryFileSetError};
use crate::ids::EntryId;

/// 文件清单的持久化端口。
///
/// 契约:
/// - 一条 `entry_id` 至多对应一份清单;`save` 是整体替换语义——同一
///   `entry_id` 的再次 `save` 会丢弃之前保存的清单,不是增量合并。
/// - `load` 对没有清单的 entry 返回 `Ok(None)`,不视为错误。
/// - entry 被删除时,其关联的清单应一并被清理,具体由实现侧保证;
///   调用方不负责显式清理。
#[async_trait]
pub trait EntryFileSetRepositoryPort: Send + Sync {
    /// 保存某条 entry 的完整清单,整体替换该 entry 之前保存的任何清单。
    async fn save(
        &self,
        entry_id: &EntryId,
        file_set: &EntryFileSet,
    ) -> Result<(), EntryFileSetError>;

    /// 取回某条 entry 的清单;不存在则返回 `Ok(None)`。
    async fn load(&self, entry_id: &EntryId) -> Result<Option<EntryFileSet>, EntryFileSetError>;
}
