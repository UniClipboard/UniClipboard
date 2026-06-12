//! mobile_lan 路由共享的小型协议工具。
//!
//! 这里仅放多个路由族都会用到的常量、日志字段映射和错误到 HTTP 的转换，
//! 避免历史兼容、文件传输、SyncClipboard.json 处理器互相依赖。

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use uc_application::facade::{ApplyIncomingMobileClipError, ApplyIncomingMobileClipOutcome};

/// `PUT /SyncClipboard.json` 与 `POST /api/history` 元数据上限。
/// SyncClipboard 协议 meta payload 远小于此(典型 < 1 KB),16 MiB 是
/// axum to_bytes 的安全上限。
pub(super) const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

/// 移动端文件上传入口的兜底磁盘安全阀。
///
/// `/file/{dataName}` 和 SyncClipboard Android 的 multipart
/// `POST /api/history` 都会把字节流式写进 staging,不把整文件读入内存。
/// 这个值只是防止异常 / 恶意客户端写满磁盘的远端硬上限。
///
/// 现写死 10 GiB。
/// TODO(P6 配置化): 拆到 settings (`mobile_sync.put_file_max_bytes`),按
/// 用户机型 / 磁盘剩余可配。
pub(super) const FILE_UPLOAD_DISK_SANITY_LIMIT: usize = 10 * 1024 * 1024 * 1024;

/// 把 outcome 翻成日志用的简短串(避免日志里出现 entry id 等敏感字段)。
pub(super) fn outcome_kind(outcome: &ApplyIncomingMobileClipOutcome) -> &'static str {
    match outcome {
        ApplyIncomingMobileClipOutcome::Applied { .. } => "applied",
        ApplyIncomingMobileClipOutcome::DuplicateSkipped { .. } => "duplicate_skipped",
        ApplyIncomingMobileClipOutcome::DecodeFailed { .. } => "decode_failed",
        ApplyIncomingMobileClipOutcome::Buffered => "buffered",
    }
}

/// `apply_incoming` 的错误映射。decode 失败按 wire-protocol 契约违反翻成
/// 400,内部错误翻成 500;outcome 维度的 `DecodeFailed` 路由层不直接收
/// (use case 已经把 decode 错包成 `Ok(DecodeFailed)`),但保留映射以防协议
/// 演进引入新错误变体。
pub(super) fn map_apply_error(err: ApplyIncomingMobileClipError, route: &'static str) -> Response {
    match err {
        ApplyIncomingMobileClipError::EncodeFailed(msg) => {
            tracing::warn!(error = %msg, route, "apply_incoming: encode failed");
            (StatusCode::BAD_REQUEST, msg).into_response()
        }
        ApplyIncomingMobileClipError::Inbound(err) => {
            tracing::warn!(error = %err, route, "apply_incoming: inbound pipeline failure");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        ApplyIncomingMobileClipError::Internal(msg) => {
            tracing::warn!(error = %msg, route, "apply_incoming: internal");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
