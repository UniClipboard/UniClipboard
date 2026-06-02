//! HTTP route handlers for lifecycle management endpoints.
//!
//! Provides GET /lifecycle/status, POST /lifecycle/retry, and POST /lifecycle/ready.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use std::sync::atomic::Ordering;
use tracing::{info, Instrument};

use uc_daemon_contract::api::dto::envelope::{ApiEnvelope, LifecycleStatusEnvelope};

use super::types::LifecycleStatusResponse;
use crate::api::dto::error::ApiError;
use crate::api::server::DaemonApiState;

/// Build the lifecycle router for daemon HTTP API.
pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/lifecycle/status", get(get_lifecycle_status_handler))
        .route("/lifecycle/retry", post(retry_lifecycle_handler))
        .route("/lifecycle/ready", post(lifecycle_ready_handler))
}

/// 通知 daemon：GUI 已解锁，可以开始采集剪贴板。
///
/// 在 `GuiInProcess` 模式下，剪贴板采集被门控住，直到 GUI 在用户解锁
/// 应用之后显式发出"就绪"信号；本端点负责打开该门控。
#[utoipa::path(
    post,
    path = "/lifecycle/ready",
    tag = "lifecycle",
    operation_id = "signalLifecycleReady",
    responses(
        (status = 204, description = "Ready signal accepted; clipboard capture gate opened")
    )
)]
async fn lifecycle_ready_handler(State(state): State<DaemonApiState>) -> impl IntoResponse {
    if let Some(gate) = &state.clipboard_capture_gate {
        let was_closed = !gate.swap(true, Ordering::SeqCst);
        if was_closed {
            info!("Clipboard capture gate opened by GUI lifecycle/ready signal");
        } else {
            info!("Clipboard capture gate already open (duplicate lifecycle/ready call)");
        }
    }

    // Trigger deferred services start (clipboard-watcher, inbound-clipboard-sync, etc.)
    if let Some(notify) = &state.deferred_ready_notify {
        notify.notify_one();
    }

    StatusCode::NO_CONTENT.into_response()
}

/// GET /lifecycle/status
///
/// Returns the current daemon lifecycle state wrapped in the canonical
/// `{ data, ts }` envelope (ADR-008). The bare `{ state }` shape is retired.
#[utoipa::path(
    get,
    path = "/lifecycle/status",
    tag = "lifecycle",
    operation_id = "getLifecycleStatus",
    responses(
        (status = 200, description = "Current lifecycle state", body = LifecycleStatusEnvelope),
        (status = 503, description = "Daemon runtime unavailable", body = ApiErrorResponse)
    )
)]
async fn get_lifecycle_status_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<LifecycleStatusEnvelope>, ApiError> {
    let app = state.app_facade_or_error()?;
    let current_state = app.lifecycle.status().await;

    Ok(Json(ApiEnvelope::now(LifecycleStatusResponse {
        state: current_state.as_str().to_string(),
    })))
}

/// POST /lifecycle/retry
///
/// Slice4 P5c: libp2p `start_network` 已退役,iroh 路由由
/// `SpaceSetupAssembly` 启动时即装好,no longer 需要 retry 出动 network。
/// 这个 endpoint 现在只做 lifecycle 状态推进 + 触发 deferred 服务启动,
/// 等价于 GUI 端 `/lifecycle/ready` 的 idempotent 重试入口。
#[utoipa::path(
    post,
    path = "/lifecycle/retry",
    tag = "lifecycle",
    operation_id = "retryLifecycle",
    responses(
        (status = 204, description = "Retry completed; lifecycle advanced to ready"),
        (status = 500, description = "Lifecycle retry failed", body = ApiErrorResponse),
        (status = 503, description = "Daemon runtime unavailable", body = ApiErrorResponse)
    )
)]
async fn retry_lifecycle_handler(State(state): State<DaemonApiState>) -> impl IntoResponse {
    let app = match state.app_facade_or_error() {
        Ok(app) => app,
        Err(error) => return error.into_response(),
    };

    let span = tracing::info_span!("daemon.lifecycle.retry");
    async move {
        if let Err(error) = app.lifecycle.retry_to_ready().await {
            return ApiError::internal(format!("lifecycle retry failed: {error}")).into_response();
        }

        // Signal clipboard capture gate (if present) — same as /lifecycle/ready.
        if let Some(gate) = &state.clipboard_capture_gate {
            let was_closed = !gate.swap(true, std::sync::atomic::Ordering::SeqCst);
            if was_closed {
                info!("Clipboard capture gate opened by lifecycle retry");
            }
        }

        // Trigger deferred services start.
        if let Some(notify) = &state.deferred_ready_notify {
            notify.notify_one();
        }

        info!("Lifecycle retry completed successfully");
        StatusCode::NO_CONTENT.into_response()
    }
    .instrument(span)
    .await
}
