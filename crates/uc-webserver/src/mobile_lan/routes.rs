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
//! | `GET /api/mobile-sync/content-availability` | **不属于 SyncClipboard 协议**,只给本项目自己的移动客户端用:按 `snapshotHash`(`blake3v1:<hex>`)做可靠的存在性 + 可用性探测,不受 `/api/history/*` 的 hash 漂移容忍影响 |
//! | `GET /healthz` | **不属于 SyncClipboard 协议**,也是唯一的公开路由:掉线的移动客户端用它常数成本地探测 listener 是否恢复。第三方 / 旧版 SyncClipboard 服务端没有这个端点,客户端据 404 判定"不支持"并退回低频兼容探测 —— 所以它是加法契约,不改变现有 wire schema |
//!
//! 这份表是协议承诺边界:修复 Android 客户端上传前的 404 不等于已经实现
//! SyncClipboard 官方服务端的完整历史系统。`content-availability` 与
//! `healthz` 路由是本项目自有能力的扩展点,不是这份协议边界的一部分,未来
//! 演进不受"不能破坏 SyncClipboard 兼容性"的约束。

use std::sync::Arc;

use axum::{
    extract::{DefaultBodyLimit, FromRef},
    routing::{any, delete, get, post},
    Router,
};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use uc_application::facade::{FileTransferFacade, MobileSyncFacade};
use uc_core::clipboard::ActiveClipboardState;

use crate::mobile_lan::middleware::basic_auth;
use crate::mobile_lan::sse_registry::SseConnectionRegistry;

mod common;
mod compat;
mod content_availability;
mod file;
mod health;
mod history;
mod sse;
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
    /// Fan-out source for active-clipboard register advances; the SSE
    /// handler subscribes once per connection to receive `update` events.
    pub sse_source: broadcast::Sender<ActiveClipboardState>,
    /// Listener-wide cancellation signal. The SSE handler must select on
    /// this directly (not just rely on axum's connection drain) — an SSE
    /// stream never ends on its own, so without an active cancel branch
    /// `with_graceful_shutdown` would hang waiting for it.
    pub cancel: CancellationToken,
    /// Per-device SSE connection cap + eviction; lives for the listener's
    /// lifetime like `sse_source` / `cancel` (fresh instance per
    /// `build_router` call — a listener restart naturally resets it).
    pub sse_registry: Arc<SseConnectionRegistry>,
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
/// Router 分两层,merge 后交给 listener:
///
/// ```text
/// public router
/// └── GET /healthz          ← 无鉴权、常数成本的 liveness
///
/// protected router
/// ├── /SyncClipboard.json
/// ├── /file/*
/// ├── /api/history/*
/// ├── /api/sse/clipboard
/// └── Basic Auth middleware
/// ```
///
/// 这个分层就是"哪些路由公开"的权威来源。`/healthz` 的公开性由它挂在
/// public router 这一事实表达,**不是**靠在 Basic Auth middleware 里加路径
/// 特判 —— 后者会让公开/受保护的边界散落进中间件的控制流,既读不出来也
/// 挡不住回归。public router 的 state 只有 `CancellationToken`,连 facade
/// 都不在作用域内,健康探针的常数成本因此由类型系统保证。
///
/// protected 路由都接 Basic Auth middleware, 未登记 / 未带头 / 凭据错的
/// 请求拿 401 + `WWW-Authenticate: Basic` 头。
///
/// `file_transfer` 是可选的:daemon 入口装配时透传 `app_facade.file_transfer`,
/// PUT /file handler 用它在收 body 的过程中发 lifecycle 事件
/// (seed / start / progress / fail);测试 / CLI fallback 可留 `None`,
/// handler 自动降级为静默(buffered 仍然写 IncomingMobileBuffer,但
/// `file_transfer` 表里不会有这条 transfer 的 lifecycle 行)。
pub(crate) fn build_router(
    facade: Arc<MobileSyncFacade>,
    file_transfer: Option<Arc<FileTransferFacade>>,
    sse_source: broadcast::Sender<ActiveClipboardState>,
    cancel: CancellationToken,
) -> Router {
    build_public_router(cancel.clone()).merge(build_protected_router(
        facade,
        file_transfer,
        sse_source,
        cancel,
    ))
}

/// 公开路由:不经过 Basic Auth,不接触任何业务 port。
///
/// state 刻意只放 `CancellationToken` —— listener 为 `with_graceful_shutdown`
/// 本来就持有它,`/healthz` 用它区分"服务中"与"正在 drain",不新增任何状态。
fn build_public_router(cancel: CancellationToken) -> Router {
    Router::new()
        .route("/healthz", get(health::get_healthz))
        .with_state(cancel)
}

/// 受保护路由:SyncClipboard 协议面 + 本项目自有扩展,全部经 Basic Auth。
fn build_protected_router(
    facade: Arc<MobileSyncFacade>,
    file_transfer: Option<Arc<FileTransferFacade>>,
    sse_source: broadcast::Sender<ActiveClipboardState>,
    cancel: CancellationToken,
) -> Router {
    let state = MobileLanState {
        mobile_sync: facade.clone(),
        file_transfer,
        sse_source,
        cancel,
        sse_registry: Arc::new(SseConnectionRegistry::new()),
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
        .route("/api/sse/clipboard", get(sse::get_sse_clipboard))
        .route(
            "/api/mobile-sync/content-availability",
            get(content_availability::get_content_availability),
        )
        .layer(axum::middleware::from_fn_with_state(facade, basic_auth))
        .with_state(state)
}
