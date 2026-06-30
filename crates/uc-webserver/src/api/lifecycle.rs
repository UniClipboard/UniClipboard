//! HTTP route handlers for lifecycle management endpoints.
//!
//! Provides GET /lifecycle/status, POST /lifecycle/retry, POST /lifecycle/ready,
//! and POST /lifecycle/restart (ADR-008 P5-L L8c controlled restart).

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;
use std::sync::atomic::Ordering;
use tracing::{info, Instrument};

use uc_daemon_contract::api::dto::envelope::{ApiEnvelope, LifecycleStatusEnvelope};
use uc_daemon_contract::api::types::{DaemonResidency, RestartAccepted, RestartRequest};

use super::types::LifecycleStatusResponse;
use crate::api::dto::error::ApiError;
use crate::api::restart::{RestartCoordinator, RestartOutcome};
use crate::api::server::DaemonApiState;

/// Build the lifecycle router for daemon HTTP API.
pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/lifecycle/status", get(get_lifecycle_status_handler))
        .route("/lifecycle/retry", post(retry_lifecycle_handler))
        .route("/lifecycle/ready", post(lifecycle_ready_handler))
        // ADR-008 P5-L L8d-1: controlled restart, surfaced as a typed client
        // contract (OpenAPI + generated TS SDK + native uc-daemon-client method).
        .route("/lifecycle/restart", post(restart_handler))
}

/// 通知 daemon：已解锁，可以开始采集剪贴板——打开 clipboard capture 门控。
///
/// 锁定期 daemon 把剪贴板采集门控住(deferred services);解锁后用本端点
/// 放行。ADR-008 P3-3 起 daemon 永远是独立进程,GUI 作为纯客户端经 loopback
/// HTTP 调它(旧 `GuiInProcess` 同进程模式已删除)。
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
/// `SyncEngineAssembly` 启动时即装好,no longer 需要 retry 出动 network。
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

/// Pure arbitration decision for a controlled-restart request (ADR-008 P5-L L8c).
///
/// HTTP-agnostic so it can be unit-tested without composing a `DaemonApiState`.
/// The handler maps each variant onto a status code; see
/// [`evaluate_restart_request`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartDecision {
    /// Request accepted — `generation` stamped, `target` locked in.
    Accepted {
        generation: u64,
        target: DaemonResidency,
    },
    /// A restart is already in progress (carries the locked-in target + gen).
    Conflict {
        current_target: DaemonResidency,
        generation: u64,
    },
    /// This daemon is not an Oneshot, so there is nothing to promote.
    NotPromotable,
    /// Controlled restart is unavailable because the single-instance lock is off.
    Disabled,
    /// The requested target is itself Oneshot — never a valid promotion target.
    InvalidTarget,
}

/// Decide a controlled-restart request (ADR-008 P5-L L8c).
///
/// Refusal checks (`Disabled` / `NotPromotable` / `InvalidTarget`) run BEFORE
/// [`RestartCoordinator::request`], so a refused request NEVER raises `quiescing`.
/// Only a request that clears all three guards is handed to the coordinator,
/// where first-wins arbitration may still return `Conflict`.
fn evaluate_restart_request(
    residency: DaemonResidency,
    single_instance_disabled: bool,
    target: DaemonResidency,
    coordinator: &RestartCoordinator,
) -> RestartDecision {
    if single_instance_disabled {
        return RestartDecision::Disabled;
    }
    if residency != DaemonResidency::Oneshot {
        return RestartDecision::NotPromotable;
    }
    if target == DaemonResidency::Oneshot {
        return RestartDecision::InvalidTarget;
    }
    match coordinator.request(target) {
        RestartOutcome::Accepted { generation } => RestartDecision::Accepted { generation, target },
        RestartOutcome::Conflict {
            current_target,
            generation,
        } => RestartDecision::Conflict {
            current_target,
            generation,
        },
    }
}

/// POST /lifecycle/restart — request a controlled restart/promotion of a
/// transient (Oneshot) daemon (ADR-008 P5-L).
///
/// REFUSES unless this daemon is an Oneshot residency AND the single-instance
/// lock is enabled AND the target is not itself Oneshot. The accepted path raises
/// the L8b `quiescing` flag (via the coordinator) so admission gates drain
/// in-flight work; the Oneshot supervisor then self-terminates and `app.rs`
/// persists the handover record. Production-neutral: no Oneshot daemon exists
/// until L8d, so the accept path is unreachable in production.
#[utoipa::path(
    post,
    path = "/lifecycle/restart",
    tag = "lifecycle",
    operation_id = "requestLifecycleRestart",
    request_body = RestartRequest,
    responses(
        (status = 202, description = "Controlled restart accepted; quiescing/drain started", body = RestartAcceptedEnvelope),
        (status = 400, description = "Invalid target mode (cannot promote to a transient target)", body = ApiErrorResponse),
        (status = 409, description = "Restart unavailable (already in progress / not a transient daemon / single-instance disabled)", body = ApiErrorResponse),
    )
)]
async fn restart_handler(
    State(state): State<DaemonApiState>,
    body: Result<Json<RestartRequest>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    let Json(request) = match body {
        Ok(json) => json,
        Err(rejection) => {
            return ApiError::bad_request(format!("invalid restart request body: {rejection}"))
                .into_response();
        }
    };

    let single_instance_disabled = uc_daemon_local::instance_lock::single_instance_disabled();
    let decision = evaluate_restart_request(
        state.residency,
        single_instance_disabled,
        request.target_mode,
        &state.restart,
    );

    match decision {
        RestartDecision::Accepted { generation, target } => {
            info!(
                generation,
                target_mode = ?target,
                "controlled restart accepted — quiescing raised"
            );
            (
                StatusCode::ACCEPTED,
                Json(ApiEnvelope::now(RestartAccepted {
                    generation,
                    target_mode: target,
                })),
            )
                .into_response()
        }
        RestartDecision::Conflict {
            current_target,
            generation,
        } => ApiError::conflict("controlled restart already in progress")
            .with_code("restart_in_progress")
            .with_details(json!({
                "currentTargetMode": current_target,
                "generation": generation,
            }))
            .into_response(),
        RestartDecision::NotPromotable => {
            ApiError::conflict("daemon is not a transient (oneshot) daemon; nothing to promote")
                .with_code("not_promotable")
                .into_response()
        }
        RestartDecision::Disabled => {
            ApiError::conflict("controlled restart unavailable: single-instance lock is disabled")
                .with_code("restart_disabled")
                .into_response()
        }
        RestartDecision::InvalidTarget => {
            ApiError::bad_request("cannot promote to a transient (oneshot) target")
                .with_code("invalid_target")
                .into_response()
        }
    }
}

#[cfg(test)]
mod restart_tests {
    use super::*;

    /// ADR-008 P5-L L8c: when the single-instance lock is disabled the request
    /// is refused with `Disabled` and quiescing is NEVER raised.
    #[test]
    fn disabled_single_instance_refuses_without_quiescing() {
        let coord = RestartCoordinator::default();
        let decision = evaluate_restart_request(
            DaemonResidency::Oneshot,
            true, // single_instance_disabled
            DaemonResidency::Standalone,
            &coord,
        );
        assert_eq!(decision, RestartDecision::Disabled);
        assert!(
            !coord.is_quiescing(),
            "a refused request must not raise quiescing"
        );
    }

    /// ADR-008 P5-L L8c: a non-Oneshot daemon has nothing to promote — refused
    /// with `NotPromotable`, quiescing untouched.
    #[test]
    fn non_oneshot_residency_is_not_promotable_without_quiescing() {
        for residency in [DaemonResidency::Standalone, DaemonResidency::ServerHeadless] {
            let coord = RestartCoordinator::default();
            let decision =
                evaluate_restart_request(residency, false, DaemonResidency::Standalone, &coord);
            assert_eq!(decision, RestartDecision::NotPromotable);
            assert!(
                !coord.is_quiescing(),
                "a NotPromotable refusal must not raise quiescing"
            );
        }
    }

    /// ADR-008 P5-L L8c: promoting TO an Oneshot target is invalid — refused with
    /// `InvalidTarget`, quiescing untouched.
    #[test]
    fn oneshot_target_is_invalid_without_quiescing() {
        let coord = RestartCoordinator::default();
        let decision = evaluate_restart_request(
            DaemonResidency::Oneshot,
            false,
            DaemonResidency::Oneshot,
            &coord,
        );
        assert_eq!(decision, RestartDecision::InvalidTarget);
        assert!(
            !coord.is_quiescing(),
            "an InvalidTarget refusal must not raise quiescing"
        );
    }

    /// ADR-008 P5-L L8c: an Oneshot daemon promoting to a valid target is
    /// accepted, raising quiescing and stamping generation 1.
    #[test]
    fn oneshot_to_standalone_is_accepted() {
        let coord = RestartCoordinator::default();
        let decision = evaluate_restart_request(
            DaemonResidency::Oneshot,
            false,
            DaemonResidency::Standalone,
            &coord,
        );
        assert_eq!(
            decision,
            RestartDecision::Accepted {
                generation: 1,
                target: DaemonResidency::Standalone,
            }
        );
        assert!(coord.is_quiescing());
    }

    /// ADR-008 P5-L L8c: a second accepted-path request loses to the first via
    /// the coordinator's first-wins arbitration; after an abort a fresh request
    /// is accepted with a monotonically-bumped generation.
    #[test]
    fn second_request_conflicts_then_abort_allows_next() {
        let coord = RestartCoordinator::default();
        assert_eq!(
            evaluate_restart_request(
                DaemonResidency::Oneshot,
                false,
                DaemonResidency::Standalone,
                &coord,
            ),
            RestartDecision::Accepted {
                generation: 1,
                target: DaemonResidency::Standalone,
            }
        );

        // Second request — even past the guards — conflicts with the locked-in one.
        assert_eq!(
            evaluate_restart_request(
                DaemonResidency::Oneshot,
                false,
                DaemonResidency::ServerHeadless,
                &coord,
            ),
            RestartDecision::Conflict {
                current_target: DaemonResidency::Standalone,
                generation: 1,
            }
        );

        coord.abort();
        // After abort, a fresh request is accepted with generation 2 (monotonic).
        assert_eq!(
            evaluate_restart_request(
                DaemonResidency::Oneshot,
                false,
                DaemonResidency::ServerHeadless,
                &coord,
            ),
            RestartDecision::Accepted {
                generation: 2,
                target: DaemonResidency::ServerHeadless,
            }
        );
    }
}
