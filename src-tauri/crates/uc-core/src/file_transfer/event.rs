use serde::{Deserialize, Serialize};

/// Business-facing direction of a file transfer.
///
/// 文件传输的业务方向。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileTransferDirection {
    Sending,
    Receiving,
}

/// Business-facing progress snapshot for an active transfer.
///
/// 文件传输的业务进度快照。
///
/// 这里故意只保留 byte-level 进度语义：
/// - `bytes_transferred`
/// - `total_bytes`
///
/// 不保留 chunk-level 字段（如 `chunks_completed`、`total_chunks`），
/// 因为 chunk 是当前传输实现的技术细节，不属于 `uc-core` 应固化的业务模型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileTransferProgress {
    pub direction: FileTransferDirection,
    pub bytes_transferred: u64,
    /// Total bytes for the transfer when known.
    ///
    /// 传输总字节数；如果当前还未知，则为 `None`。
    pub total_bytes: Option<u64>,
}

/// Stable business reason for a failed file transfer.
///
/// 文件传输失败的稳定业务原因。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileTransferFailureReason {
    NetworkUnavailable,
    TimedOut,
    AccessDenied,
    StorageUnavailable,
    IntegrityCheckFailed,
    Unknown,
}

/// Stable business reason for a cancelled file transfer.
///
/// 文件传输取消的稳定业务原因。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileTransferCancellationReason {
    LocalUser,
    RemotePeer,
    Replaced,
    /// Inactivity sweep tore down the inbound fetch because no new bytes
    /// arrived within the configured pending / transferring timeout window.
    ///
    /// Carries no peer attribution — both ends are notified by the same
    /// event flow regardless of which side stalled.
    Timeout,
    Unknown,
}

/// File transfer domain events.
///
/// This event model captures business facts only. Transport details such as
/// chunk counters, local file paths, and raw infrastructure errors stay out of
/// this boundary.
///
/// 文件传输领域事件。
///
/// 这些事件只表达业务事实，不表达底层传输实现细节。
/// 例如：
/// - 可以表达“传输已开始”“传输已完成”
/// - 不应表达某个协议帧、chunk 编号、底层流实现或本地文件路径
///
/// `Started` 是领域 timeline 的唯一起点。接收端若需要在字节流到达前就让 UI
/// 看到一条“即将开始”的预告，属于表示层的呈现需要，由接收端 worker 直接通过
/// host-event 发 `pending` 状态，不进入领域事件模型。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileTransferEvent {
    /// The transfer content has started flowing.
    ///
    /// 文件内容传输已经开始。这是领域 timeline 的起点。
    Started {
        transfer_id: String,
        peer_id: String,
        filename: String,
        file_size: Option<u64>,
    },
    /// Business progress for an active transfer.
    ///
    /// 活跃传输的业务进度。
    Progress {
        transfer_id: String,
        peer_id: String,
        progress: FileTransferProgress,
    },
    /// The transfer has completed successfully.
    ///
    /// 传输已成功完成。
    Completed {
        transfer_id: String,
        peer_id: String,
    },
    /// The transfer has failed.
    ///
    /// 传输已失败。
    ///
    /// `detail` carries the optional free-text context produced at the failure
    /// site (for example, an underlying I/O error message). It is surfaced to
    /// the UI layer alongside the typed `reason` so users can see both the
    /// business category and the specific cause. The domain is intentionally
    /// agnostic about format and treats `detail` as opaque.
    Failed {
        transfer_id: String,
        peer_id: String,
        reason: FileTransferFailureReason,
        detail: Option<String>,
    },
    /// The transfer has been cancelled.
    ///
    /// 传输已被取消。
    Cancelled {
        transfer_id: String,
        peer_id: String,
        reason: FileTransferCancellationReason,
    },
}

impl FileTransferEvent {
    /// Create a `Started` event.
    ///
    /// 构造“传输已开始”事件。
    pub fn started(
        transfer_id: impl Into<String>,
        peer_id: impl Into<String>,
        filename: impl Into<String>,
        file_size: Option<u64>,
    ) -> Self {
        Self::Started {
            transfer_id: transfer_id.into(),
            peer_id: peer_id.into(),
            filename: filename.into(),
            file_size,
        }
    }

    /// Create a `Completed` event.
    ///
    /// 构造“传输已完成”事件。
    pub fn completed(transfer_id: impl Into<String>, peer_id: impl Into<String>) -> Self {
        Self::Completed {
            transfer_id: transfer_id.into(),
            peer_id: peer_id.into(),
        }
    }

    /// Create a `Failed` event.
    ///
    /// 构造“传输已失败”事件。
    pub fn failed(
        transfer_id: impl Into<String>,
        peer_id: impl Into<String>,
        reason: FileTransferFailureReason,
        detail: Option<String>,
    ) -> Self {
        Self::Failed {
            transfer_id: transfer_id.into(),
            peer_id: peer_id.into(),
            reason,
            detail,
        }
    }

    /// Create a `Cancelled` event.
    ///
    /// 构造“传输已取消”事件。
    pub fn cancelled(
        transfer_id: impl Into<String>,
        peer_id: impl Into<String>,
        reason: FileTransferCancellationReason,
    ) -> Self {
        Self::Cancelled {
            transfer_id: transfer_id.into(),
            peer_id: peer_id.into(),
            reason,
        }
    }
}
