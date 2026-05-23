//! 文件传输用户操作命令。
//!
//! 目前只承载一个动作:接收方点"取消"按钮主动撤回一次进行中的入站
//! 大文件传输。
//!
//! 命令走 in-process facade(`runtime.app_facade()`),不经 HTTP/webserver。
//! 取消的三件套行为(让 fetch 路径退出 / 撕 QUIC connection / 落 Cancelled
//! 事件)统一在 `BlobTransferFacade::cancel_inbound_transfer` 收口,
//! 这里只做 Tauri 入参解析 + 调 facade。
//!
//! 与"删除 in-flight transfer"的关系:删除流程(P1 后续阶段)需要先
//! 调本命令撤回 fetch,再调 cleanup 删本地文件 —— 否则 cleanup 撞上
//! 正在被 iroh-blobs 句柄持有的文件会卡住。

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{info_span, Instrument};

use uc_application::facade::BlobTransferError;
use uc_core::FileTransferCancellationReason;
use uc_platform::ports::observability::TraceMetadata;

use crate::bootstrap::TauriAppRuntime;
use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;

/// `BlobTransferError` 翻译到 IPC 边界错误。
///
/// cancel_inbound_transfer 的 happy path 是 `Ok(())`,fail path 仅有
/// 一种:facade 装配缺失(`BlobTransferError::Fetch("blob facade unavailable")`),
/// 这是装配缺陷而不是业务失败,统一映射到 `InternalError`。
/// `Publish` / `Cancelled` 变体在本命令的路径上不应出现,但映射到
/// `InternalError` 保持总匹配。
impl From<BlobTransferError> for CommandError {
    fn from(err: BlobTransferError) -> Self {
        CommandError::InternalError(err.to_string())
    }
}

/// 取消原因的前端可序列化镜像。与领域 `FileTransferCancellationReason`
/// 一一对应,但用 camelCase tag 让前端 union 写起来自然。
///
/// 当前 P1 只暴露 `LocalUser` —— 用户主动点取消按钮触发本命令。
/// 其它变体(`RemotePeer` / `Replaced` / `Unknown`)是 domain 内部状态
/// 转移,不通过本 IPC 入口。
#[derive(Debug, Deserialize, Serialize, specta::Type)]
#[serde(tag = "tag", rename_all = "camelCase")]
pub enum CancelTransferReasonDto {
    LocalUser,
}

impl From<CancelTransferReasonDto> for FileTransferCancellationReason {
    fn from(value: CancelTransferReasonDto) -> Self {
        match value {
            CancelTransferReasonDto::LocalUser => FileTransferCancellationReason::LocalUser,
        }
    }
}

/// 取消一次进行中的入站文件传输。
///
/// 幂等:`transfer_id` 不在 inflight registry 时(没有进行中的 fetch /
/// 已经被取消过)正常返回。本命令完成后前端会通过 `file-transfer.status_changed`
/// host event 收到 cancelled 状态。
#[tauri::command]
#[specta::specta]
pub async fn cancel_file_transfer(
    runtime: tauri::State<'_, Arc<TauriAppRuntime>>,
    transfer_id: String,
    reason: CancelTransferReasonDto,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.file_transfer.cancel",
        transfer_id = %transfer_id,
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    let runtime = runtime.inner().clone();
    async move {
        // 取消结果(Cancelled / NotInflight)在 IPC 边界上不暴露 —— 前端
        // 只关心"我点了取消,后续等 status_changed 通知";`NotInflight`
        // 表示后端已经没活 fetch (已完成/已取消/还没启动),对前端等价
        // 于"操作幂等成功"。
        let _ = runtime
            .app_facade()
            .cancel_inbound_transfer(&transfer_id, reason.into())
            .await?;
        Ok(())
    }
    .instrument(span)
    .await
}
