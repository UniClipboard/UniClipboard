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
use tracing::{info, Instrument};
use uc_engine::{
    EngineState, MembershipReadinessStateSummary, Operation, OperationResult, RecoverSessionInput,
};

use uc_daemon_contract::api::dto::envelope::{ApiEnvelope, LifecycleStatusEnvelope};
use uc_daemon_contract::api::types::{
    DaemonResidency, LifecyclePendingReason, RestartAccepted, RestartRequest,
};
use uc_daemon_contract::constants::http_route;

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
        // Caller-requested orderly shutdown of THIS daemon process, any
        // residency — unlike `/lifecycle/restart` this is not an Oneshot
        // promotion and does not touch the `RestartCoordinator`.
        .route(
            http_route::LIFECYCLE_GRACEFUL_STOP,
            post(graceful_stop_handler),
        )
}

/// 通知 daemon：客户端已经观察到核心完成解锁和接收恢复。
///
/// 创建、加入和解锁操作已经由 `Engine` 在返回成功前完成接收恢复。本端点
/// 只保留客户端幂等确认，不再持有或重复调用内部接收接口。
#[utoipa::path(
    post,
    path = "/lifecycle/ready",
    tag = "lifecycle",
    operation_id = "signalLifecycleReady",
    responses(
        (status = 204, description = "Ready signal acknowledged")
    )
)]
async fn lifecycle_ready_handler(State(_state): State<DaemonApiState>) -> impl IntoResponse {
    info!("Lifecycle ready signal accepted by the running engine");
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
    let response = match state.engine.lifecycle_state().await {
        EngineState::Running => match state.execute(Operation::QueryMembershipReadiness).await {
            Ok(OperationResult::MembershipReadiness(readiness)) => match readiness.state {
                MembershipReadinessStateSummary::Ready => {
                    let roster_readable = match state.execute(Operation::QueryPeerConnections).await
                    {
                        Ok(OperationResult::PeerConnections(_)) => true,
                        Ok(_) => {
                            return Err(ApiError::internal(
                                "engine returned an unexpected peer connections result",
                            ));
                        }
                        Err(error) => {
                            tracing::warn!(
                                code = error.code(),
                                category = %error.category(),
                                "membership roster is not ready"
                            );
                            false
                        }
                    };
                    running_lifecycle_response(readiness.state, roster_readable)
                }
                _ => running_lifecycle_response(readiness.state, false),
            },
            Ok(_) => {
                return Err(ApiError::internal(
                    "engine returned an unexpected membership readiness result",
                ));
            }
            Err(error) => {
                tracing::error!(
                    code = error.code(),
                    category = %error.category(),
                    "membership readiness query failed"
                );
                return Err(ApiError::service_unavailable(
                    "membership readiness is unavailable",
                ));
            }
        },
        EngineState::Quiescing | EngineState::Quiesced | EngineState::Suspended => {
            LifecycleStatusResponse {
                state: "Pending".to_owned(),
                pending_reason: None,
            }
        }
        EngineState::ShuttingDown | EngineState::Stopped => LifecycleStatusResponse {
            state: "Idle".to_owned(),
            pending_reason: None,
        },
    };

    Ok(Json(ApiEnvelope::now(response)))
}

fn running_lifecycle_response(
    state: MembershipReadinessStateSummary,
    roster_readable: bool,
) -> LifecycleStatusResponse {
    let (state, pending_reason) = match state {
        MembershipReadinessStateSummary::Ready if roster_readable => ("Ready", None),
        MembershipReadinessStateSummary::Ready => {
            ("Pending", Some(LifecyclePendingReason::MembershipRecovery))
        }
        MembershipReadinessStateSummary::Locked => {
            ("Pending", Some(LifecyclePendingReason::SpaceLocked))
        }
        MembershipReadinessStateSummary::Recovering => {
            ("Pending", Some(LifecyclePendingReason::MembershipRecovery))
        }
    };
    LifecycleStatusResponse {
        state: state.to_owned(),
        pending_reason,
    }
}

#[cfg(test)]
mod readiness_tests {
    use super::*;

    #[test]
    fn membership_recovery_keeps_the_product_pending() {
        let response =
            running_lifecycle_response(MembershipReadinessStateSummary::Recovering, false);

        assert_eq!(response.state, "Pending");
        assert_eq!(
            response.pending_reason,
            Some(LifecyclePendingReason::MembershipRecovery)
        );
    }

    #[test]
    fn verified_membership_admits_the_product_ui() {
        let response = running_lifecycle_response(MembershipReadinessStateSummary::Ready, true);

        assert_eq!(response.state, "Ready");
        assert_eq!(response.pending_reason, None);
    }

    #[test]
    fn unreadable_roster_keeps_verified_membership_pending() {
        let response = running_lifecycle_response(MembershipReadinessStateSummary::Ready, false);

        assert_eq!(response.state, "Pending");
        assert_eq!(
            response.pending_reason,
            Some(LifecyclePendingReason::MembershipRecovery)
        );
    }
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
    let span = tracing::info_span!("daemon.lifecycle.retry");
    async move {
        match state
            .execute(Operation::RecoverSession(RecoverSessionInput {
                allow_secure_storage_unlock: true,
            }))
            .await
        {
            Ok(OperationResult::SessionRecovered { .. }) => {}
            Ok(_) => {
                return ApiError::internal("engine returned an unexpected lifecycle-retry result")
                    .into_response();
            }
            Err(error) => {
                tracing::error!(
                    code = error.code(),
                    category = %error.category(),
                    "lifecycle retry failed"
                );
                return ApiError::internal("lifecycle retry failed").into_response();
            }
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

/// POST /lifecycle/graceful-stop — request an orderly shutdown of THIS daemon
/// process, regardless of residency.
///
/// The true root cause this closes: on Windows, a caller-initiated restart
/// (e.g. GUI settings-change restart in `uc-desktop::daemon_probe::restart_local_daemon`)
/// has no real signal to send — `TerminateProcess` kills the process before its
/// async graceful-shutdown task ever runs `mark_clean_exit()`, so the next boot
/// logs a false "previous daemon run exited abnormally". This endpoint lets the
/// caller ask first: the start marker is cleared HERE, synchronously, before the
/// shutdown sequence even starts, so the marker is already correct even if the
/// caller's graceful wait times out and it falls back to a hard kill.
///
/// Deliberately NOT routed through [`RestartCoordinator`] — that machinery is
/// reserved for Oneshot-residency promotion (ADR-008 P5-L L8c) and refuses any
/// other residency. This endpoint works for every residency and does not touch
/// `quiescing`.
#[utoipa::path(
    post,
    path = "/lifecycle/graceful-stop",
    tag = "lifecycle",
    operation_id = "requestGracefulStop",
    responses(
        (status = 202, description = "Graceful stop requested; shutdown sequence started")
    )
)]
async fn graceful_stop_handler(State(state): State<DaemonApiState>) -> impl IntoResponse {
    info!("graceful stop requested via HTTP control plane");
    perform_graceful_stop(state.run_marker.as_ref(), &state.graceful_stop_requested);
    StatusCode::ACCEPTED.into_response()
}

/// Pure(ish) side-effect core of [`graceful_stop_handler`], split out so it is
/// unit-testable without composing a full `DaemonApiState` (would otherwise
/// require a real `Engine`).
fn perform_graceful_stop(
    run_marker: Option<&uc_daemon_local::crash_marker::DaemonRunMarker>,
    notify: &tokio::sync::Notify,
) {
    if let Some(marker) = run_marker {
        if let Err(error) = marker.mark_clean_exit() {
            tracing::warn!(%error, "graceful-stop: failed to mark this run's clean exit");
        }
    }
    notify.notify_one();
}

#[cfg(test)]
mod graceful_stop_tests {
    use super::*;
    use uc_daemon_local::crash_marker::DaemonRunMarker;

    #[tokio::test]
    async fn graceful_stop_clears_the_start_marker_and_notifies() {
        let temp = tempfile::TempDir::new().unwrap();
        let marker = DaemonRunMarker::new(temp.path().to_path_buf());
        // Simulate an in-flight run: a start marker is on disk.
        marker.begin_run(4242).unwrap();

        let notify = tokio::sync::Notify::new();
        perform_graceful_stop(Some(&marker), &notify);

        // The marker must be cleared SYNCHRONOUSLY by the handler, not deferred
        // to the (not-yet-run) shutdown sequence.
        assert_eq!(
            marker.begin_run(9999).unwrap(),
            None,
            "a graceful-stop request must clear the start marker so the next boot is silent"
        );
        // The select loop's wait must be woken.
        tokio::time::timeout(std::time::Duration::from_millis(50), notify.notified())
            .await
            .expect("graceful-stop must notify the shutdown waiter");
    }

    #[tokio::test]
    async fn graceful_stop_without_a_run_marker_still_notifies() {
        // Assembly paths that don't wire a run marker (tests, non-standard
        // hosts) must not panic — only the marker side effect is skipped.
        let notify = tokio::sync::Notify::new();
        perform_graceful_stop(None, &notify);

        tokio::time::timeout(std::time::Duration::from_millis(50), notify.notified())
            .await
            .expect("graceful-stop must notify even without a run marker");
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
