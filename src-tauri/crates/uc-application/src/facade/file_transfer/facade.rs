//! [`FileTransferFacade`] —— 文件传输 lifecycle 应用层入口。
//!
//! ## 暴露的动作
//!
//! 每个公开方法对应一个 lifecycle use case 或 receiver-side projection
//! 维护操作：
//!
//! | 方法 | 对应 use case / 端口 | 语义 |
//! |---|---|---|
//! | [`FileTransferFacade::start`] | `StartTransferUseCase` | 启动一个新传输，落 `Started` 事件 |
//! | [`FileTransferFacade::report_progress`] | `ReportTransferProgressUseCase` | 上报进度，落 `Progress` 事件 |
//! | [`FileTransferFacade::complete`] | `CompleteTransferUseCase` | 标记传输完成，落 `Completed` 事件 |
//! | [`FileTransferFacade::fail`] | `FailTransferUseCase` | 标记传输失败，落 `Failed` 事件 |
//! | [`FileTransferFacade::cancel`] | `CancelTransferUseCase` | 取消传输，落 `Cancelled` 事件 |
//! | [`FileTransferFacade::link_transfer_to_entry`] | `FileTransferRepositoryPort::link_transfer_to_entry` | 把 projection 行重新关联到另一个 `entry_id`（`now_ms` 由内部 clock 提供） |
//! | [`FileTransferFacade::seed_receiver_context`] | `FileTransferRepositoryPort::upsert_pending_transfer` | 在 receiver-side projection 表里 upsert 一条 `pending` 行（接收方本地上下文，不进 domain event 总线） |
//!
//! ## 设计取舍
//!
//! - 5 个 lifecycle 动作各自有完整事件历史校验（见 `timeline::TransferTimeline`）；
//!   facade 不再做额外校验，直接转发 use case。
//! - `link_transfer_to_entry` / `seed_receiver_context` 走的是 receiver-side
//!   projection 端口而不是 domain 事件总线 —— 它们修改的是 receiver 本地
//!   投影状态（哪条 entry 拥有这个 transfer / 一条 pending 行先存在），
//!   不属于 transfer 本身的状态转移，没有对应的 domain event。

use std::sync::Arc;

use uc_core::file_transfer::{FileTransferEventPublisherPort, FileTransferEventStorePort};
use uc_core::ports::file_transfer_repository::PendingInboundTransfer;
use uc_core::ports::{ClockPort, FileTransferRepositoryPort};
use uc_core::FileTransferEvent;

use crate::file_transfer::{
    CancelTransfer, CancelTransferUseCase, CompleteTransfer, CompleteTransferUseCase, FailTransfer,
    FailTransferUseCase, FileTransferApplicationError, ReportTransferProgress,
    ReportTransferProgressUseCase, StartTransfer, StartTransferUseCase,
};

/// Re-associate a transfer projection row with a different `entry_id`.
///
/// 应用层输入：把 receiver-side `file_transfer` 表里的 transfer 行从
/// 旧的 `entry_id` 改挂到新的 `entry_id`。`now_ms` 由 facade 内部
/// `ClockPort` 提供，不暴露给调用方。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkTransferToEntry {
    pub transfer_id: String,
    pub entry_id: String,
}

/// Seed an initial pending row for an inbound transfer.
///
/// 应用层输入：在 receiver-side `file_transfer` 表里 upsert 一条 `pending`
/// 行，把 transfer_id → entry_id / filename / cached_path 的关系先落到
/// projection。`created_at_ms` 由 facade 内部 `ClockPort` 提供，不暴露
/// 给调用方。`cached_path` 仅在落盘场景有意义（free-standing file），
/// 内嵌进 representation 的 blob 路径填空字符串占位即可。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedReceiverContext {
    pub transfer_id: String,
    pub entry_id: String,
    pub origin_device_id: String,
    pub filename: String,
    pub cached_path: String,
}

/// 构造 [`FileTransferFacade`] 所需的依赖集合。
///
/// 由 bootstrap 在装配期填充：5 个 lifecycle use case 共享同一对
/// `store` + `publisher`；`repo` 用于 receiver-side projection 维护
/// 操作；`clock` 给 projection 写入打时间戳。
pub struct FileTransferFacadeDeps {
    pub store: Arc<dyn FileTransferEventStorePort>,
    pub publisher: Arc<dyn FileTransferEventPublisherPort>,
    pub repo: Arc<dyn FileTransferRepositoryPort>,
    pub clock: Arc<dyn ClockPort>,
}

/// 文件传输 lifecycle 应用层入口。
///
/// 包装 5 个 lifecycle use case + receiver-side projection 维护操作，
/// 让多条 inbound 路径能用同一组动作产出一致的事件流（domain timeline +
/// host event）。
pub struct FileTransferFacade {
    start_uc: Arc<StartTransferUseCase>,
    report_progress_uc: Arc<ReportTransferProgressUseCase>,
    complete_uc: Arc<CompleteTransferUseCase>,
    fail_uc: Arc<FailTransferUseCase>,
    cancel_uc: Arc<CancelTransferUseCase>,
    repo: Arc<dyn FileTransferRepositoryPort>,
    clock: Arc<dyn ClockPort>,
}

impl FileTransferFacade {
    pub fn new(deps: FileTransferFacadeDeps) -> Self {
        let start_uc = Arc::new(StartTransferUseCase::new(
            Arc::clone(&deps.store),
            Arc::clone(&deps.publisher),
        ));
        let report_progress_uc = Arc::new(ReportTransferProgressUseCase::new(
            Arc::clone(&deps.store),
            Arc::clone(&deps.publisher),
        ));
        let complete_uc = Arc::new(CompleteTransferUseCase::new(
            Arc::clone(&deps.store),
            Arc::clone(&deps.publisher),
        ));
        let fail_uc = Arc::new(FailTransferUseCase::new(
            Arc::clone(&deps.store),
            Arc::clone(&deps.publisher),
        ));
        let cancel_uc = Arc::new(CancelTransferUseCase::new(deps.store, deps.publisher));

        Self {
            start_uc,
            report_progress_uc,
            complete_uc,
            fail_uc,
            cancel_uc,
            repo: deps.repo,
            clock: deps.clock,
        }
    }

    pub async fn start(
        &self,
        input: StartTransfer,
    ) -> Result<FileTransferEvent, FileTransferApplicationError> {
        self.start_uc.execute(input).await
    }

    pub async fn report_progress(
        &self,
        input: ReportTransferProgress,
    ) -> Result<FileTransferEvent, FileTransferApplicationError> {
        self.report_progress_uc.execute(input).await
    }

    pub async fn complete(
        &self,
        input: CompleteTransfer,
    ) -> Result<FileTransferEvent, FileTransferApplicationError> {
        self.complete_uc.execute(input).await
    }

    pub async fn fail(
        &self,
        input: FailTransfer,
    ) -> Result<FileTransferEvent, FileTransferApplicationError> {
        self.fail_uc.execute(input).await
    }

    pub async fn cancel(
        &self,
        input: CancelTransfer,
    ) -> Result<FileTransferEvent, FileTransferApplicationError> {
        self.cancel_uc.execute(input).await
    }

    /// 把一条 transfer 重新关联到指定 `entry_id`。
    ///
    /// 返回 `true` 表示 receiver-side projection 表里有匹配行被更新；
    /// 返回 `false` 表示 `transfer_id` 还没被 seed —— 调用方自己决定
    /// 是当作错误，还是先 seed 再 link。
    pub async fn link_transfer_to_entry(
        &self,
        input: LinkTransferToEntry,
    ) -> Result<bool, FileTransferApplicationError> {
        let now_ms = self.clock.now_ms();
        self.repo
            .link_transfer_to_entry(&input.transfer_id, &input.entry_id, now_ms)
            .await
            .map_err(|err| FileTransferApplicationError::Repository(err.to_string()))
    }

    /// 在 receiver-side projection 表里 upsert 一条 `pending` 行。
    ///
    /// 用于：接收方在拿到要传输的元数据但 transfer 真正开始（`Started`
    /// 事件）之前，把 transfer_id → entry_id / filename / cached_path 的
    /// 关系先落到本地 projection；之后 `apply_event` 触发的状态转移就能
    /// 找到对应的 row，前端 hydrate / dashboard 列表也能立即看到这条
    /// transfer 的占位。
    ///
    /// 幂等：用同一个 `transfer.transfer_id` 多次调用等价于一次。
    /// `created_at_ms` 由 facade 内部 `ClockPort` 提供。
    pub async fn seed_receiver_context(
        &self,
        input: SeedReceiverContext,
    ) -> Result<(), FileTransferApplicationError> {
        let now_ms = self.clock.now_ms();
        self.repo
            .upsert_pending_transfer(&PendingInboundTransfer {
                transfer_id: input.transfer_id,
                entry_id: input.entry_id,
                origin_device_id: input.origin_device_id,
                filename: input.filename,
                cached_path: input.cached_path,
                created_at_ms: now_ms,
            })
            .await
            .map_err(|err| FileTransferApplicationError::Repository(err.to_string()))
    }
}
