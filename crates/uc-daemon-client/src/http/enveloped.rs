//! Single seam for enveloped daemon HTTP requests (ADR-008 §H).
//!
//! Every enveloped endpoint's success body is the canonical
//! `ApiEnvelope<T> { data, ts }`. This module owns the full request ritual —
//! connection check, session-token authorization, transport, status/error
//! normalization, envelope decode — so feature clients declare only the
//! payload type `T`. Endpoints exempt from the envelope (binary bodies,
//! empty 2xx responses) use [`send_checked`] / [`empty_request`] to share
//! everything but the decode.

use reqwest::{Method, RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;

use crate::http::authorized_daemon_request_with_type;
use crate::DaemonConnectionState;

/// Structured error for a daemon HTTP request.
///
/// Callers branch on variants/fields instead of scraping message strings:
/// `Status.code` carries the daemon's stable error code (e.g.
/// `session_locked`, `restart_in_progress`) parsed from the JSON error body,
/// and `Status.message` the human-readable server text (falling back to the
/// raw body when it isn't the canonical `{ code, message }` shape).
#[derive(Debug, thiserror::Error)]
pub enum DaemonRequestError {
    #[error("daemon connection info is not available")]
    NotConnected,

    #[error("failed to authorize daemon request {path}: {error}")]
    Auth {
        path: String,
        // Named `error`, not `source`: anyhow::Error does not implement
        // std::error::Error, so it cannot be a thiserror #[source].
        error: anyhow::Error,
    },

    #[error("failed to call daemon route {path}: {source}")]
    Transport {
        path: String,
        source: reqwest::Error,
    },

    #[error("daemon request {path} failed with status {status}{}: {message}", code_suffix(.code))]
    Status {
        path: String,
        status: StatusCode,
        code: Option<String>,
        message: String,
    },

    #[error("failed to decode daemon response for {path}: {source}")]
    Decode {
        path: String,
        source: reqwest::Error,
    },
}

impl DaemonRequestError {
    /// HTTP status of a non-2xx response, if that is what failed.
    pub fn status(&self) -> Option<StatusCode> {
        match self {
            Self::Status { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Stable daemon error code (e.g. `session_locked`) from the error body.
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Status { code, .. } => code.as_deref(),
            _ => None,
        }
    }

    /// Human-readable server message from the error body.
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Status { message, .. } => Some(message),
            _ => None,
        }
    }

    /// True when the daemon answered 404 (commonly mapped to `Ok(None)`).
    pub fn is_not_found(&self) -> bool {
        self.status() == Some(StatusCode::NOT_FOUND)
    }
}

fn code_suffix(code: &Option<String>) -> String {
    code.as_deref()
        .map(|c| format!(" [{c}]"))
        .unwrap_or_default()
}

/// Parse the daemon's canonical `{ code, message }` error body. Falls back to
/// the raw body as the message when the shape doesn't match.
fn parse_error_body(body: &str) -> (Option<String>, String) {
    #[derive(serde::Deserialize)]
    struct ErrorBody {
        code: Option<String>,
        message: Option<String>,
    }
    match serde_json::from_str::<ErrorBody>(body) {
        Ok(parsed) => (
            parsed.code,
            parsed.message.unwrap_or_else(|| body.to_string()),
        ),
        Err(_) => (None, body.to_string()),
    }
}

/// Build, authorize, and send a daemon request; normalize non-2xx into
/// [`DaemonRequestError::Status`]. Returns the raw 2xx [`Response`] for
/// callers that read binary bodies or headers.
pub(crate) async fn send_checked(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    client_type: &str,
    method: Method,
    path: &str,
    customize: impl FnOnce(RequestBuilder) -> RequestBuilder,
) -> Result<Response, DaemonRequestError> {
    let connection = connection_state
        .get()
        .ok_or(DaemonRequestError::NotConnected)?;
    let request = authorized_daemon_request_with_type(
        http,
        connection_state,
        method,
        path,
        connection.pid,
        client_type,
    )
    .await
    .map_err(|error| DaemonRequestError::Auth {
        path: path.to_string(),
        error,
    })?;

    let response =
        customize(request)
            .send()
            .await
            .map_err(|source| DaemonRequestError::Transport {
                path: path.to_string(),
                source,
            })?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        let (code, message) = parse_error_body(&body);
        return Err(DaemonRequestError::Status {
            path: path.to_string(),
            status,
            code,
            message,
        });
    }

    Ok(response)
}

/// Send an enveloped request and return the unwrapped payload `T`.
///
/// `customize` attaches the request body / query params (use the identity
/// closure `|r| r` for plain requests). The `ts` half of the envelope is
/// intentionally dropped — no client consumes it.
pub(crate) async fn enveloped_request<T>(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    client_type: &str,
    method: Method,
    path: &str,
    customize: impl FnOnce(RequestBuilder) -> RequestBuilder,
) -> Result<T, DaemonRequestError>
where
    T: DeserializeOwned,
{
    let response =
        send_checked(http, connection_state, client_type, method, path, customize).await?;
    let envelope: ApiEnvelope<T> =
        response
            .json()
            .await
            .map_err(|source| DaemonRequestError::Decode {
                path: path.to_string(),
                source,
            })?;
    Ok(envelope.data)
}

/// Send a request whose success body is empty (or ignored). Shares the full
/// ritual with [`enveloped_request`] minus the envelope decode.
pub(crate) async fn empty_request(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    client_type: &str,
    method: Method,
    path: &str,
    customize: impl FnOnce(RequestBuilder) -> RequestBuilder,
) -> Result<(), DaemonRequestError> {
    send_checked(http, connection_state, client_type, method, path, customize).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_error_body() {
        let (code, message) =
            parse_error_body(r#"{"code":"session_locked","message":"unlock first"}"#);
        assert_eq!(code.as_deref(), Some("session_locked"));
        assert_eq!(message, "unlock first");
    }

    #[test]
    fn falls_back_to_raw_body_when_message_missing() {
        let (code, message) = parse_error_body(r#"{"code":"oops"}"#);
        assert_eq!(code.as_deref(), Some("oops"));
        assert_eq!(message, r#"{"code":"oops"}"#);
    }

    #[test]
    fn falls_back_to_raw_body_on_non_json() {
        let (code, message) = parse_error_body("<html>502</html>");
        assert_eq!(code, None);
        assert_eq!(message, "<html>502</html>");
    }

    #[test]
    fn status_error_display_includes_code() {
        let err = DaemonRequestError::Status {
            path: "/search/rebuild".to_string(),
            status: StatusCode::CONFLICT,
            code: Some("rebuild_already_running".to_string()),
            message: "rebuild in progress".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "daemon request /search/rebuild failed with status 409 Conflict [rebuild_already_running]: rebuild in progress"
        );
    }
}
