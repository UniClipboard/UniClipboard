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
use uc_daemon_process::socket::resolve_daemon_http_addr;

pub use connection::DaemonConnectionState;
pub use http::{
    DaemonAnalyticsClient, DaemonClipboardClient, DaemonLifecycleClient, DaemonMobileSyncClient,
    DaemonPairingClient, DaemonPairingRequestError, DaemonQueryClient, DaemonSearchClient,
    DaemonSearchRequestError, DaemonSettingsClient, DaemonSetupClient, DaemonSetupV2Client,
    DaemonUpgradeClient, ExchangedSessionToken, SearchQueryRequest,
};
pub use http_ws_service::HttpWsDaemonService;
pub use service::{DaemonService, FileExport};
pub use ws_bridge::{BridgeState, DaemonWsBridge, DaemonWsBridgeConfig, DaemonWsBridgeError};

const ENV_BASE_URL: &str = "UNICLIPBOARD_DAEMON_BASE_URL";
const ENV_TOKEN_PATH: &str = "UNICLIPBOARD_DAEMON_TOKEN_PATH";

/// Resolve the daemon base URL for client connections.
///
/// Checks `UNICLIPBOARD_DAEMON_BASE_URL` env var first, then falls back to
/// resolving the profile-aware loopback HTTP address from the daemon socket.
fn resolve_base_url() -> Result<String> {
    if let Ok(value) = std::env::var(ENV_BASE_URL) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.trim_end_matches('/').to_string());
        }
    }

    // resolve_daemon_http_addr() returns SocketAddr directly (panics on error)
    let addr = resolve_daemon_http_addr();
    Ok(format!("http://{}:{}", addr.ip(), addr.port()))
}

/// Resolve the filesystem path to the daemon authentication token.
///
/// Checks the `UNICLIPBOARD_DAEMON_TOKEN_PATH` environment variable first (if set and non-empty);
/// otherwise uses the platform/profile-aware daemon token location.
///
/// # Returns
///
/// The resolved `PathBuf` pointing to the daemon token on success.
///
/// # Examples
///
/// ```ignore
/// let path = uc_daemon_client::resolve_token_path().unwrap();
/// eprintln!("daemon token path: {}", path.display());
/// ```
fn resolve_token_path() -> Result<PathBuf> {
    if let Ok(value) = std::env::var(ENV_TOKEN_PATH) {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }

    uc_daemon_process::socket::resolve_daemon_token_path().map_err(anyhow::Error::from)
}

/// Resolve the daemon connection info from environment for CLI clients.
///
/// This is the CLI equivalent of what the GUI gets from the daemon lifecycle manager.
/// Reads bearer token from daemon.token file and resolves the HTTP base URL.
pub fn resolve_connection_info_from_env() -> Result<DaemonConnectionInfo> {
    let base_url = resolve_base_url()?;
    let token_path = resolve_token_path()?;

    let token = std::fs::read_to_string(&token_path).with_context(|| {
        if token_path.exists() {
            format!(
                "failed to read daemon auth token at {}",
                token_path.display()
            )
        } else {
            format!(
                "daemon auth token not found at {} (is the daemon running?)",
                token_path.display()
            )
        }
    })?;
    let token = token.trim().to_string();
    if token.is_empty() {
        anyhow::bail!("daemon auth token at {} is empty", token_path.display());
    }

    let cli_pid = std::process::id();
    Ok(DaemonConnectionInfo {
        base_url: base_url.clone(),
        ws_url: format!(
            "{}/ws",
            base_url
                .replacen("http://", "ws://", 1)
                .replacen("https://", "wss://", 1)
        ),
        token,
        pid: cli_pid,
    })
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
/// let setup = ctx.setup_client();
///
/// // For CLI (short-lived, exchanges fresh token each call):
/// let ctx = DaemonClientContext::from_env()?;
/// let setup = ctx.setup_client();
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
    /// Reads the bearer token from `daemon.token` and resolves the HTTP base URL
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

    /// Spawn a [`DaemonSetupClient`] that shares this context's connection state and HTTP client.
    pub fn setup_client(&self) -> DaemonSetupClient {
        DaemonSetupClient::with_http_conn_state_and_type(
            self.http.clone(),
            self.connection_state.clone(),
            self.client_type.clone(),
        )
    }

    /// Spawn a [`DaemonSetupV2Client`] that shares this context's connection state and HTTP client.
    pub fn setup_v2_client(&self) -> DaemonSetupV2Client {
        DaemonSetupV2Client::with_http_conn_state_and_type(
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

    /// Spawn a [`DaemonSearchClient`] that shares this context's connection state and HTTP client.
    pub fn search_client(&self) -> DaemonSearchClient {
        DaemonSearchClient::with_http_conn_state_and_type(
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
