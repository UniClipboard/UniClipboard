//! Switch-space 迁移状态机持久化 port。
//!
//! 与 [`super::setup_status::SetupStatusPort`] 是一对独立持久化 port——
//! `SetupStatus.has_completed` 表达"本机是否已经完成首次 setup"，
//! 而 `MigrationPhase` 表达"是否正处于一次 switch-space 迁移流程的中间态"。
//! 两件事独立，分两个 port 是为了：
//!
//! 1. 不污染 `SetupStatus` 已经稳定的 wire/磁盘格式，避免所有现有 fake / repo
//!    实现都要被动改字段。
//! 2. 迁移状态生命周期短（迁移完成即清回 `None`），与 setup 完成事实的
//!    永久性语义错位，分开更好。
//!
//! 持久化推荐用与 `SetupStatusPort` 同一份后端（设置文件 / SQLite /
//! tauri-plugin-store 等），但本 port 不强制要求。

use async_trait::async_trait;

use crate::setup::migration::MigrationPhase;

/// `MigrationStatePort` 操作失败原因。
#[derive(Debug, thiserror::Error)]
pub enum MigrationStateError {
    /// 持久化层不可用（磁盘满、文件被锁、进程权限缺失等）。
    #[error("storage failure: {0}")]
    Storage(String),

    /// 其它内部错误（serde、逻辑不可恢复）。
    #[error("migration state internal error: {0}")]
    Internal(String),
}

/// 当前 switch-space 迁移阶段的读写 port。
///
/// 方法契约：
/// * `get_current` 在没有进行中的迁移时返回 `Ok(None)`，不应该报错。
/// * `set_current(Some(..))` 推进阶段：调用方负责按
///   `Prepared → HandshakeDone → Swapped` 顺序写入。
/// * `set_current(None)` 标志迁移结束（成功完成 phase 4 或主动放弃）。
/// * 实现需在 daemon 重启间保留状态，让恢复路径能从最后一次落盘的阶段续跑。
#[async_trait]
pub trait MigrationStatePort: Send + Sync {
    async fn get_current(&self) -> Result<Option<MigrationPhase>, MigrationStateError>;

    /// 持久化当前迁移阶段。`None` 表示无在飞迁移。
    ///
    /// 取 owned 而非 `Option<&MigrationPhase>`：adapter 反正要 clone 出去做
    /// serde 序列化，调用方传 owned 时无额外开销；测试侧 `mockall` 替身
    /// 也避开了 `Option<&T>` 在 HRTB 上的处理难题。
    async fn set_current(&self, phase: Option<MigrationPhase>) -> Result<(), MigrationStateError>;
}
