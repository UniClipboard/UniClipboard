//! Switch-space 重加密迁移使用的剪贴板事件批量读写 port。
//!
//! 与常态的 [`super::clipboard_event_repository::ClipboardEventRepositoryPort`]
//! 隔离：
//!
//! * 那个 port 服务运行时的"读单条 representation"路径，签名小、单调用。
//! * 本 port 服务一次性的迁移流程，需要"列出所有受影响行 / 写入 backup
//!   集合 / 清空 backup 集合 / 覆盖主表 inline_data"等批量操作，访问形态
//!   与日常完全不同。
//!
//! 之所以拆成独立 port，是为了让普通业务代码不能误碰"backup 集合 /
//! 覆盖 inline_data"这种迁移专用语义；同时也让无关 adapter 不必为日常路径
//! 实现这套大批量接口。

use async_trait::async_trait;

use crate::ids::{EventId, RepresentationId};

/// `BlobMigrationRepoPort` 操作失败原因。
///
/// 粒度对齐 `MembershipError::Repository`——调用方一般只关心"出问题了"
/// 和"目标行已经不存在"两类，详细原因由 adapter 写日志。
#[derive(Debug, thiserror::Error)]
pub enum BlobMigrationRepoError {
    /// 后端存储不可用（DB 连接断、磁盘满、事务冲突等）。
    #[error("storage failure: {0}")]
    Storage(String),

    /// 其它内部错误（serde 失败、不可恢复的逻辑错）。
    #[error("blob migration repo internal error: {0}")]
    Internal(String),
}

/// 一条迁移备份记录：迁移开始时把每个 representation 的 inline_data
/// 用 migration_key 重加密后写进备份集合，handshake 完成后再读回来用
/// 新 master_key 重新加密回主表。
///
/// AAD 由调用方在 use-case 层按 `aad::for_inline(event_id, repr_id)`
/// 重建——不在 record 里冗余存放，避免落盘格式与 AAD 派生规则双源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationRecord {
    pub event_id: EventId,
    pub representation_id: RepresentationId,
    /// 用 migration_key 加密后的字节，wire format 与
    /// [`super::super::security::BlobCipherPort`] 的 `Ciphertext` 一致
    /// （`serde_json::to_vec(&EncryptedBlob)`）。
    pub migration_ciphertext: Vec<u8>,
}

/// 剪贴板重加密迁移仓库。
///
/// 生命周期：迁移开始时填充备份集合（`upsert_record` × N），迁移结束时
/// 清空（`discard_all_records`）。调用方为 `SwitchSpaceUseCase`，常态业务
/// 代码不应触碰本 port。
#[async_trait]
pub trait BlobMigrationRepoPort: Send + Sync {
    /// 列出主表中所有目前持有 inline_data 的 representation——也就是
    /// phase 1 必须备份的全集。空 inline_data（纯 blob_id 引用）的行会
    /// 被排除：那些数据由 `BlobRepository` 负责，不在本 port 职责内。
    async fn list_main_inline_representations(
        &self,
    ) -> Result<Vec<(EventId, RepresentationId)>, BlobMigrationRepoError>;

    /// 读取主表上单条 representation 的 inline_data 密文字节。返回
    /// `None` 表示行已被并发删掉、或者 inline_data 为空（已经迁到 blob）。
    async fn read_main_inline_data(
        &self,
        event_id: &EventId,
        representation_id: &RepresentationId,
    ) -> Result<Option<Vec<u8>>, BlobMigrationRepoError>;

    /// 把一条迁移备份记录写入备份集合。按 `(event_id, representation_id)`
    /// 幂等——phase 1 因故重跑时会覆盖旧备份而不是追加。
    async fn upsert_record(&self, record: &MigrationRecord) -> Result<(), BlobMigrationRepoError>;

    /// 备份集合当前条数。phase 3 跑进度查询时用作分母。
    async fn count_records(&self) -> Result<u64, BlobMigrationRepoError>;

    /// 拉出全部备份记录供 phase 3 消费。返回 `Vec` 而不是 stream 是
    /// 折中：剪贴板历史规模通常在 KB-MB 级，全量返回简单且测试友好；
    /// 后续真的撑不住再换成游标接口。
    async fn list_records(&self) -> Result<Vec<MigrationRecord>, BlobMigrationRepoError>;

    /// 用新密文原子覆盖主表上单条 representation 的 inline_data。
    /// phase 3 流式调用：每条都解密 → 加密 → 立即覆盖。
    async fn update_main_inline_data(
        &self,
        event_id: &EventId,
        representation_id: &RepresentationId,
        new_ciphertext: &[u8],
    ) -> Result<(), BlobMigrationRepoError>;

    /// 清空备份集合。phase 4 cleanup 调用，启动期检测到孤立 `Prepared`
    /// 状态时也会调用以放弃迁移。幂等：空集合再清空不报错。
    async fn discard_all_records(&self) -> Result<(), BlobMigrationRepoError>;
}
