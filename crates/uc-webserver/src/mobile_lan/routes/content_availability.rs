//! `GET /api/mobile-sync/content-availability` — mobile-only content-existence probe.
//!
//! Deliberately **not** part of the SyncClipboard-compat wire surface (`/api/history/*`,
//! `/SyncClipboard.json`): those routes serve third-party SyncClipboard clients under a
//! protocol contract this project does not own, and `/api/history/{profileId}`'s `hash`
//! segment intentionally tolerates drift for re-encoded Image/File content (see
//! `history.rs`'s `current_profile_hash_is_compatible`) — it cannot answer "does this exact
//! content exist" reliably. This route exists so our own mobile client can ask that question
//! precisely, keyed by the client-reproducible `snapshot_hash` (`"blake3v1:<hex>"`,
//! `uc-content-hash` crate) instead of a per-request SHA-256 that a server-side re-encode can
//! invalidate.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};

use uc_application::facade::{CheckContentAvailableError, MobileSyncFacade};

#[derive(Debug, Deserialize)]
pub(super) struct ContentAvailabilityQuery {
    #[serde(rename = "snapshotHash")]
    snapshot_hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ContentAvailabilityDoc {
    available: bool,
}

pub(super) async fn get_content_availability(
    State(facade): State<Arc<MobileSyncFacade>>,
    Query(query): Query<ContentAvailabilityQuery>,
) -> Result<Json<ContentAvailabilityDoc>, Response> {
    if query.snapshot_hash.trim().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "snapshotHash is required").into_response());
    }
    match facade.check_content_available(&query.snapshot_hash).await {
        Ok(available) => {
            tracing::info!(
                snapshot_hash = %query.snapshot_hash,
                available,
                "GET /api/mobile-sync/content-availability: 200"
            );
            Ok(Json(ContentAvailabilityDoc { available }))
        }
        Err(CheckContentAvailableError::Repository(err)) => {
            tracing::warn!(
                error = %err,
                "GET /api/mobile-sync/content-availability: lookup failed"
            );
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}
