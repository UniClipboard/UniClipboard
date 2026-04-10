//! HTTP route handlers for local statistics endpoints.

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;
use uc_app::usecases::CoreUseCases;
use uc_core::network::daemon_api_strings::http_route;

use crate::api::routes::internal_error;
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new().route(
        http_route::LOCAL_STATS_DASHBOARD,
        get(get_local_stats_dashboard_handler),
    )
}

async fn get_local_stats_dashboard_handler(
    State(state): State<DaemonApiState>,
) -> impl IntoResponse {
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let usecases = CoreUseCases::new(runtime.as_ref());
    match usecases.get_local_stats_dashboard().execute().await {
        Ok(result) => {
            let ts = chrono::Utc::now().timestamp_millis();
            Json(json!({ "data": result, "ts": ts })).into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "Failed to load local stats dashboard");
            internal_error(error).into_response()
        }
    }
}
