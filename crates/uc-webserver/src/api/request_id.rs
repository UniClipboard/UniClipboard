//! Request identity shared by the access log, the response header and error
//! bodies.
//!
//! A caller that already has a correlation id (the GUI frontend, the CLI) may
//! send it as `x-request-id`; the daemon keeps it when it is well formed and
//! generates one otherwise. The id is a log field and a response header only:
//! it never carries meaning for the request itself.

use axum::http::HeaderMap;

/// Header carrying the request id in both directions.
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// Longest inbound id that is kept.
const MAX_REQUEST_ID_LEN: usize = 64;

tokio::task_local! {
    static CURRENT: String;
}

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

/// Run `future` with `request_id` as the current request's id.
pub(crate) async fn scope<F: std::future::Future>(request_id: String, future: F) -> F::Output {
    CURRENT.scope(request_id, future).await
}

/// Id of the request being handled, `None` outside a request.
pub(crate) fn current() -> Option<String> {
    CURRENT.try_with(Clone::clone).ok()
}
