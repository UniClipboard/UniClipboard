use thiserror::Error;

/// Application-layer errors for file-transfer orchestration.
///
/// 文件传输应用层错误。
///
/// 这些错误只表达“这次应用动作为什么不能继续”，
/// 不承担底层存储实现或传输实现的细节语义。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FileTransferApplicationError {
    /// 持久化历史事件失败。
    #[error("file transfer event store failed: {0}")]
    Store(String),
    /// 向外发布事件失败。
    #[error("file transfer event publishing failed: {0}")]
    Publish(String),
    /// 操作 receiver-side projection 表失败（例如 `link_transfer_to_entry`）。
    #[error("file transfer repository failed: {0}")]
    Repository(String),
    /// 传输尚未开始，不能继续推进。
    #[error("transfer `{transfer_id}` has not been started")]
    TransferNotStarted { transfer_id: String },
    /// 传输已经开始过，不能重复开始。
    #[error("transfer `{transfer_id}` has already been started")]
    TransferAlreadyStarted { transfer_id: String },
    /// 传输已经进入结束态，不能继续推进。
    #[error("transfer `{transfer_id}` has already finished")]
    TransferAlreadyFinished { transfer_id: String },
    /// 当前操作的对端与历史记录里的对端不一致。
    #[error(
        "transfer `{transfer_id}` belongs to peer `{expected_peer_id}`, not `{actual_peer_id}`"
    )]
    PeerMismatch {
        transfer_id: String,
        expected_peer_id: String,
        actual_peer_id: String,
    },
    /// 已存储的事件历史本身不合法，无法恢复出可信状态。
    #[error("transfer `{transfer_id}` history is invalid: {message}")]
    InvalidHistory {
        transfer_id: String,
        message: String,
    },
    /// 进度出现倒退，说明输入和当前状态不一致。
    #[error(
        "transfer `{transfer_id}` progress moved backwards from {previous_bytes} to {new_bytes}"
    )]
    ProgressWentBackwards {
        transfer_id: String,
        previous_bytes: u64,
        new_bytes: u64,
    },
}
