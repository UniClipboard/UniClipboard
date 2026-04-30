//! 把入站 fetch 的字节级进度回报给数据来源端(sender)的端口。
//!
//! ## 业务动机
//!
//! iroh blob 是 pull-based:sender 调用 `publish` 只是把 ciphertext 放进
//! 本地 share-store 并签发 ticket,真正的字节传输发生在 receiver 用
//! ticket 拉取时。因此 sender 自身没有"被拉取了多少字节"的事件可观测,
//! 也就无法在自己 UI 上展示对端的真实接收进度。
//!
//! 本端口表达的业务能力是:**接收端把自己看到的 fetch 字节进度,沿
//! 反向 P2P 通道回报给数据来源端**。这是字节级进度展示在发送方落地
//! 的唯一可观测来源。
//!
//! 端口本身不绑定具体协议(适配器可以用任何 wire 形态实现),uc-core
//! 也不关心反向通道是 unicast / 长连接 / per-transfer 短连接,这些是
//! 适配器关注点。
//!
//! ## 与 [`super::event::FileTransferEvent`] 的边界
//!
//! `FileTransferEvent` 是**领域事件**(发生过的事实),适配器和应用
//! 流程都可以读;本端口是**外部能力**(把进度送到对端设备的动作),
//! 只在接收端 fetch 主路径上被旁路调用,不进入领域事件 timeline。

use async_trait::async_trait;

use crate::ids::DeviceId;

/// 一帧出站进度上报的语义状态。
///
/// 适配器端会把这个值映射到 wire 状态字节;应用层只关心三种粗粒度
/// 业务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundProgressStatus {
    /// 进行中:`bytes_transferred` 是当前累计已收字节,`total_bytes`
    /// 在已知时透传。
    InProgress,
    /// 接收端已经把整个 blob 拉完,`bytes_transferred` 等于最终大小。
    Completed,
    /// 接收端 fetch 失败,sender 端可据此把 transferring 状态切为 failed。
    Failed,
}

/// 接收端把字节级 fetch 进度回报给数据来源端(sender)。
///
/// 实现端通常对应一个 P2P adapter,负责打开/复用反向通道并写一帧。
/// 节流由调用方负责(`BlobProgressSink::report` 已经按字节阈值 + 时间
/// 窗节流过),实现端不应自行做长时间阻塞操作。
///
/// `transfer_id` 是协议层关联键。当前协议约定接收端复用 V3 envelope
/// 里 `blob_refs[i].entry_id`(发送端 `EntryId` 的字符串形式),sender
/// 端收到帧后用它索引本地 entry,从而把进度落到 sender UI 对应的那
/// 行剪贴板上。
///
/// 失败行为:实现端应记录日志后吞掉错误,**不应**让 fetch 主流程感
/// 知 ——"无法把进度通知 sender"对接收端业务并不致命,接收端的本地
/// fetch 仍然要继续。
#[async_trait]
pub trait OutboundProgressReporterPort: Send + Sync {
    async fn report(
        &self,
        target: &DeviceId,
        transfer_id: &str,
        bytes_transferred: u64,
        total_bytes: Option<u64>,
        status: OutboundProgressStatus,
    );
}
