//! SyncClipboard 协议根路径路由(P5a.6 起接真实管线)。
//!
//! 替代 Phase 3 子步骤 3 的 `GET /mobile/v1/handshake` stub —— v3 切到
//! SyncClipboard 兼容路径后,客户端会在用户填的 base URL 后面直接拼官方
//! 路径。daemon 必须把这些路由挂在根路径,否则路径前缀对不上。
//!
//! ## 当前能力边界
//!
//! | 路由组 | 当前状态 |
//! |---|---|
//! | `GET/PUT /SyncClipboard.json` | 真实接入当前剪贴板读写 |
//! | `GET/PUT/HEAD /file/{dataName}` | 真实接入文件字节读写与 staging |
//! | `GET /api/time`, `GET /api/version`, `GET /version`, `/` | 真实返回探测信息 |
//! | `GET /api/history/{profileId}`, `GET /api/history/{profileId}/data`, `POST /api/history` | 桥接到当前最新剪贴板与移动同步入站流程 |
//! | `POST /api/history/query`, `GET /api/history/statistics` | 兼容壳,只暴露当前最新一条或空结果,不是完整历史分页 / 统计 |
//! | `PATCH /api/history/{type}/{hash}`, `DELETE /api/history/clear`, `DELETE /file` | 兼容壳,接住客户端请求,暂不持久化标星 / 置顶 / 删除状态,也不执行真实清空 |
//!
//! 这份表是协议承诺边界:修复 Android 客户端上传前的 404 不等于已经实现
//! SyncClipboard 官方服务端的完整历史系统。

use std::sync::Arc;

use axum::{
    extract::{DefaultBodyLimit, FromRef},
    routing::{any, delete, get, post},
    Router,
};

use uc_application::facade::{FileTransferFacade, MobileSyncFacade};

use crate::mobile_lan::middleware::basic_auth;

mod common;
mod compat;
mod file;
mod history;
mod sync_doc;

#[cfg(test)]
mod tests;

/// mobile_lan 路由共享 state。`mobile_sync` 是 PUT/GET 业务入口;
/// `file_transfer` 是 PUT /file 流式接收过程中 handler 用来发
/// `seed_receiver_context` / `start` / `report_progress` / `fail` 的可选
/// lifecycle facade —— CLI / fallback 装配可留 `None`,handler 自动降级
/// 为静默(buffered 阶段不发 lifecycle 事件,SyncDoc 阶段一样不会 link)。
#[derive(Clone)]
pub(crate) struct MobileLanState {
    pub mobile_sync: Arc<MobileSyncFacade>,
    pub file_transfer: Option<Arc<FileTransferFacade>>,
}

impl FromRef<MobileLanState> for Arc<MobileSyncFacade> {
    fn from_ref(state: &MobileLanState) -> Self {
        state.mobile_sync.clone()
    }
}

impl FromRef<MobileLanState> for Option<Arc<FileTransferFacade>> {
    fn from_ref(state: &MobileLanState) -> Self {
        state.file_transfer.clone()
    }
}

/// 构造根路径 SyncClipboard 协议路由。daemon listener 把它挂到 axum app 根。
///
/// 所有路由都接 Basic Auth middleware, 未登记 / 未带头 / 凭据错的请求拿
/// 401 + `WWW-Authenticate: Basic` 头。
///
/// `file_transfer` 是可选的:daemon 入口装配时透传 `app_facade.file_transfer`,
/// PUT /file handler 用它在收 body 的过程中发 lifecycle 事件
/// (seed / start / progress / fail);测试 / CLI fallback 可留 `None`,
/// handler 自动降级为静默(buffered 仍然写 IncomingMobileBuffer,但
/// `file_transfer` 表里不会有这条 transfer 的 lifecycle 行)。
pub(crate) fn build_router(
    facade: Arc<MobileSyncFacade>,
    file_transfer: Option<Arc<FileTransferFacade>>,
) -> Router {
    let state = MobileLanState {
        mobile_sync: facade.clone(),
        file_transfer,
    };
    Router::new()
        .route("/", any(compat::root_compat))
        .route("/api/time", get(compat::get_api_time))
        .route("/api/version", get(compat::get_api_version))
        .route("/version", get(compat::get_api_version))
        .route(
            "/api/history",
            post(history::post_history_record)
                .layer(DefaultBodyLimit::max(common::FILE_UPLOAD_DISK_SANITY_LIMIT)),
        )
        .route("/api/history/query", post(history::query_history_records))
        .route(
            "/api/history/statistics",
            get(history::get_history_statistics),
        )
        .route("/api/history/clear", delete(history::clear_history_compat))
        .route(
            "/api/history/:profile_id/data",
            get(history::get_history_data),
        )
        .route("/api/history/:profile_id", get(history::get_history_record))
        .route(
            "/api/history/:item_type/:hash",
            axum::routing::patch(history::patch_history_record),
        )
        .route(
            "/SyncClipboard.json",
            get(sync_doc::get_sync_clipboard_json).put(sync_doc::put_sync_clipboard_json),
        )
        .route("/file", any(compat::file_folder_compat))
        .route(
            "/file/:data_name",
            get(file::get_clipboard_file).put(file::put_clipboard_file),
        )
        .layer(axum::middleware::from_fn_with_state(facade, basic_auth))
        .with_state(state)
}
