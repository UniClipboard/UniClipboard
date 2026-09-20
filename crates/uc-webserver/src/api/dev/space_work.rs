//! Process-scoped Engine event controls for isolated GUI end-to-end tests.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use uc_engine::{DevOperation, DevOperationResult, DevSpaceWorkEvent, DevSpaceWorkEventKind};

use crate::api::server::DaemonApiState;

pub fn router(state: DaemonApiState) -> Router<DaemonApiState> {
    Router::new()
        .route("/e2e/space-work", post(space_work))
        .with_state(state)
}

#[derive(Deserialize)]
struct Request {
    command: String,
    after_sequence: Option<u64>,
    kind: Option<String>,
}

fn kind(name: &str) -> Option<DevSpaceWorkEventKind> {
    match name {
        "final_confirmation_connection_failed" => {
            Some(DevSpaceWorkEventKind::FinalConfirmationConnectionFailed)
        }
        "final_confirmation_retry_started" => {
            Some(DevSpaceWorkEventKind::FinalConfirmationRetryStarted)
        }
        "final_confirmation_reply_received" => {
            Some(DevSpaceWorkEventKind::FinalConfirmationReplyReceived)
        }
        "ordinary_member_update_started" => {
            Some(DevSpaceWorkEventKind::OrdinaryMemberUpdateStarted)
        }
        "membership_history_sync_started" => {
            Some(DevSpaceWorkEventKind::MembershipHistorySyncStarted)
        }
        _ => None,
    }
}

fn event(value: DevSpaceWorkEvent) -> Value {
    let name = match value.kind {
        DevSpaceWorkEventKind::FinalConfirmationConnectionFailed => {
            "final_confirmation_connection_failed"
        }
        DevSpaceWorkEventKind::FinalConfirmationRetryStarted => "final_confirmation_retry_started",
        DevSpaceWorkEventKind::FinalConfirmationReplyReceived => {
            "final_confirmation_reply_received"
        }
        DevSpaceWorkEventKind::OrdinaryMemberUpdateStarted => "ordinary_member_update_started",
        DevSpaceWorkEventKind::MembershipHistorySyncStarted => "membership_history_sync_started",
    };
    json!({ "sequence": value.sequence, "kind": name })
}

async fn space_work(
    State(state): State<DaemonApiState>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> Result<Json<Value>, StatusCode> {
    let token = std::env::var("UC_E2E_SPACE_WORK_TOKEN").map_err(|_| StatusCode::NOT_FOUND)?;
    let rendezvous =
        std::env::var("UC_E2E_RENDEZVOUS_BASE_URL").map_err(|_| StatusCode::NOT_FOUND)?;
    if token.len() < 32
        || !rendezvous.starts_with("http://127.0.0.1:")
        || std::env::var("UNICLIPBOARD_ENV").as_deref() != Ok("development")
    {
        return Err(StatusCode::NOT_FOUND);
    }
    if headers
        .get("x-uc-e2e-space-work-token")
        .and_then(|value| value.to_str().ok())
        != Some(&token)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    let operation = match request.command.as_str() {
        "arm_complete_ack_failure" => DevOperation::ArmFinalConfirmationConnectionFailure,
        "wait_space_work_event" => DevOperation::WaitForSpaceWorkEvent {
            after_sequence: request.after_sequence.ok_or(StatusCode::BAD_REQUEST)?,
            kind: kind(request.kind.as_deref().ok_or(StatusCode::BAD_REQUEST)?)
                .ok_or(StatusCode::BAD_REQUEST)?,
        },
        "space_work_events" => DevOperation::QuerySpaceWorkEvents,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let result = state
        .engine
        .execute_dev(operation)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let value = match result {
        DevOperationResult::FinalConfirmationConnectionFailureArmed { after_sequence } => {
            json!({ "after_sequence": after_sequence })
        }
        DevOperationResult::SpaceWorkEvent(value) => event(value),
        DevOperationResult::SpaceWorkEvents(values) => {
            Value::Array(values.into_iter().map(event).collect())
        }
        _ => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    Ok(Json(value))
}
