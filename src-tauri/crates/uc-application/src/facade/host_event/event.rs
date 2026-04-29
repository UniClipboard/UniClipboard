use uc_core::file_transfer::FileTransferDirection;

/// 剪贴板内容来源。
#[derive(Debug, Clone)]
pub enum ClipboardOriginKind {
    Local,
    Remote,
}

/// 剪贴板子系统发给宿主的语义事件。
#[derive(Debug, Clone)]
pub enum ClipboardHostEvent {
    NewContent {
        entry_id: String,
        preview: String,
        origin: ClipboardOriginKind,
    },
    /// 一个 inbound clipboard 已确认即将到达 —— V3 envelope 已解码,
    /// blob 拉取还没开始 / 进行中。前端可凭这个事件在剪贴板列表里立刻
    /// 渲染一个占位卡片(用 entry_id 作 key),配合
    /// `TransferHostEvent::Progress` 显示进度;后续 `NewContent` 到达
    /// 时占位卡片自然被真实 entry 替换(同 entry_id)。
    IncomingPending {
        entry_id: String,
        from_device: String,
        /// envelope 中声明的 blob 总字节数。多个 blob 时为合计;
        /// 没有 blob(纯文本同步)时为 `None`。
        total_bytes: Option<u64>,
    },
}

/// 文件传输子系统发给宿主的语义事件。
#[derive(Debug, Clone)]
pub enum TransferHostEvent {
    StatusChanged {
        transfer_id: String,
        entry_id: String,
        status: String,
        reason: Option<String>,
    },
    Progress {
        transfer_id: String,
        entry_id: Option<String>,
        peer_id: String,
        direction: FileTransferDirection,
        bytes_transferred: u64,
        total_bytes: Option<u64>,
    },
}

/// application 发给宿主环境的统一事件。
#[derive(Debug, Clone)]
pub enum HostEvent {
    Clipboard(ClipboardHostEvent),
    Transfer(TransferHostEvent),
}

/// 宿主事件发送失败。
#[derive(Debug, thiserror::Error)]
pub enum EmitError {
    #[error("emit failed: {0}")]
    Failed(String),
}

/// 宿主事件发送端口。
pub trait HostEventEmitterPort: Send + Sync {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError>;
}
