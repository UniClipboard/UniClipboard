//! Request identity shared by the access log, the response header and error
//! bodies.
//!
//! A caller that already has a correlation id (the GUI frontend, the CLI) may
//! send it as `x-request-id`; the daemon keeps it when it is well formed and
//! generates one otherwise. The id is a log field and a response header only:
//! it never carries meaning for the request itself.

use axum::body::Body;
use axum::body::HttpBody as _;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::HeaderMap;
use axum::response::Response;

/// Header carrying the request id in both directions.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Longest inbound id that is kept.
const MAX_REQUEST_ID_LEN: usize = 64;

/// The inbound id when it is 1..=64 characters of `[A-Za-z0-9._-]`, otherwise a
/// fresh random one. The character set keeps log lines and header values safe
/// from injection.
pub(crate) fn accept_or_generate(headers: &HeaderMap) -> String {
    headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|id| is_well_formed(id))
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{:016x}", rand::random::<u64>()))
}

fn is_well_formed(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_REQUEST_ID_LEN
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Largest error body that is inspected; anything bigger is passed through.
const MAX_STAMPED_BODY_BYTES: usize = 64 * 1024;

/// Add `requestId` to a daemon error body.
///
/// Failure bodies of the daemon are JSON objects built in many places: the
/// canonical `{ code, message }` of `ApiErrorResponse` in handlers, route
/// fallbacks and the security middleware, and older `{ error }` rejections. The
/// id is added here, once, after the handler ran, so no producer has to know
/// about it. Responses that are not client or server errors, are not JSON, are
/// larger than the bound, or are not a JSON object are returned unchanged.
pub(crate) async fn stamp_error_body(response: Response, request_id: &str) -> Response {
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error()) {
        return response;
    }
    let is_json = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    let small_enough = response
        .body()
        .size_hint()
        .upper()
        .is_some_and(|upper| upper <= MAX_STAMPED_BODY_BYTES as u64);
    if !is_json || !small_enough {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_STAMPED_BODY_BYTES).await else {
        // The body was read once and cannot be restored: an unreadable body is
        // already a broken response.
        return Response::from_parts(parts, Body::empty());
    };
    let stamped = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|mut value| {
            value
                .as_object_mut()?
                .entry("requestId")
                .or_insert_with(|| serde_json::Value::from(request_id));
            serde_json::to_vec(&value).ok()
        });
    let bytes = match stamped {
        Some(stamped) => {
            parts.headers.remove(CONTENT_LENGTH);
            stamped
        }
        None => bytes.to_vec(),
    };
    Response::from_parts(parts, Body::from(bytes))
}
