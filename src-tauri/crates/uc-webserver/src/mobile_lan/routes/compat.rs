//! SyncClipboard 客户端连接探测与目录兼容入口。
//!
//! 这些入口不改动剪贴板内容，只用于让官方桌面端 / Android 端确认服务在线、
//! 版本可读、`/file` 目录存在。

use axum::{http::StatusCode, Json};
use chrono::Utc;

pub(super) async fn get_api_time() -> Json<String> {
    Json(Utc::now().to_rfc3339())
}

pub(super) async fn get_api_version() -> Json<String> {
    Json(format!("UniClipboard {}", env!("CARGO_PKG_VERSION")))
}

pub(super) async fn root_compat() -> &'static str {
    "Server is running."
}

/// 兼容官方服务端的 `/file` 目录操作。
///
/// 实际文件内容由 `/file/{dataName}` 和移动同步 staging 管线管理；这个目录
/// 入口只接住客户端的目录探测 / 清理请求，不代表执行了真实目录删除。
pub(super) async fn file_folder_compat() -> StatusCode {
    StatusCode::OK
}
