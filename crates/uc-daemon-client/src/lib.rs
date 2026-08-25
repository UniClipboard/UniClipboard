//! # uc-daemon-client
//!
//! Daemon HTTP and WebSocket client for UniClipboard.
//! Zero Tauri dependencies -- usable from any async context.

use std::path::PathBuf;
use std::sync::Arc;

pub mod connection;
pub mod http;
pub mod http_ws_service;
pub mod realtime;
pub mod service;
pub mod setup;
pub mod ws_bridge;

use anyhow::{Context, Result};
use uc_daemon_contract::api::auth::DaemonConnectionInfo;

pub use connection::DaemonConnectionState;
pub use http::{
    DaemonAnalyticsClient, DaemonClipboardClient, DaemonConfigClient, DaemonDiagnosticsClient,
    DaemonLifecycleClient, DaemonMemberClient, DaemonMobileSyncClient, DaemonPairingClient,
    DaemonPairingRequestError, DaemonQueryClient, DaemonRequestError, DaemonSearchClient,
    DaemonSettingsClient, DaemonSetupV2Client, DaemonSpacesV2Client, DaemonUpgradeClient,
    ExchangedSessionToken, SearchQueryRequest,
};
pub use http_ws_service::HttpWsDaemonService;
pub use service::{ControlLeaseGuard, DaemonService, FileExport};
pub use ws_bridge::{BridgeState, DaemonWsBridge, DaemonWsBridgeConfig, DaemonWsBridgeError};

const ENV_BASE_URL: &str = "UNICLIPBOARD_DAEMON_BASE_URL";
const ENV_TOKEN_PATH: &str = "UNICLIPBOARD_DAEMON_TOKEN_PATH";

/// Resolve the daemon connection info from environment for CLI clients.
///
/// This is the CLI equivalent of what the GUI gets from the daemon lifecycle manager.
///
/// Resolution order (ADR-011):
/// 1. Explicit `UNICLIPBOARD_DAEMON_BASE_URL` / `UNICLIPBOARD_DAEMON_TOKEN_PATH`
///    overrides (CI / tests / manual runs) — unchanged legacy contract.
/// 2. The `daemon.conn` connection file — the single source of truth for the
///    loopback address and bearer token of the running daemon. Missing file or
///    an unsupported format means the daemon is not reachable / incompatible.
pub fn resolve_connection_info_from_env() -> Result<DaemonConnectionInfo> {
    let env_base_url = non_empty_env(ENV_BASE_URL);
    let env_token_path = non_empty_env(ENV_TOKEN_PATH);

    if env_base_url.is_some() || env_token_path.is_some() {
        return resolve_connection_info_env_only(env_base_url, env_token_path);
    }

    let conn = uc_daemon_process::socket::read_daemon_conn_file()?
        .with_context(|| "daemon connection file not found (is the daemon running?)")?;
    let base_url = format!("http://{}:{}", conn.host, conn.port);
    let ws_url = format!(
        "{}/ws",
        base_url
            .replacen("http://", "ws://", 1)
            .replacen("https://", "wss://", 1)
    );
    Ok(DaemonConnectionInfo {
        base_url,
        ws_url,
        token: conn.token,
        pid: std::process::id(),
    })
}

/// Resolve connection info from explicit env overrides only.
///
/// `UNICLIPBOARD_DAEMON_BASE_URL` pins the base URL; the bearer token comes
/// from `UNICLIPBOARD_DAEMON_TOKEN_PATH` (a plain token file) when set, else
/// from the `daemon.conn` connection file. Kept for the CI/test contract that
/// points at mock daemons.
fn resolve_connection_info_env_only(
    env_base_url: Option<String>,
    env_token_path: Option<String>,
) -> Result<DaemonConnectionInfo> {
    let base_url = env_base_url.ok_or_else(|| {
        anyhow::anyhow!(
            "{ENV_BASE_URL} must be set when using env-based daemon connection overrides"
        )
    })?;

    let token = match env_token_path {
        Some(path) => {
            let path = PathBuf::from(path);
            let token = std::fs::read_to_string(&path).with_context(|| {
                if path.exists() {
                    format!("failed to read daemon auth token at {}", path.display())
                } else {
                    format!(
                        "daemon auth token not found at {} (is the daemon running?)",
                        path.display()
                    )
                }
            })?;
            let token = token.trim().to_string();
            if token.is_empty() {
                anyhow::bail!("daemon auth token at {} is empty", path.display());
            }
            token
        }
        None => {
            uc_daemon_process::socket::read_daemon_conn_file()?
                .context("daemon connection file not found (is the daemon running?)")?
                .token
        }
    };

    Ok(DaemonConnectionInfo {
        base_url: base_url.clone(),
        ws_url: base_url
            .replacen("http://", "ws://", 1)
            .replacen("https://", "wss://", 1),
        token,
        pid: std::process::id(),
    })
}

fn non_empty_env(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        _ => None,
    }
}

/// Shared context for all daemon HTTP clients.
///
/// Create once per process, then use factory methods to spawn child clients
/// that all share the same `DaemonConnectionState`, `reqwest::Client`, and auth
/// token exchange behavior.
///
/// # Example
///
/// ```ignore
/// // For GUI (long-running, caches session tokens):
/// let ctx = DaemonClientContext::new(connection_info);
/// let setup = ctx.setup_v2_client();
///
/// // For CLI (short-lived, exchanges fresh token each call):
/// let ctx = DaemonClientContext::from_env()?;
/// let setup = ctx.setup_v2_client();
/// ```
#[derive(Clone)]
pub struct DaemonClientContext {
    pub(crate) connection_state: DaemonConnectionState,
    pub(crate) http: Arc<reqwest::Client>,
    pub(crate) client_type: String,
}

impl DaemonClientContext {
    /// Create a new context with the given connection info and a default HTTP client.
    ///
    /// Uses `"gui"` as the client type (session tokens are cached per-request
    /// via `get_session_token` in `authorized_daemon_request`).
    pub fn new(connection_info: DaemonConnectionInfo) -> Self {
        let connection_state = DaemonConnectionState::default();
        connection_state.set(connection_info);
        Self {
            connection_state,
            http: Arc::new(reqwest::Client::new()),
            client_type: "gui".to_string(),
        }
    }

    /// Create a CLI context by resolving connection info from environment.
    ///
    /// Reads the bearer token from `daemon.conn` and resolves the HTTP base URL
    /// (ADR-011); env overrides bypass the file for CI/mock scenarios.
    /// via the same profile-aware logic the daemon uses for its socket.
    ///
    /// Uses `"cli"` as the client type — each HTTP request exchanges a fresh
    /// session token (no caching) since CLI processes are short-lived.
    pub fn from_env() -> Result<Self> {
        let connection_info = resolve_connection_info_from_env()?;
        Ok(Self::with_connection_info(
            connection_info,
            "cli".to_string(),
        ))
    }

    /// Create a context with an explicit connection info and client type.
    pub fn with_connection_info(
        connection_info: DaemonConnectionInfo,
        client_type: String,
    ) -> Self {
        let connection_state = DaemonConnectionState::default();
        connection_state.set(connection_info);
        Self {
            connection_state,
            http: Arc::new(reqwest::Client::new()),
            client_type,
        }
    }

    /// Spawn a [`DaemonSetupV2Client`] that shares this context's connection state and HTTP client.
    pub fn setup_v2_client(&self) -> DaemonSetupV2Client {
        DaemonSetupV2Client::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonSpacesV2Client`] that shares this context's connection state and HTTP client.
    pub fn spaces_v2_client(&self) -> DaemonSpacesV2Client {
        DaemonSpacesV2Client::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonPairingClient`] that shares this context's connection state and HTTP client.
    pub fn pairing_client(&self) -> DaemonPairingClient {
        DaemonPairingClient::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonQueryClient`] that shares this context's connection state and HTTP client.
    pub fn query_client(&self) -> DaemonQueryClient {
        DaemonQueryClient::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonClipboardClient`] that shares this context's connection state and HTTP client.
    pub fn clipboard_client(&self) -> DaemonClipboardClient {
        DaemonClipboardClient::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonSettingsClient`] that shares this context's connection state and HTTP client.
    pub fn settings_client(&self) -> DaemonSettingsClient {
        DaemonSettingsClient::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonDiagnosticsClient`] that shares this context's connection state and HTTP client.
    pub fn diagnostics_client(&self) -> DaemonDiagnosticsClient {
        DaemonDiagnosticsClient::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonConfigClient`] that shares this context's connection state and HTTP client.
    pub fn config_client(&self) -> DaemonConfigClient {
        DaemonConfigClient::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonSearchClient`] that shares this context's connection state and HTTP client.
    pub fn search_client(&self) -> DaemonSearchClient {
        DaemonSearchClient::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonMemberClient`] that shares this context's connection state and HTTP client.
    pub fn member_client(&self) -> DaemonMemberClient {
        DaemonMemberClient::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonMobileSyncClient`] that shares this context's connection state and HTTP client.
    pub fn mobile_sync_client(&self) -> DaemonMobileSyncClient {
        DaemonMobileSyncClient::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonLifecycleClient`] that shares this context's connection state and HTTP client.
    pub fn lifecycle_client(&self) -> DaemonLifecycleClient {
        DaemonLifecycleClient::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonUpgradeClient`] that shares this context's connection state and HTTP client.
    pub fn upgrade_client(&self) -> DaemonUpgradeClient {
        DaemonUpgradeClient::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Get a clone of the underlying connection state.
    pub fn connection_state(&self) -> DaemonConnectionState {
        self.connection_state.clone()
    }

    /// Get a clone of the underlying HTTP client.
    pub fn http(&self) -> Arc<reqwest::Client> {
        self.http.clone()
    }

    /// Exchange the raw bearer token for a short-lived daemon session token.
    pub async fn exchange_session_token(
        &self,
        pid: u32,
        client_type: &str,
    ) -> Result<ExchangedSessionToken> {
        http::exchange_session_token_with_metadata(
            self.http.as_ref(),
            &self.connection_state,
            pid,
            client_type,
        )
        .await
    }

    /// Get a clone of the client type.
    pub fn client_type(&self) -> &str {
        &self.client_type
    }
}
