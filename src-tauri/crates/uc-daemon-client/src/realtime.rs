use async_trait::async_trait;
use tokio::sync::mpsc;
use uc_core::file_transfer::FileTransferDirection;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RealtimeTopic {
    Pairing,
    Peers,
    PairedDevices,
    Setup,
    Clipboard,
    FileTransfer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingUpdatedEvent {
    pub session_id: String,
    pub status: String,
    pub peer_id: Option<String>,
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingVerificationRequiredEvent {
    pub session_id: String,
    pub peer_id: Option<String>,
    pub device_name: Option<String>,
    pub code: Option<String>,
    pub local_fingerprint: Option<String>,
    pub peer_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingFailedEvent {
    pub session_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCompleteEvent {
    pub session_id: String,
    pub peer_id: Option<String>,
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimePeerSummary {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerChangedEvent {
    pub peers: Vec<RealtimePeerSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerNameUpdatedEvent {
    pub peer_id: String,
    pub device_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerConnectionChangedEvent {
    pub peer_id: String,
    pub connected: bool,
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealtimeSpaceMemberSummary {
    pub device_id: String,
    pub device_name: String,
    pub last_seen_ts: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceMembersChangedEvent {
    pub devices: Vec<RealtimeSpaceMemberSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardNewContentEvent {
    pub entry_id: String,
    pub preview: String,
    pub origin: String, // "local" or "remote"
}

/// 接收端确认一个 inbound clipboard 即将到达(V3 envelope 已解码,blob 拉取
/// 尚未完成)。携带最终 entry_id —— 订阅方据此插入占位卡片。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardIncomingPendingEvent {
    pub entry_id: String,
    pub from_device: String,
    pub total_bytes: Option<u64>,
    pub filenames: Vec<String>,
}

/// 文件传输状态变化(running / completed / failed 等)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTransferStatusChangedEvent {
    pub transfer_id: String,
    pub entry_id: String,
    pub status: String,
    pub reason: Option<String>,
}

/// 文件传输 byte-level 进度快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTransferProgressEvent {
    pub transfer_id: String,
    pub entry_id: Option<String>,
    pub peer_id: String,
    pub direction: FileTransferDirection,
    pub bytes_transferred: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RealtimeEvent {
    PairingUpdated(PairingUpdatedEvent),
    PairingVerificationRequired(PairingVerificationRequiredEvent),
    PairingFailed(PairingFailedEvent),
    PairingComplete(PairingCompleteEvent),
    PeersChanged(PeerChangedEvent),
    PeersNameUpdated(PeerNameUpdatedEvent),
    PeersConnectionChanged(PeerConnectionChangedEvent),
    SpaceMembersChanged(SpaceMembersChangedEvent),
    ClipboardNewContent(ClipboardNewContentEvent),
    ClipboardIncomingPending(ClipboardIncomingPendingEvent),
    FileTransferStatusChanged(FileTransferStatusChangedEvent),
    FileTransferProgress(FileTransferProgressEvent),
}

#[async_trait]
pub trait RealtimeTopicPort: Send + Sync {
    async fn subscribe(
        &self,
        consumer: &'static str,
        topics: &[RealtimeTopic],
    ) -> anyhow::Result<mpsc::Receiver<RealtimeEvent>>;
}
