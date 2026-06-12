//! B2: async mobile-sync client over FFI.
//!
//! Execution model (this is the load-bearing part of the spike):
//! - [`MobileSyncClient`] hosts a `current_thread` tokio runtime on ONE
//!   dedicated thread (iOS extension jetsam budget rules out the multi-thread
//!   runtime; spike plan §4). reqwest futures need a tokio reactor, and the
//!   exported async fns are polled by UniFFI's rust-future machinery which
//!   provides none — so every request is `spawn`ed onto that runtime and the
//!   exported fn awaits only the `JoinHandle` (reactor-free).
//! - Seam 3 falls out of this: dropping the exported future (Swift `Task`
//!   cancellation, process suspension tearing down the await) detaches the
//!   spawned request task, it runs to completion on the runtime thread. The
//!   file→metadata window inside [`MobileSyncClient::put_clipboard`] is
//!   therefore atomic with respect to caller-side future drops; only
//!   [`MobileSyncClient::cancel_in_flight`] aborts it explicitly.
//! - Seam 1: rustls 0.23 ships with no default CryptoProvider and this cdylib
//!   has no `main()` to install one, so [`uc_mobile_init`] must be called
//!   before constructing a client; the constructor enforces it.

use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tokio::task::AbortHandle;

// ─── seam 1: process-wide init ──────────────────────────────────────────

static INITIALIZED: OnceLock<()> = OnceLock::new();

/// Install the process-wide rustls `ring` CryptoProvider (idempotent).
///
/// Must be called once per process before constructing a
/// [`MobileSyncClient`] — in every embedding context separately: the iOS app,
/// the keyboard extension, and the share extension each load the cdylib into
/// their own process with no Rust `main()` to do this.
#[uniffi::export]
pub fn uc_mobile_init() {
    INITIALIZED.get_or_init(|| {
        // Err means a provider is already installed (e.g. host test harness);
        // that satisfies the invariant, so it is not an error here.
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn ensure_initialized() -> Result<(), SyncError> {
    if INITIALIZED.get().is_some() {
        Ok(())
    } else {
        Err(SyncError::NotInitialized)
    }
}

// ─── FFI surface types ──────────────────────────────────────────────────

/// Connection target + HTTP Basic Auth credentials, typically taken from a
/// parsed connect URI (`base_url` = one of `urls`, credentials = `user`/`pwd`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct ServerConfig {
    /// Server base URL without trailing slash, e.g. `http://192.168.1.5:42720`.
    pub base_url: String,
    pub username: String,
    pub password: String,
}

/// Mirror of the SyncClipboard `type` values the daemon speaks
/// (`uc-webserver/src/mobile_lan/routes/sync_doc.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ClipboardKind {
    Text,
    Image,
    File,
    Group,
}

/// Clipboard metadata as exchanged with `GET/PUT /SyncClipboard.json`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ClipboardMeta {
    pub kind: ClipboardKind,
    /// Text content for `Text`; file-name hint for payload kinds.
    pub text: String,
    /// Server-side payload name; required when a binary payload exists.
    pub data_name: Option<String>,
    pub has_data: bool,
    pub size: u64,
    /// SHA-256 hex. Optional on upload, always present in daemon responses.
    pub hash: Option<String>,
}

/// Failure surface of the async client.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum SyncError {
    #[error("uc_mobile_init() must be called before constructing a client")]
    NotInitialized,
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    #[error("network: {reason}")]
    Network { reason: String },
    #[error("unauthorized (401): check username/password")]
    Unauthorized,
    #[error("server returned HTTP {status}")]
    Http { status: u16 },
    #[error("protocol: {reason}")]
    Protocol { reason: String },
    #[error("cancelled")]
    Cancelled,
    #[error("internal: {reason}")]
    Internal { reason: String },
}

// ─── wire DTO ───────────────────────────────────────────────────────────

/// Client-side mirror of the daemon's `SyncClipboardDoc` wire schema. The
/// daemon serializes lowercase/camelCase and accepts PascalCase aliases; we
/// emit exactly its response casing so request and response stay symmetric.
#[derive(Debug, Serialize, Deserialize)]
struct WireDoc {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
    #[serde(rename = "dataName", default, skip_serializing_if = "Option::is_none")]
    data_name: Option<String>,
    #[serde(rename = "hasData", default)]
    has_data: bool,
    #[serde(default)]
    size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hash: Option<String>,
}

impl WireDoc {
    fn from_meta(meta: &ClipboardMeta) -> Self {
        Self {
            kind: match meta.kind {
                ClipboardKind::Text => "Text",
                ClipboardKind::Image => "Image",
                ClipboardKind::File => "File",
                ClipboardKind::Group => "Group",
            }
            .to_string(),
            text: meta.text.clone(),
            data_name: meta.data_name.clone(),
            has_data: meta.has_data,
            size: meta.size,
            hash: meta.hash.clone(),
        }
    }

    fn into_meta(self) -> Result<ClipboardMeta, SyncError> {
        let kind = match self.kind.as_str() {
            "Text" => ClipboardKind::Text,
            "Image" => ClipboardKind::Image,
            "File" => ClipboardKind::File,
            "Group" => ClipboardKind::Group,
            other => {
                return Err(SyncError::Protocol {
                    reason: format!("unknown SyncClipboard type {other:?}"),
                })
            }
        };
        Ok(ClipboardMeta {
            kind,
            text: self.text,
            data_name: self.data_name,
            has_data: self.has_data,
            size: self.size,
            hash: self.hash,
        })
    }
}

// ─── platform bridge (seam 2, carried over from B1) ─────────────────────

/// Host-side services the native app provides to Rust.
///
/// `with_foreign` (NOT `callback_interface`) is load-bearing: only
/// `with_foreign` traits can appear as `Arc<dyn …>` constructor arguments
/// (uniffi-rs #2797). Snapshot-style contract: natives read bytes BEFORE
/// entering async Rust, so foreign calls never block a tokio worker from
/// inside a future (spike plan §4).
#[uniffi::export(with_foreign)]
pub trait PlatformBridge: Send + Sync {
    /// Absolute path of the app-group container directory (shared between
    /// the iOS app and its keyboard/share extensions).
    fn app_group_dir(&self) -> String;
}

// ─── runtime host ───────────────────────────────────────────────────────

/// Owns the dedicated runtime thread; dropping shuts the runtime down.
struct RuntimeHost {
    handle: tokio::runtime::Handle,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl RuntimeHost {
    fn spawn() -> Result<Self, SyncError> {
        let (handle_tx, handle_rx) = std::sync::mpsc::channel();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("uc-mobile-rt".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = handle_tx.send(Err(e.to_string()));
                        return;
                    }
                };
                if handle_tx.send(Ok(rt.handle().clone())).is_err() {
                    return;
                }
                // Park until shutdown; spawned request tasks run regardless
                // of whether any exported future is still awaited (seam 3).
                let _ = rt.block_on(shutdown_rx);
            })
            .map_err(|e| SyncError::Internal {
                reason: format!("spawn runtime thread: {e}"),
            })?;
        let handle = handle_rx
            .recv()
            .map_err(|_| SyncError::Internal {
                reason: "runtime thread exited before handing back a handle".into(),
            })?
            .map_err(|e| SyncError::Internal {
                reason: format!("build current_thread runtime: {e}"),
            })?;
        Ok(Self {
            handle,
            shutdown: Some(shutdown_tx),
            thread: Some(thread),
        })
    }
}

impl Drop for RuntimeHost {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ─── client ─────────────────────────────────────────────────────────────

/// Async mobile-sync client backed by reqwest(ring rustls) + a dedicated
/// current_thread tokio runtime.
#[derive(uniffi::Object)]
pub struct MobileSyncClient {
    bridge: Arc<dyn PlatformBridge>,
    rt: RuntimeHost,
    http: reqwest::Client,
    in_flight: Mutex<Vec<AbortHandle>>,
}

#[uniffi::export]
impl MobileSyncClient {
    /// Seam-2 probe: a foreign-implemented trait object as constructor input.
    /// Fails with [`SyncError::NotInitialized`] if [`uc_mobile_init`] has not
    /// run in this process.
    #[uniffi::constructor]
    pub fn new(bridge: Arc<dyn PlatformBridge>) -> Result<Arc<Self>, SyncError> {
        ensure_initialized()?;
        let http = reqwest::Client::builder()
            // No idle connection pool: iOS extensions live under a ~48MB
            // jetsam ceiling and requests are sporadic (spike plan §4).
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|e| SyncError::Internal {
                reason: format!("build http client: {e}"),
            })?;
        Ok(Arc::new(Self {
            bridge,
            rt: RuntimeHost::spawn()?,
            http,
            in_flight: Mutex::new(Vec::new()),
        }))
    }

    /// Round-trip probe: Rust calling back into the foreign bridge (B1).
    pub fn bridge_probe(&self) -> String {
        self.bridge.app_group_dir()
    }

    /// `GET /SyncClipboard.json` — latest clipboard metadata (spec §2.1).
    pub async fn get_latest(&self, server: ServerConfig) -> Result<ClipboardMeta, SyncError> {
        let http = self.http.clone();
        self.run(async move {
            let url = endpoint(&server.base_url, &["SyncClipboard.json"])?;
            let resp = http
                .get(url)
                .basic_auth(&server.username, Some(&server.password))
                .send()
                .await
                .map_err(network)?;
            let doc: WireDoc =
                check(resp)
                    .await?
                    .json()
                    .await
                    .map_err(|e| SyncError::Protocol {
                        reason: format!("decode SyncClipboard.json: {e}"),
                    })?;
            doc.into_meta()
        })
        .await
    }

    /// `PUT /SyncClipboard.json`, optionally preceded by
    /// `PUT /file/{dataName}` for the binary payload (spec §2.2/§2.3).
    ///
    /// The file→metadata sequence runs as one detached task on the runtime
    /// thread: dropping this future mid-flight does NOT interrupt the window
    /// (seam 3) — see the module docs.
    pub async fn put_clipboard(
        &self,
        server: ServerConfig,
        meta: ClipboardMeta,
        payload: Option<Vec<u8>>,
    ) -> Result<(), SyncError> {
        let http = self.http.clone();
        self.run(async move {
            if let Some(bytes) = payload {
                let data_name = meta.data_name.as_deref().ok_or(SyncError::InvalidInput {
                    reason: "payload requires meta.data_name".into(),
                })?;
                let url = endpoint(&server.base_url, &["file", data_name])?;
                let resp = http
                    .put(url)
                    .basic_auth(&server.username, Some(&server.password))
                    .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                    .body(bytes)
                    .send()
                    .await
                    .map_err(network)?;
                check(resp).await?;
            }
            let url = endpoint(&server.base_url, &["SyncClipboard.json"])?;
            let resp = http
                .put(url)
                .basic_auth(&server.username, Some(&server.password))
                .json(&WireDoc::from_meta(&meta))
                .send()
                .await
                .map_err(network)?;
            check(resp).await?;
            Ok(())
        })
        .await
    }

    /// B2 TLS acceptance probe: complete one real TLS handshake (HTTPS GET)
    /// and return the status code. Proves the ring provider installed by
    /// [`uc_mobile_init`] actually drives a handshake in this process
    /// context; the response body is discarded.
    pub async fn tls_probe(&self, url: String) -> Result<u16, SyncError> {
        if !url.starts_with("https://") {
            return Err(SyncError::InvalidInput {
                reason: "tls_probe requires an https:// url".into(),
            });
        }
        let http = self.http.clone();
        self.run(async move {
            let resp = http.get(&url).send().await.map_err(network)?;
            Ok(resp.status().as_u16())
        })
        .await
    }

    /// Abort all requests currently running on the runtime thread. Their
    /// awaiting callers observe [`SyncError::Cancelled`].
    pub fn cancel_in_flight(&self) {
        if let Ok(mut handles) = self.in_flight.lock() {
            for h in handles.drain(..) {
                h.abort();
            }
        }
    }
}

impl MobileSyncClient {
    /// Spawn `fut` as a detached task on the runtime thread and await its
    /// JoinHandle (reactor-free, so safe to poll from UniFFI's machinery).
    async fn run<T: Send + 'static>(
        &self,
        fut: impl Future<Output = Result<T, SyncError>> + Send + 'static,
    ) -> Result<T, SyncError> {
        let join = self.rt.handle.spawn(fut);
        if let Ok(mut handles) = self.in_flight.lock() {
            handles.retain(|h| !h.is_finished());
            handles.push(join.abort_handle());
        }
        match join.await {
            Ok(result) => result,
            Err(e) if e.is_cancelled() => Err(SyncError::Cancelled),
            Err(e) => Err(SyncError::Internal {
                reason: format!("request task failed: {e}"),
            }),
        }
    }
}

// ─── helpers ────────────────────────────────────────────────────────────

fn endpoint(base_url: &str, segments: &[&str]) -> Result<url::Url, SyncError> {
    let mut url = url::Url::parse(base_url).map_err(|e| SyncError::InvalidInput {
        reason: format!("invalid base_url: {e}"),
    })?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| SyncError::InvalidInput {
                reason: "base_url cannot be a base".into(),
            })?;
        path.pop_if_empty();
        for s in segments {
            path.push(s);
        }
    }
    Ok(url)
}

fn network(e: reqwest::Error) -> SyncError {
    SyncError::Network {
        reason: e.to_string(),
    }
}

async fn check(resp: reqwest::Response) -> Result<reqwest::Response, SyncError> {
    match resp.status().as_u16() {
        200..=299 => Ok(resp),
        401 => Err(SyncError::Unauthorized),
        status => Err(SyncError::Http { status }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::time::Duration;

    use axum::extract::{Path, State};
    use axum::http::{header, HeaderMap, StatusCode};
    use axum::routing::{get, put};
    use axum::{Json, Router};

    struct NoopBridge;
    impl PlatformBridge for NoopBridge {
        fn app_group_dir(&self) -> String {
            String::new()
        }
    }

    /// Mock daemon: Basic-Auth-checked SyncClipboard endpoints recording the
    /// request sequence, with a configurable delay on the file PUT so tests
    /// can drop/cancel mid-window.
    struct MockState {
        events: Mutex<Vec<String>>,
        expected_auth: String,
        file_delay: Duration,
    }

    impl MockState {
        fn events(&self) -> Vec<String> {
            self.events.lock().expect("mock lock").clone()
        }
        fn record(&self, e: impl Into<String>) {
            self.events.lock().expect("mock lock").push(e.into());
        }
    }

    fn authed(state: &MockState, headers: &HeaderMap) -> bool {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == state.expected_auth)
    }

    async fn mock_get_doc(
        State(state): State<Arc<MockState>>,
        headers: HeaderMap,
    ) -> Result<Json<WireDoc>, StatusCode> {
        if !authed(&state, &headers) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        state.record("get-doc");
        Ok(Json(WireDoc {
            kind: "Text".into(),
            text: "hello from daemon".into(),
            data_name: None,
            has_data: false,
            size: 0,
            hash: Some("aa".into()),
        }))
    }

    async fn mock_put_doc(
        State(state): State<Arc<MockState>>,
        headers: HeaderMap,
        Json(doc): Json<WireDoc>,
    ) -> StatusCode {
        if !authed(&state, &headers) {
            return StatusCode::UNAUTHORIZED;
        }
        state.record(format!("put-doc:{}", doc.kind));
        StatusCode::OK
    }

    async fn mock_put_file(
        State(state): State<Arc<MockState>>,
        Path(name): Path<String>,
        headers: HeaderMap,
        body: axum::body::Bytes,
    ) -> StatusCode {
        if !authed(&state, &headers) {
            return StatusCode::UNAUTHORIZED;
        }
        tokio::time::sleep(state.file_delay).await;
        state.record(format!("put-file:{name}:{}", body.len()));
        StatusCode::OK
    }

    async fn spawn_mock(file_delay: Duration) -> (SocketAddr, Arc<MockState>) {
        use base64::Engine as _;
        let state = Arc::new(MockState {
            events: Mutex::new(Vec::new()),
            expected_auth: format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("u:p")
            ),
            file_delay,
        });
        // axum 0.7: route params use `:name`, not `{name}`.
        let app = Router::new()
            .route("/SyncClipboard.json", get(mock_get_doc).put(mock_put_doc))
            .route("/file/:name", put(mock_put_file))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock");
        let addr = listener.local_addr().expect("mock addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (addr, state)
    }

    fn server_cfg(addr: SocketAddr, password: &str) -> ServerConfig {
        ServerConfig {
            base_url: format!("http://{addr}"),
            username: "u".into(),
            password: password.into(),
        }
    }

    fn new_client() -> Arc<MobileSyncClient> {
        uc_mobile_init();
        MobileSyncClient::new(Arc::new(NoopBridge)).expect("client constructs after init")
    }

    fn file_meta() -> ClipboardMeta {
        ClipboardMeta {
            kind: ClipboardKind::File,
            text: "f.bin".into(),
            data_name: Some("f.bin".into()),
            has_data: true,
            size: 3,
            hash: None,
        }
    }

    #[tokio::test]
    async fn get_latest_decodes_doc() {
        let (addr, _state) = spawn_mock(Duration::ZERO).await;
        let client = new_client();
        let meta = client
            .get_latest(server_cfg(addr, "p"))
            .await
            .expect("get ok");
        assert_eq!(meta.kind, ClipboardKind::Text);
        assert_eq!(meta.text, "hello from daemon");
        assert_eq!(meta.hash.as_deref(), Some("aa"));
    }

    #[tokio::test]
    async fn put_clipboard_sends_file_before_doc() {
        let (addr, state) = spawn_mock(Duration::ZERO).await;
        let client = new_client();
        client
            .put_clipboard(server_cfg(addr, "p"), file_meta(), Some(vec![1, 2, 3]))
            .await
            .expect("put ok");
        assert_eq!(state.events(), vec!["put-file:f.bin:3", "put-doc:File"]);
    }

    #[tokio::test]
    async fn wrong_password_maps_to_unauthorized() {
        let (addr, _state) = spawn_mock(Duration::ZERO).await;
        let client = new_client();
        let err = client
            .get_latest(server_cfg(addr, "wrong"))
            .await
            .expect_err("must 401");
        assert_eq!(err, SyncError::Unauthorized);
    }

    /// Seam 3: dropping the exported future mid file→metadata window must NOT
    /// interrupt the sequence — the detached task finishes both requests.
    #[tokio::test]
    async fn dropped_put_future_still_completes_file_and_doc() {
        let (addr, state) = spawn_mock(Duration::from_millis(150)).await;
        let client = new_client();
        let mut fut =
            Box::pin(client.put_clipboard(server_cfg(addr, "p"), file_meta(), Some(vec![1, 2, 3])));
        // Poll once so the inner task is spawned, then drop the caller-side
        // future while the file PUT is still sleeping inside the mock.
        tokio::select! {
            biased;
            _ = &mut fut => panic!("put must not finish within 20ms"),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
        drop(fut);
        for _ in 0..200 {
            if state.events().len() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            state.events(),
            vec!["put-file:f.bin:3", "put-doc:File"],
            "detached task must complete the full file→metadata window"
        );
    }

    #[tokio::test]
    async fn cancel_in_flight_yields_cancelled() {
        let (addr, _state) = spawn_mock(Duration::from_millis(500)).await;
        let client = new_client();
        let mut fut =
            Box::pin(client.put_clipboard(server_cfg(addr, "p"), file_meta(), Some(vec![1, 2, 3])));
        tokio::select! {
            biased;
            _ = &mut fut => panic!("put must not finish within 20ms"),
            _ = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
        client.cancel_in_flight();
        assert_eq!(fut.await, Err(SyncError::Cancelled));
    }

    #[tokio::test]
    async fn tls_probe_rejects_plain_http() {
        let client = new_client();
        let err = client
            .tls_probe("http://127.0.0.1:1".into())
            .await
            .expect_err("http must be rejected");
        assert!(matches!(err, SyncError::InvalidInput { .. }));
    }

    #[test]
    fn endpoint_joins_paths_without_double_slash() {
        let url = endpoint("http://10.0.0.5:42720/", &["SyncClipboard.json"]).expect("join");
        assert_eq!(url.as_str(), "http://10.0.0.5:42720/SyncClipboard.json");
        let url = endpoint("http://10.0.0.5:42720", &["file", "a b.png"]).expect("join");
        assert_eq!(url.as_str(), "http://10.0.0.5:42720/file/a%20b.png");
    }
}
