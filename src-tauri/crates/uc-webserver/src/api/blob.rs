//! HTTP endpoints for serving raw blob and thumbnail binary content.
//!
//! These endpoints return binary data with Content-Type headers,
//! replacing the uc:// custom protocol handler in uc-tauri.

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use uc_application::facade::ResourceFacadeError;

use crate::api::dto::error::log_facade_failure;
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/clipboard/blobs/:blob_id", get(get_blob))
        .route("/clipboard/thumbnails/:rep_id", get(get_thumbnail))
        .route("/clipboard/entries/:id/file", get(get_entry_file))
}

/// GET /clipboard/blobs/:blob_id
///
/// Serves the raw bytes of a stored blob. Binary endpoint: the response is
/// `application/octet-stream` (the resolved MIME type when known), NOT the
/// `{ data, ts }` JSON envelope (ADR-008 §0.2 keeps binary endpoints exempt).
/// Returns 404 if the blob is unknown, 500 on an internal resolution failure.
#[utoipa::path(
    get,
    path = "/clipboard/blobs/{blob_id}",
    tag = "clipboard",
    operation_id = "getClipboardBlob",
    params(
        ("blob_id" = String, Path, description = "Blob identifier"),
    ),
    responses(
        (
            status = 200,
            description = "Raw blob bytes",
            content_type = "application/octet-stream",
            body = Vec<u8>,
        ),
        (status = 404, description = "Blob not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_blob(
    State(state): State<DaemonApiState>,
    Path(blob_id): Path<String>,
) -> impl IntoResponse {
    let app = match state.app_facade_or_error() {
        Ok(app) => app,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "daemon application facade unavailable",
            )
                .into_response();
        }
    };

    // D6 (ADR-008 P3-d) interim RSS guard: bound concurrent full-buffer blob
    // materialization until the streaming `BlobReaderPort` lands (see
    // `DaemonApiState::large_blob_semaphore` and the P0 perf spike §4). Held for
    // the materialization window — the dominant RSS driver; the subsequent
    // loopback send is sub-10ms (spike §2). `acquire_owned` only errors if the
    // semaphore is closed (we never close it); on that impossible path we
    // proceed unguarded rather than fail the pull.
    let _permit = state
        .large_blob_semaphore
        .clone()
        .acquire_owned()
        .await
        .ok();

    match app.resource.blob(&blob_id).await {
        Ok(result) => {
            let content_type = result
                .mime_type
                .as_deref()
                .unwrap_or("application/octet-stream");
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, content_type.to_string())],
                result.bytes,
            )
                .into_response()
        }
        Err(err) => map_resource_error("get_blob", err, "blob not found", &blob_id),
    }
}

/// GET /clipboard/thumbnails/:rep_id
///
/// Serves the raw bytes of a representation's thumbnail. Binary endpoint: the
/// response is `application/octet-stream` (the resolved MIME type when known),
/// NOT the `{ data, ts }` JSON envelope (ADR-008 §0.2 keeps binary endpoints
/// exempt). Returns 404 if the thumbnail is unknown, 500 on an internal
/// resolution failure.
#[utoipa::path(
    get,
    path = "/clipboard/thumbnails/{rep_id}",
    tag = "clipboard",
    operation_id = "getClipboardThumbnail",
    params(
        ("rep_id" = String, Path, description = "Representation identifier"),
    ),
    responses(
        (
            status = 200,
            description = "Raw thumbnail bytes",
            content_type = "application/octet-stream",
            body = Vec<u8>,
        ),
        (status = 404, description = "Thumbnail not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_thumbnail(
    State(state): State<DaemonApiState>,
    Path(rep_id): Path<String>,
) -> impl IntoResponse {
    let app = match state.app_facade_or_error() {
        Ok(app) => app,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "daemon application facade unavailable",
            )
                .into_response();
        }
    };

    match app.resource.thumbnail(&rep_id).await {
        Ok(result) => {
            let content_type = result
                .mime_type
                .as_deref()
                .unwrap_or("application/octet-stream");
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, content_type.to_string())],
                result.bytes,
            )
                .into_response()
        }
        Err(err) => map_resource_error("get_thumbnail", err, "thumbnail not found", &rep_id),
    }
}

/// GET /clipboard/entries/:id/file
///
/// Serves the bytes of an entry's first materialized free-file (ADR-008
/// P5-1b). The daemon materializes inbound free-files into a controlled cache
/// and rewrites the entry's file-list representation; this endpoint reads that
/// cached file and streams it back. Binary endpoint: the response is
/// `application/octet-stream` (the representation MIME when known), NOT the
/// `{ data, ts }` JSON envelope (ADR-008 §0.2 keeps binary endpoints exempt).
/// A `Content-Disposition: attachment` header carries the cached filename so
/// CLI `recv` can name the local copy. Returns 404 when the entry is unknown
/// or carries no materialized free-file, 500 on a read failure.
#[utoipa::path(
    get,
    path = "/clipboard/entries/{id}/file",
    tag = "clipboard",
    operation_id = "getClipboardEntryFile",
    params(
        ("id" = String, Path, description = "Entry identifier"),
    ),
    responses(
        (
            status = 200,
            description = "Raw bytes of the entry's first materialized file",
            content_type = "application/octet-stream",
            body = Vec<u8>,
        ),
        (status = 404, description = "Entry or file not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
async fn get_entry_file(
    State(state): State<DaemonApiState>,
    Path(entry_id): Path<String>,
) -> impl IntoResponse {
    let app = match state.app_facade_or_error() {
        Ok(app) => app,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "daemon application facade unavailable",
            )
                .into_response();
        }
    };

    // Mirror get_blob's interim RSS guard: bound concurrent full-buffer file
    // materialization until the streaming reader lands (ADR-008 P3-d / P5-1b).
    let _permit = state
        .large_blob_semaphore
        .clone()
        .acquire_owned()
        .await
        .ok();

    match app.resource.entry_file(&entry_id).await {
        Ok(result) => {
            let content_type = result.mime.as_deref().unwrap_or("application/octet-stream");
            // The facade already sanitized the filename to a bare basename; we
            // additionally drop quotes/control chars so the header value stays
            // well-formed.
            let header_name = sanitize_disposition_filename(&result.filename);
            let disposition = format!("attachment; filename=\"{header_name}\"");
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, content_type.to_string()),
                    (header::CONTENT_DISPOSITION, disposition),
                ],
                result.bytes,
            )
                .into_response()
        }
        Err(err) => map_resource_error("get_entry_file", err, "entry file not found", &entry_id),
    }
}

/// Strip characters that would break a `Content-Disposition` header value
/// (quotes, backslashes, control chars). The facade already removed path
/// separators, so this only guards the header encoding.
fn sanitize_disposition_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '\\'))
        .collect();
    if cleaned.is_empty() {
        "download.bin".to_string()
    } else {
        cleaned
    }
}

fn map_resource_error(
    op: &'static str,
    error: ResourceFacadeError,
    not_found_message: &'static str,
    resource_id: &str,
) -> axum::response::Response {
    use ResourceFacadeError as E;
    let (variant, status, message): (&'static str, StatusCode, String) = match error {
        E::NotFound => (
            "not_found",
            StatusCode::NOT_FOUND,
            not_found_message.to_string(),
        ),
        E::Mismatch(detail) => (
            "mismatch",
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("resource {resource_id} mismatch: {detail}"),
        ),
        E::Internal(detail) => (
            "internal",
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("resource {resource_id} internal: {detail}"),
        ),
    };
    log_facade_failure("resource", op, variant, status, &message);
    let body = if status == StatusCode::NOT_FOUND {
        not_found_message
    } else {
        "internal error"
    };
    (status, body).into_response()
}
