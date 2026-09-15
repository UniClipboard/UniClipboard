pub mod analytics;
pub mod clipboard;
pub mod config;
pub mod diagnostics;
pub mod enveloped;
pub mod lifecycle;
pub mod member;
pub mod mobile_sync;
pub mod pairing;
pub mod query;
pub mod search;
pub mod settings;
pub mod setup_v2;
pub mod upgrade;

pub use analytics::DaemonAnalyticsClient;
pub use clipboard::DaemonClipboardClient;
pub use config::DaemonConfigClient;
pub use diagnostics::DaemonDiagnosticsClient;
pub use enveloped::DaemonRequestError;
pub use lifecycle::DaemonLifecycleClient;
pub use member::DaemonMemberClient;
pub use mobile_sync::DaemonMobileSyncClient;
pub use pairing::{DaemonPairingClient, DaemonPairingRequestError};
pub use query::DaemonQueryClient;
pub use search::{DaemonSearchClient, SearchQueryRequest};
pub use settings::DaemonSettingsClient;
pub use setup_v2::DaemonSetupV2Client;
pub use upgrade::DaemonUpgradeClient;

use crate::DaemonConnectionState;
use anyhow::{anyhow, Context, Result};
use reqwest::header::AUTHORIZATION;
use reqwest::{Method, RequestBuilder};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn encode_path_segment(segment: &str) -> Result<String> {
    if matches!(segment, "." | "..") {
        anyhow::bail!("dynamic URL path segment cannot be `.` or `..`");
    }
    const PATH_SEGMENT: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'.')
        .remove(b'_')
        .remove(b'~');
    Ok(percent_encoding::utf8_percent_encode(segment, PATH_SEGMENT).to_string())
}

/// Session details returned by `/auth/connect`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangedSessionToken {
    pub session_token: String,
    pub expires_in_secs: i64,
    pub refresh_at_secs: i64,
}

/// Exchange the bearer token for a JWT session token via POST /auth/connect.
///
/// The returned session token must be used with `Authorization: Session <token>` header
/// for all daemon HTTP and WebSocket requests (the daemon's auth middleware requires
/// a valid JWT, not the raw bearer token).
///
/// NOTE: `pid` should be the current process ID (used for PID whitelist verification
/// in the daemon's JWT middleware). `client_type` distinguishes the client (e.g. "gui", "cli").
///
/// Exposed as `pub` so the CLI crate can call it directly.
pub async fn exchange_session_token(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    pid: u32,
    client_type: &str,
) -> Result<String> {
    Ok(
        exchange_session_token_with_metadata(http, connection_state, pid, client_type)
            .await?
            .session_token,
    )
}

/// Exchange the bearer token for JWT session metadata via POST /auth/connect.
pub async fn exchange_session_token_with_metadata(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    pid: u32,
    client_type: &str,
) -> Result<ExchangedSessionToken> {
    let connection = connection_state
        .get()
        .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
    exchange_session_token_for_connection(http, &connection, pid, client_type).await
}

async fn exchange_session_token_for_connection(
    http: &reqwest::Client,
    connection: &uc_daemon_contract::api::auth::DaemonConnectionInfo,
    pid: u32,
    client_type: &str,
) -> Result<ExchangedSessionToken> {
    let url = format!("{}/auth/connect", connection.base_url);
    let response = http
        .post(&url)
        .header(AUTHORIZATION, format!("Bearer {}", connection.token))
        .json(&serde_json::json!({
            "pid": pid,
            "clientType": client_type
        }))
        .send()
        .await
        .context("failed to send session token exchange request")?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "session token exchange failed with status {}: {}",
            status,
            body
        );
    }

    // Wire shape (ADR-008 §H): `/auth/connect` is now enveloped as
    // `{ data: SessionTokenResponse, ts }`. This is the L1/public bootstrap
    // handshake — decode the canonical envelope and unwrap `.data`. The public
    // `ExchangedSessionToken` return type is unchanged.
    use uc_daemon_contract::api::dto::auth::SessionTokenResponse;
    use uc_daemon_contract::api::dto::envelope::ApiEnvelope;

    let envelope: ApiEnvelope<SessionTokenResponse> = response
        .json()
        .await
        .context("failed to decode session token exchange response")?;
    let resp = envelope.data;

    Ok(ExchangedSessionToken {
        session_token: resp.session_token,
        expires_in_secs: resp.expires_in_secs,
        refresh_at_secs: resp.refresh_at_secs,
    })
}

/// Exchange bearer token for JWT session using the CLI client type.
/// Shorthand for `exchange_session_token(http, conn, pid, "cli")`.
pub async fn exchange_cli_session_token(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    pid: u32,
) -> Result<String> {
    exchange_session_token(http, connection_state, pid, "cli").await
}

/// Get or exchange the session token for daemon auth.
///
/// Lazily exchanges the bearer token on first call, then caches the result.
/// Returns the cached session token if still valid (with 30-second buffer before expiry).
/// Exposed as `pub(crate)` so the WS bridge can also use it.
pub(crate) async fn get_session_token(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    pid: u32,
) -> Result<String> {
    let snapshot = connection_state
        .snapshot()
        .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
    get_session_token_for_snapshot(http, connection_state, pid, &snapshot).await
}

async fn get_session_token_for_snapshot(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    pid: u32,
    snapshot: &(uc_daemon_contract::api::auth::DaemonConnectionInfo, u64),
) -> Result<String> {
    let (connection, revision) = snapshot;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    if let Some(token) = connection_state.cached_session_token(*revision, now) {
        return Ok(token);
    }

    let new_token = exchange_session_token_for_connection(http, connection, pid, "gui").await?;
    let expires_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 300; // TTL from /auth/connect response

    connection_state.cache_session_token_if_current(
        *revision,
        new_token.session_token.clone(),
        expires_at,
    );

    Ok(new_token.session_token)
}

/// Build an authorized HTTP request using the session token (JWT).
///
/// This is the async version — it lazily exchanges the bearer token for a session JWT
/// for "gui" client type on first call, then uses `Authorization: Session <token>`
/// for all subsequent requests.
/// Use this for all daemon HTTP API calls from the daemon-client.
pub async fn authorized_daemon_request(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    method: Method,
    path: &str,
    pid: u32,
) -> Result<RequestBuilder> {
    authorized_daemon_request_with_type(http, connection_state, method, path, pid, "gui").await
}

/// Build an authorized HTTP request using the session token (JWT) with explicit client type.
///
/// Uses cached tokens for "gui" client type (via `get_session_token`) to avoid
/// redundant /auth/connect calls in long-running GUI processes.
/// For other client types (e.g. "cli"), calls `exchange_session_token` directly
/// so each invocation gets a fresh token.
pub async fn authorized_daemon_request_with_type(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    method: Method,
    path: &str,
    pid: u32,
    client_type: &str,
) -> Result<RequestBuilder> {
    let snapshot = connection_state
        .snapshot()
        .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
    authorized_daemon_request_for_snapshot(
        http,
        connection_state,
        method,
        path,
        pid,
        client_type,
        snapshot,
    )
    .await
}

async fn authorized_daemon_request_for_snapshot(
    http: &reqwest::Client,
    connection_state: &DaemonConnectionState,
    method: Method,
    path: &str,
    pid: u32,
    client_type: &str,
    snapshot: (uc_daemon_contract::api::auth::DaemonConnectionInfo, u64),
) -> Result<RequestBuilder> {
    let (connection, _) = &snapshot;
    let url = format!("{}{}", connection.base_url, path);

    // GUI clients benefit from token caching (long-running process).
    // CLI and other types use fresh tokens each call.
    let session_token = if client_type == "gui" {
        get_session_token_for_snapshot(http, connection_state, pid, &snapshot).await?
    } else {
        exchange_session_token_for_connection(http, connection, pid, client_type)
            .await?
            .session_token
    };

    Ok(http
        .request(method, url)
        .header(AUTHORIZATION, format!("Session {}", session_token)))
}

#[cfg(test)]
mod connection_replacement_tests {
    use super::*;
    use std::time::Duration;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn authorized_request_keeps_one_snapshot_during_connection_replacement() {
        let first_server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/connect"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(Duration::from_millis(10))
                    .set_body_json(serde_json::json!({
                        "data": {
                            "sessionToken": "first-session",
                            "expiresInSecs": 300,
                            "refreshAtSecs": 240
                        },
                        "ts": 1
                    })),
            )
            .expect(1)
            .mount(&first_server)
            .await;
        let second_server = MockServer::start().await;
        let connection_state = DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: first_server.uri(),
            ws_url: "ws://127.0.0.1/first".to_string(),
            token: "first-bearer".to_string(),
            pid: 42,
        });
        let snapshot = connection_state.snapshot().expect("first connection");
        connection_state.set(DaemonConnectionInfo {
            base_url: second_server.uri(),
            ws_url: "ws://127.0.0.1/second".to_string(),
            token: "second-bearer".to_string(),
            pid: 42,
        });

        let request = authorized_daemon_request_for_snapshot(
            &reqwest::Client::new(),
            &connection_state,
            Method::GET,
            "/paired-devices",
            42,
            "gui",
            snapshot,
        )
        .await
        .expect("authorized request")
        .build()
        .expect("request");

        assert_eq!(
            request.url().as_str(),
            format!("{}/paired-devices", first_server.uri())
        );
        assert_eq!(
            request.headers().get(AUTHORIZATION).expect("authorization"),
            "Session first-session"
        );
        let (_, current_revision) = connection_state.snapshot().expect("second connection");
        assert_eq!(
            connection_state.cached_session_token(current_revision, 0),
            None
        );
    }
}
