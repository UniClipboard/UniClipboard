//! `ClipboardOutboundFanOutAdapter` —— [`MobileInboundFanOutPort`] 的生产
//! 实现, 把 mobile 入站完成事件接到 [`ClipboardOutboundFacade`] 的完整出
//! 站管线。
//!
//! # 设计意图
//!
//! `ApplyIncomingMobileClipUseCase` 通过 [`MobileInboundFanOutPort`] 这层
//! 薄抽象与"如何把 snapshot 分发出去"解耦, use case 自己不知道
//! `ClipboardOutboundFacade` 存在 ——
//!
//! - **测试时**:fake 实现直接 record 调用, 不必拉真实 dispatcher / blob
//!   facade / iroh adapter;
//! - **生产时**:本 adapter 承担"接到 outbound dispatcher、`tokio::spawn`
//!   fire-and-forget、失败仅 `warn!`、日志字段编排"等具体职责。
//!
//! 这样未来要再加一条 fan-out 旁路(例如 telemetry / 第三方协议)只改
//! adapter 或新增 adapter, 不动 use case 的依赖 surface。
//!
//! # 复用本机捕获出站管线
//!
//! 本 adapter 调用 [`ClipboardOutboundFacade::dispatch_capture`], 复用
//! daemon 本机剪贴板捕获完全相同的出站逻辑 ——
//!
//! - **文本 / 小图**:inline 进 V3 envelope, 经 iroh 加密通道投递;
//! - **大图**:超过 `OVERSIZED_REP_THRESHOLD_BYTES` 的 image rep 自动剥
//!   成 `representation_index = Some(i)` 的 V3BlobRef, 避免撞 iroh wire
//!   层 2 MiB payload 上限;
//! - **文件**:`text/uri-list` rep 中的 `file://` URI 被 dispatcher 抽
//!   出, 字节通过 `BlobTransferFacade::publish_blob_path` 流式 publish
//!   到 iroh-blobs, 构造 `representation_index = None` 的 free-file
//!   V3BlobRef。接收端 `InboundBlobMaterializer` 拉回字节并改写 file-list
//!   rep 成本机 URI —— "手机 → 任一桌面 → 所有桌面"的文件传输闭环靠这条
//!   路径成立。
//!
//! 同样自动受 `OutboundSyncPlanner` 控制 —— 用户在 settings 关了某个类型
//! 的同步, mobile fan-out 与本机复制 fan-out 一同被 suppress, 不出现
//! "mobile 上传可以绕过同步开关"的旁路。
//!
//! # 错误降级
//!
//! `dispatch_capture` 失败仅 `warn!`, 不抛回上层 use case —— mobile 上传
//! 是否成功只取决于"本机入站是否生效", fan-out 是事后传播, 网络出口故
//! 障不应倒灌成 HTTP 4xx/5xx 让 iPhone 端误判而触发用户重传。
//!
//! [`MobileInboundFanOutPort`]: crate::usecases::mobile_sync::apply_incoming::MobileInboundFanOutPort
//! [`ClipboardOutboundFacade`]: crate::facade::clipboard_outbound::ClipboardOutboundFacade

use std::sync::Arc;

use tracing::{info, warn};

use uc_core::ids::EntryId;
use uc_core::mobile_sync::MobileDeviceId;
use uc_core::{ClipboardChangeOrigin, SystemClipboardSnapshot};

use crate::facade::clipboard_outbound::{
    ClipboardOutboundFacade, ClipboardOutboundInput, ClipboardOutboundOutcome,
};
use crate::usecases::mobile_sync::apply_incoming::MobileInboundFanOutPort;

/// `MobileInboundFanOutPort` 的生产实现, 委托给 [`ClipboardOutboundFacade`]。
pub(crate) struct ClipboardOutboundFanOutAdapter {
    outbound: Arc<ClipboardOutboundFacade>,
}

impl ClipboardOutboundFanOutAdapter {
    pub(crate) fn new(outbound: Arc<ClipboardOutboundFacade>) -> Self {
        Self { outbound }
    }
}

impl MobileInboundFanOutPort for ClipboardOutboundFanOutAdapter {
    fn fan_out(
        &self,
        entry_id: EntryId,
        snapshot: SystemClipboardSnapshot,
        source_device_id: MobileDeviceId,
    ) {
        let outbound = Arc::clone(&self.outbound);
        let entry_id_log = entry_id.clone();
        let source_log = source_device_id.clone();
        let entry_id_str = entry_id.as_str().to_string();
        // origin = LocalCapture: 让 dispatcher 把本机视为"刚捕获了一份
        // 新内容"的设备 ——
        // - 触发 file 路径提取 + blob 发布(`RemotePush` 会被 dispatcher
        //   显式 short-circuit 成 Skipped, 不走 publish);
        // - 经由 `OutboundSyncPlanner` 与本机复制走同一条策略链路。
        tokio::spawn(async move {
            match outbound
                .dispatch_capture(ClipboardOutboundInput {
                    entry_id: entry_id_str,
                    snapshot,
                    origin: ClipboardChangeOrigin::LocalCapture,
                })
                .await
            {
                Ok(ClipboardOutboundOutcome::Dispatched {
                    accepted,
                    duplicate,
                    offline,
                    errored,
                    pending,
                    blob_ref_count,
                }) => info!(
                    entry_id = %entry_id_log,
                    source = %source_log,
                    accepted,
                    duplicate,
                    offline,
                    errored,
                    pending,
                    blob_ref_count,
                    "mobile_sync fan-out: relayed mobile-inbound snapshot to paired peers"
                ),
                Ok(ClipboardOutboundOutcome::Skipped { reason }) => info!(
                    entry_id = %entry_id_log,
                    source = %source_log,
                    reason = %reason,
                    "mobile_sync fan-out: dispatcher skipped (planner / origin guard)"
                ),
                Err(err) => warn!(
                    entry_id = %entry_id_log,
                    source = %source_log,
                    error = %err,
                    "mobile_sync fan-out: dispatch_capture failed — mobile-inbound NOT relayed to other paired devices"
                ),
            }
        });
    }
}
