//! Async mobile-sync HTTP client over FFI (spike B2 + goal-B M2).
//!
//! Byte-for-byte port of uc-ios `Shared/Network/SyncClipboardClient.swift` and
//! `SyncError.swift` (regression checklist A6). The Swift sources and their
//! tests (`SyncClipboardClientTests.swift`) are the NORMATIVE reference. All
//! pure wire codecs (Clipboard JSON, multipart, hashing, ISO-8601, history
//! records) live in `uc-mobile-proto` and are consumed here; this crate adds
//! only the HTTP transport, status mapping, retry, and cancellation on top.
//!
//! Goal-B M3 adds the connectivity probe (`ConnectionTester.swift`, regression
//! checklist A7): [`MobileSyncClient::test_connection`] (full single-URL test),
//! [`MobileSyncClient::probe`] (concurrent, short-timeout, no-retry,
//! status-only multi-URL probe returning a [`ProbeReport`]), and the pure
//! [`first_reachable`] picker over the §5.3 shape order (`ordered_urls` lives
//! in `uc-mobile-proto`). Reachability semantics there deliberately diverge
//! from the main client: 404 = reachable (see [`ProbeResult`]).
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
//!
//! ## Deliberate divergence from Swift: cancellation does NOT poison
//!
//! Swift's `SyncClipboardClient` is constructed per `ServerConfig` / network
//! context and discarded right after `cancelInFlight()`, so it sets a
//! permanent `isCancelled` flag (subsequent requests throw `.cancelled`). That
//! flag is a workaround for a `URLSession`-specific footgun (creating a task on
//! an invalidated session raises an ObjC exception) and for the
//! per-context-then-discard lifetime.
//!
//! Neither applies here: this client is LONG-LIVED, serves many servers (the
//! target is passed per call, not at construction), and owns the tokio runtime
//! thread — the whole point of B2 is "spin the runtime up once". So
//! [`MobileSyncClient::cancel_in_flight`] aborts in-flight request tasks (their
//! awaiting callers observe [`SyncError::Cancelled`], and the 300ms retry can
//! no longer fire because aborting the task tears down its `sleep`) but does
//! NOT poison the client: a subsequent call — which carries a fresh, possibly
//! new-network-path `ServerConfig` chosen by the native shell — proceeds
//! normally. Permanent poisoning would force the native side to rebuild the
//! client (respawning the runtime thread) on every Wi-Fi/cellular flip.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use chrono::DateTime;
use tokio::task::AbortHandle;

use uc_mobile_proto::{
    Clipboard as ProtoClipboard, ClipboardKind as ProtoKind, HistoryQuery as ProtoHistoryQuery,
    HistoryRecord as ProtoHistoryRecord,
};

/// Idle/connect timeout for production clients. Mirrors Swift
/// `timeoutIntervalForRequest = 10` — an IDLE timer (reqwest `read_timeout`
/// resets on every received byte), so it does NOT cap large transfers; it is
/// the backstop for a blackholed route (LAN IP over cellular) when no
/// network-path change fired to cancel the request explicitly
/// (`SyncClipboardClient.makeSession`).
const REQUEST_IDLE_TIMEOUT: Duration = Duration::from_secs(10);

/// Single 300ms retry delay for `.networkConnectionLost` / `.timedOut`
/// (Swift `perform`: `Task.sleep(nanoseconds: 300_000_000)`).
const RETRY_DELAY: Duration = Duration::from_millis(300);

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

/// Mirror of the SyncClipboard `type` values (`uc_mobile_proto::ClipboardKind`,
/// raw wire strings `Text`/`Image`/`File`/`Group`). A uniffi-native enum so it
/// can cross the FFI boundary; converts to/from the proto kind for wire codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ClipboardKind {
    Text,
    Image,
    File,
    Group,
}

impl From<ProtoKind> for ClipboardKind {
    fn from(k: ProtoKind) -> Self {
        match k {
            ProtoKind::Text => Self::Text,
            ProtoKind::Image => Self::Image,
            ProtoKind::File => Self::File,
            ProtoKind::Group => Self::Group,
        }
    }
}

impl From<ClipboardKind> for ProtoKind {
    fn from(k: ClipboardKind) -> Self {
        match k {
            ClipboardKind::Text => Self::Text,
            ClipboardKind::Image => Self::Image,
            ClipboardKind::File => Self::File,
            ClipboardKind::Group => Self::Group,
        }
    }
}

/// Clipboard metadata as exchanged with `GET/PUT /SyncClipboard.json`. The
/// FFI-native surface; the wire bytes are produced/consumed exclusively by
/// [`uc_mobile_proto::Clipboard`] (single source of truth for the JSON shape),
/// which this maps to/from via [`ClipboardMeta::into_proto`] /
/// [`ClipboardMeta::from_proto`].
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

impl ClipboardMeta {
    /// Map to the canonical wire type for serialization. Routes through
    /// [`ProtoClipboard::new`] so the `hash` empty/whitespace→omitted
    /// normalization (Swift `Clipboard.init`) is applied on upload.
    pub(crate) fn into_proto(self) -> ProtoClipboard {
        ProtoClipboard::new(
            self.kind.into(),
            self.hash,
            self.text,
            self.has_data,
            self.data_name,
            // The FFI surface keeps `size` non-optional; the daemon/Swift
            // upload path always carries a size, so emit it.
            Some(self.size as i64),
        )
    }

    /// Map from a decoded wire document. The proto decoder already normalized
    /// `hash`; a degenerate negative `size` from a buggy peer clamps to 0.
    pub(crate) fn from_proto(c: ProtoClipboard) -> Self {
        Self {
            kind: c.kind.into(),
            text: c.text,
            data_name: c.data_name,
            has_data: c.has_data,
            size: c.size.unwrap_or(0).max(0) as u64,
            hash: c.hash,
        }
    }
}

/// Filter parameters for [`MobileSyncClient::query_history`] (spec §2.7). FFI
/// mirror of [`uc_mobile_proto::HistoryQuery`]; timestamps are Unix epoch
/// milliseconds (Swift-side `Date(timeIntervalSince1970: ms/1000)`), all other
/// semantics — including `None` = "field omitted from the multipart body" —
/// are identical to the proto type.
#[derive(Debug, Clone, Default, PartialEq, Eq, uniffi::Record)]
pub struct HistoryQuery {
    /// 1-indexed page; omit to fetch from the start. An empty result page is
    /// the documented end-of-list signal.
    pub page: Option<i64>,
    /// Strict upper bound on `createTime` (epoch millis).
    pub before_ms: Option<i64>,
    /// Inclusive lower bound on `createTime` (epoch millis).
    pub after_ms: Option<i64>,
    /// STRICT lower bound on `lastModified` (epoch millis) — the
    /// incremental-sync primitive.
    pub modified_after_ms: Option<i64>,
    /// Type bitmask: Text=1, Image=2, File=4, Group=8 (15 = all).
    pub types: Option<i64>,
    /// Server-side substring match against the record's `text`.
    pub search_text: Option<String>,
    pub starred: Option<bool>,
    pub sort_by_last_accessed: Option<bool>,
}

impl HistoryQuery {
    fn into_proto(self) -> ProtoHistoryQuery {
        ProtoHistoryQuery {
            page: self.page,
            before: self.before_ms.and_then(DateTime::from_timestamp_millis),
            after: self.after_ms.and_then(DateTime::from_timestamp_millis),
            modified_after: self
                .modified_after_ms
                .and_then(DateTime::from_timestamp_millis),
            types: self.types,
            search_text: self.search_text,
            starred: self.starred,
            sort_by_last_accessed: self.sort_by_last_accessed,
        }
    }
}

/// A history record returned by [`MobileSyncClient::query_history`] (spec
/// §3.6). FFI mirror of [`uc_mobile_proto::HistoryRecord`] with timestamps as
/// Unix epoch milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct HistoryRecord {
    /// SHA-256 uppercase hex of the content.
    pub hash: String,
    pub kind: ClipboardKind,
    /// Preview text; `None` only when the wire field was absent.
    pub text: Option<String>,
    pub has_data: bool,
    pub size: Option<i64>,
    pub create_time_ms: Option<i64>,
    pub last_modified_ms: Option<i64>,
    pub last_accessed_ms: Option<i64>,
    pub starred: bool,
    pub pinned: bool,
    /// Server-side optimistic-lock version (0 on create).
    pub version: Option<i64>,
    pub is_deleted: bool,
}

impl From<ProtoHistoryRecord> for HistoryRecord {
    fn from(r: ProtoHistoryRecord) -> Self {
        Self {
            hash: r.hash,
            kind: r.kind.into(),
            text: r.text,
            has_data: r.has_data,
            size: r.size,
            create_time_ms: r.create_time.map(|d| d.timestamp_millis()),
            last_modified_ms: r.last_modified.map(|d| d.timestamp_millis()),
            last_accessed_ms: r.last_accessed.map(|d| d.timestamp_millis()),
            starred: r.starred,
            pinned: r.pinned,
            version: r.version,
            is_deleted: r.is_deleted,
        }
    }
}

/// Failure surface of the async client. Mirrors the relevant `SyncError.Kind`
/// cases (`SyncError.swift`): the HTTP-status mapping is byte-for-byte
/// faithful to Swift `mapHTTPStatus`. Swift's finer network kinds
/// (`connectTimeout` / `receiveTimeout` / `networkUnreachable`) collapse into
/// [`SyncError::Network`] — the sync engine treats every non-cancelled
/// network failure identically (backoff), so the distinction is not surfaced.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum SyncError {
    #[error("uc_mobile_init() must be called before constructing a client")]
    NotInitialized,
    /// Swift `.invalidURL`: bad base URL, file name, or profile id (rejected
    /// before any network call).
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    /// Swift `.networkUnreachable` / `.connectTimeout` / `.receiveTimeout`.
    #[error("network: {reason}")]
    Network { reason: String },
    /// Swift `.authFailed` — HTTP 401.
    #[error("unauthorized (401): check username/password")]
    Unauthorized,
    /// Swift `.notFound` — HTTP 404 (also the documented "empty server" GET
    /// state, which callers treat as "absent", not an error).
    #[error("not found (404)")]
    NotFound,
    /// Swift `.serverError(status)` — HTTP 5xx.
    #[error("server error (HTTP {status})")]
    ServerError { status: u16 },
    /// Swift `.protocolError(status)` — any other non-success status (e.g.
    /// other 4xx, 3xx, or a non-{200,201,204} 2xx).
    #[error("protocol error (HTTP {status})")]
    ProtocolError { status: u16 },
    /// Swift `.decodingFailed` — a 2xx body that did not parse as the expected
    /// wire shape.
    #[error("decoding failed: {reason}")]
    DecodingFailed { reason: String },
    /// Swift `.cancelled` — the request was aborted via
    /// [`MobileSyncClient::cancel_in_flight`] (or the caller-side future was
    /// dropped). A deliberate no-op for the sync engine, not a failure.
    #[error("cancelled")]
    Cancelled,
    #[error("internal: {reason}")]
    Internal { reason: String },
}

// ─── connectivity probe (A7 / §5.3 Layer 2) ──────────────────────────────

/// Reachability verdict for one candidate URL. Byte-for-byte port of Swift
/// `ConnectionTester.Result` (`ConnectionTester.swift`).
///
/// §2.1 reachability semantics — DELIBERATELY different from the main client's
/// status mapping: a URL is *reachable* when the server answered at all, so
/// **404 maps to [`ProbeResult::Success`]** ("no clipboard published yet", the
/// server is up and auth is fine) and 401 maps to [`ProbeResult::AuthFailed`]
/// (reachable, credentials wrong). Bad credentials are an account problem, not
/// a path problem; the URL picker must not skip a perfectly good direct path
/// because the password is stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum ProbeResult {
    /// 2xx success or 404 — the server answered, the path works.
    Success,
    /// 401 — reachable, but the credentials were rejected.
    AuthFailed,
    /// No HTTP answer (connect refused, timeout, TLS failure, malformed URL) or
    /// a 5xx / other non-success status.
    Unreachable,
    /// A required input (URL / username / password) was empty — no request made.
    MissingFields,
}

impl ProbeResult {
    /// §5.3: a candidate is reachable when the server answered at all
    /// ([`Self::Success`] or [`Self::AuthFailed`]). Single source of truth for
    /// the picker; mirrors Swift `Result.isReachable`.
    fn is_reachable(&self) -> bool {
        matches!(self, ProbeResult::Success | ProbeResult::AuthFailed)
    }
}

/// Outcome of a multi-URL [`MobileSyncClient::probe`]: the per-URL verdicts
/// stamped with the network epoch they were captured under.
///
/// The epoch is opaque to this crate (a monotonic counter the native shell
/// bumps on every network-path change). It is carried through verbatim so a
/// later consumer can discard a stale snapshot — "a probe conclusion is only
/// valid while the epoch has not changed" (§5.3). The epoch *check* itself
/// belongs to the sync engine (goal-B M5), which does not exist yet; M3 only
/// stamps the snapshot.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ProbeReport {
    /// The `network_epoch` passed to [`MobileSyncClient::probe`], echoed back.
    pub network_epoch: u64,
    /// One verdict per *distinct* URL string probed (Swift `[String: Result]`).
    pub results: HashMap<String, ProbeResult>,
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

/// Timeout policy for the underlying reqwest client. Production uses an idle
/// (read) + connect timeout mirroring Swift's `timeoutIntervalForRequest`;
/// tests override with a short TOTAL timeout to exercise the retry-on-timeout
/// path deterministically.
#[derive(Debug, Clone, Copy)]
struct HttpTimeouts {
    connect: Option<Duration>,
    read: Option<Duration>,
    total: Option<Duration>,
}

impl HttpTimeouts {
    fn production() -> Self {
        Self {
            connect: Some(REQUEST_IDLE_TIMEOUT),
            read: Some(REQUEST_IDLE_TIMEOUT),
            total: None,
        }
    }
}

fn build_http_client(
    t: HttpTimeouts,
    trust_insecure_cert: bool,
) -> reqwest::Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        // No idle connection pool: iOS extensions live under a ~48MB jetsam
        // ceiling and requests are sporadic (spike plan §4). Also makes each
        // request a fresh connection, so a per-connection reset surfaces
        // cleanly to the retry path.
        .pool_max_idle_per_host(0);
    if trust_insecure_cert {
        // Swift `TrustingDelegate` / `makeProbeSession(trustInsecureCert:)`:
        // skip certificate validation for self-signed-cert NAS hosts. Threaded
        // in by both the ConnectionTester probe/test paths (regression checklist
        // A7) and — since M4 (E区) — the long-lived PRODUCTION client, which
        // honors the `trustInsecureCert` app setting via `construct` /
        // `set_trust_insecure_cert`.
        builder = builder.danger_accept_invalid_certs(true);
    }
    if let Some(c) = t.connect {
        builder = builder.connect_timeout(c);
    }
    if let Some(r) = t.read {
        builder = builder.read_timeout(r);
    }
    if let Some(total) = t.total {
        builder = builder.timeout(total);
    }
    builder.build()
}

/// Async mobile-sync client backed by reqwest(ring rustls) + a dedicated
/// current_thread tokio runtime.
#[derive(uniffi::Object)]
pub struct MobileSyncClient {
    bridge: Arc<dyn PlatformBridge>,
    rt: RuntimeHost,
    /// The long-lived production client. Interior-mutable so the
    /// settings-gated `trustInsecureCert` toggle can swap it
    /// ([`Self::set_trust_insecure_cert`]) without restarting the runtime
    /// thread — cheap, since `reqwest::Client` is `Arc`-backed.
    http: RwLock<reqwest::Client>,
    in_flight: Mutex<Vec<AbortHandle>>,
    /// Monotonic source of deterministic, per-request multipart boundaries
    /// (the pure proto crate never generates randomness).
    boundary_seq: AtomicU64,
}

#[uniffi::export]
impl MobileSyncClient {
    /// Seam-2 probe: a foreign-implemented trait object as constructor input.
    /// Fails with [`SyncError::NotInitialized`] if [`uc_mobile_init`] has not
    /// run in this process.
    ///
    /// `trust_insecure_cert` fixes the production client's TLS-validation policy
    /// at construction (the `trustInsecureCert` app setting, regression checklist
    /// E区). The native layer rebuilds via [`Self::set_trust_insecure_cert`] when
    /// the user toggles it — clients are long-lived per server, so a per-call
    /// flag would force needless rebuilds.
    #[uniffi::constructor]
    pub fn new(
        bridge: Arc<dyn PlatformBridge>,
        trust_insecure_cert: bool,
    ) -> Result<Arc<Self>, SyncError> {
        Self::construct(bridge, HttpTimeouts::production(), trust_insecure_cert)
    }

    /// Swap the production client's TLS-validation policy in place (the user
    /// toggled `trustInsecureCert`). Rebuilds only the reqwest client — the
    /// runtime thread, in-flight tracking, and boundary counter are untouched.
    pub fn set_trust_insecure_cert(&self, trust_insecure_cert: bool) -> Result<(), SyncError> {
        let client =
            build_http_client(HttpTimeouts::production(), trust_insecure_cert).map_err(|e| {
                SyncError::Internal {
                    reason: format!("rebuild http client: {e}"),
                }
            })?;
        // The lock guards only a clone/replace (no panics inside), so poisoning
        // is impossible in practice; recover the inner value if it ever happens
        // rather than unwrap-panicking.
        match self.http.write() {
            Ok(mut guard) => *guard = client,
            Err(poisoned) => *poisoned.into_inner() = client,
        }
        Ok(())
    }

    /// Round-trip probe: Rust calling back into the foreign bridge (B1).
    pub fn bridge_probe(&self) -> String {
        self.bridge.app_group_dir()
    }

    /// `GET /SyncClipboard.json` — latest clipboard metadata (spec §2.1).
    pub async fn get_latest(&self, server: ServerConfig) -> Result<ClipboardMeta, SyncError> {
        let http = self.http();
        self.run(async move { get_latest_with(&http, &server).await })
            .await
    }

    /// `PUT /SyncClipboard.json`, optionally preceded by
    /// `PUT /file/{dataName}` for the binary payload (spec §2.2/§2.3/§3.5).
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
        // Validate the payload's file name before spawning any work, matching
        // Swift's "reject bad names before any network call".
        if payload.is_some() {
            let data_name = meta.data_name.as_deref().ok_or(SyncError::InvalidInput {
                reason: "payload requires meta.data_name".into(),
            })?;
            validate_path_component(data_name, "filename")?;
        }
        let http = self.http();
        self.run(async move {
            if let Some(bytes) = payload {
                // Unwrap-free: presence + validity checked above.
                if let Some(data_name) = meta.data_name.clone() {
                    put_file_inner(&http, &server, &data_name, bytes).await?;
                }
            }
            let url = endpoint(&server.base_url, &["SyncClipboard.json"])?;
            let req = http
                .put(url)
                .basic_auth(&server.username, Some(&server.password))
                .json(&meta.into_proto());
            check(send_with_retry(req).await?).await?;
            Ok(())
        })
        .await
    }

    /// `PUT /file/{name}` — upload payload bytes (spec §2.3). Rejects names
    /// containing `/`, `\`, or empty before any network call.
    pub async fn put_file(
        &self,
        server: ServerConfig,
        name: String,
        body: Vec<u8>,
    ) -> Result<(), SyncError> {
        validate_path_component(&name, "filename")?;
        let http = self.http();
        self.run(async move { put_file_inner(&http, &server, &name, body).await })
            .await
    }

    /// `GET /file/{name}` — download payload bytes (spec §2.4). Same filename
    /// guard as [`Self::put_file`]; 404 surfaces as [`SyncError::NotFound`].
    pub async fn get_file(&self, server: ServerConfig, name: String) -> Result<Vec<u8>, SyncError> {
        validate_path_component(&name, "filename")?;
        let http = self.http();
        self.run(async move {
            let url = endpoint(&server.base_url, &["file", &name])?;
            let req = http
                .get(url)
                .basic_auth(&server.username, Some(&server.password));
            let resp = check(send_with_retry(req).await?).await?;
            Ok(resp.bytes().await.map_err(network)?.to_vec())
        })
        .await
    }

    /// `POST /api/history/query` — paginated history listing (spec §2.7).
    /// Filters are sent as `multipart/form-data`. An empty array is the
    /// documented end-of-list signal, NOT an error.
    pub async fn query_history(
        &self,
        server: ServerConfig,
        query: HistoryQuery,
    ) -> Result<Vec<HistoryRecord>, SyncError> {
        let http = self.http();
        let boundary = self.next_boundary();
        self.run(async move {
            let multipart = query.into_proto().multipart_encoded(&boundary);
            let content_type = multipart.content_type();
            let body = multipart.encoded();
            let url = endpoint(&server.base_url, &["api", "history", "query"])?;
            let req = http
                .post(url)
                .basic_auth(&server.username, Some(&server.password))
                .header(reqwest::header::CONTENT_TYPE, content_type)
                .body(body);
            let resp = check(send_with_retry(req).await?).await?;
            let records: Vec<ProtoHistoryRecord> =
                resp.json().await.map_err(decoding("history query"))?;
            Ok(records.into_iter().map(HistoryRecord::from).collect())
        })
        .await
    }

    /// `GET /api/history/{profileId}/data` — download a history record's
    /// payload bytes (spec §2.11). `profile_id` is the composite
    /// `<type>-<hash>` form ([`uc_mobile_proto::composite_profile_id`]);
    /// rejected before any network call if empty or containing `/` / `\`.
    pub async fn get_history_payload(
        &self,
        server: ServerConfig,
        profile_id: String,
    ) -> Result<Vec<u8>, SyncError> {
        validate_path_component(&profile_id, "profileId")?;
        let http = self.http();
        self.run(async move {
            let url = endpoint(&server.base_url, &["api", "history", &profile_id, "data"])?;
            let req = http
                .get(url)
                .basic_auth(&server.username, Some(&server.password));
            let resp = check(send_with_retry(req).await?).await?;
            Ok(resp.bytes().await.map_err(network)?.to_vec())
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
        let http = self.http();
        self.run(async move {
            let resp = http.get(&url).send().await.map_err(network)?;
            Ok(resp.status().as_u16())
        })
        .await
    }

    /// "测试连接" — probe ONE server's reachability + credentials via a full
    /// `GET /SyncClipboard.json` (spec §5.3 Layer 2 single-URL form; Swift
    /// `ConnectionTester.test`). Uses the production retry/timeout policy
    /// (Swift builds a fresh full client per test), then folds every outcome
    /// into a [`ProbeResult`] — including a 2xx body that fails to decode,
    /// which maps to [`ProbeResult::Unreachable`].
    ///
    /// `trust_insecure_cert` is honored here (a per-call client is built when
    /// set); the default-false path reuses the validating production client.
    pub async fn test_connection(
        &self,
        server: ServerConfig,
        trust_insecure_cert: bool,
    ) -> ProbeResult {
        // Swift: empty url/username/password short-circuits to .missingFields
        // before any client is constructed.
        if server.base_url.trim().is_empty()
            || server.username.is_empty()
            || server.password.is_empty()
        {
            return ProbeResult::MissingFields;
        }
        // trust=false reuses the validating production client; trust=true builds
        // a fresh danger-accept client with the same production timeouts (Swift
        // constructs a new SyncClipboardClient per test() carrying the flag —
        // a constructor failure there maps to .unreachable).
        let http = if trust_insecure_cert {
            match build_http_client(HttpTimeouts::production(), true) {
                Ok(c) => c,
                Err(_) => return ProbeResult::Unreachable,
            }
        } else {
            self.http()
        };
        // The inner future never returns Err (test_outcome absorbs every
        // SyncError); a task-level Cancelled/Internal still maps to Unreachable,
        // matching Swift's `default` catch arm.
        self.run(async move { Ok(test_outcome(get_latest_with(&http, &server).await)) })
            .await
            .unwrap_or(ProbeResult::Unreachable)
    }

    /// Probe every distinct candidate URL of a profile concurrently and report
    /// per-URL reachability (spec §5.3; Swift `ConnectionTester.probe`).
    ///
    /// Deliberately different from [`Self::test_connection`]:
    /// - SHORT total timeout (`timeout_ms`, native passes 2000) — "is this path
    ///   up *right now*"; a LAN IP on cellular must fail fast.
    /// - NO retry, NO body decode — `GET SyncClipboard.json`'s status code alone
    ///   carries the signal (404 = reachable-but-empty; 401 = reachable, creds
    ///   wrong). reqwest never waits for connectivity, so "no route right now"
    ///   surfaces immediately as [`ProbeResult::Unreachable`] (Swift
    ///   `waitsForConnectivity = false`).
    ///
    /// `network_epoch` is stamped onto the returned [`ProbeReport`] verbatim
    /// (see its docs). Returns one verdict per *distinct* URL string.
    pub async fn probe(
        &self,
        urls: Vec<String>,
        username: String,
        password: String,
        trust_insecure_cert: bool,
        timeout_ms: u32,
        network_epoch: u64,
    ) -> ProbeReport {
        let report = |results| ProbeReport {
            network_epoch,
            results,
        };
        let distinct = dedup_preserving_order(urls);
        if distinct.is_empty() {
            return report(HashMap::new());
        }
        // Swift: empty credentials → every candidate .missingFields, no network.
        if username.is_empty() || password.is_empty() {
            return report(uniform_results(&distinct, ProbeResult::MissingFields));
        }
        let timeouts = HttpTimeouts {
            connect: None,
            read: None,
            total: Some(Duration::from_millis(timeout_ms as u64)),
        };
        let http = match build_http_client(timeouts, trust_insecure_cert) {
            Ok(c) => c,
            Err(_) => return report(uniform_results(&distinct, ProbeResult::Unreachable)),
        };
        let results = self
            .run(async move {
                // Concurrent fan-out on the single runtime thread: each request
                // yields at its await points, so the candidates interleave and
                // the slowest one bounds the wall-clock (not their sum). The
                // JoinSet is dropped if the parent task is aborted, which aborts
                // every child — cancellation propagates without per-child
                // bookkeeping.
                let mut set = tokio::task::JoinSet::new();
                for url in distinct {
                    let http = http.clone();
                    let username = username.clone();
                    let password = password.clone();
                    set.spawn(async move {
                        let verdict = probe_one(&http, &url, &username, &password).await;
                        (url, verdict)
                    });
                }
                let mut out: HashMap<String, ProbeResult> = HashMap::new();
                while let Some(joined) = set.join_next().await {
                    if let Ok((url, verdict)) = joined {
                        out.insert(url, verdict);
                    }
                }
                Ok(out)
            })
            .await
            .unwrap_or_default();
        report(results)
    }

    /// Abort all requests currently running on the runtime thread. Their
    /// awaiting callers observe [`SyncError::Cancelled`]. Does NOT poison the
    /// client — subsequent calls proceed normally (see the module docs).
    pub fn cancel_in_flight(&self) {
        if let Ok(mut handles) = self.in_flight.lock() {
            for h in handles.drain(..) {
                h.abort();
            }
        }
    }
}

impl MobileSyncClient {
    /// Shared construction path for the exported [`Self::new`] (production
    /// timeouts) and tests (short total timeout). Enforces seam 1.
    ///
    /// `trust_insecure_cert` fixes the long-lived production client's TLS policy
    /// (settings-driven, E区); it can later be swapped via
    /// [`Self::set_trust_insecure_cert`]. The per-call ConnectionTester
    /// probe/test clients still build their own trust override independently.
    fn construct(
        bridge: Arc<dyn PlatformBridge>,
        timeouts: HttpTimeouts,
        trust_insecure_cert: bool,
    ) -> Result<Arc<Self>, SyncError> {
        ensure_initialized()?;
        let http =
            build_http_client(timeouts, trust_insecure_cert).map_err(|e| SyncError::Internal {
                reason: format!("build http client: {e}"),
            })?;
        Ok(Arc::new(Self {
            bridge,
            rt: RuntimeHost::spawn()?,
            http: RwLock::new(http),
            in_flight: Mutex::new(Vec::new()),
            boundary_seq: AtomicU64::new(0),
        }))
    }

    /// Clone the current production client. `reqwest::Client` is `Arc`-backed,
    /// so this is cheap and lets each request use a stable snapshot even if a
    /// concurrent [`Self::set_trust_insecure_cert`] swaps the client mid-flight.
    fn http(&self) -> reqwest::Client {
        match self.http.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Next deterministic multipart boundary. A monotonic counter (not a
    /// random UUID like Swift) — for non-adversarial inputs the collision risk
    /// with body content is the same as Swift's, and the failure mode is a
    /// server-rejected request, never a security issue.
    fn next_boundary(&self) -> String {
        let n = self.boundary_seq.fetch_add(1, Ordering::Relaxed);
        format!("UCB-uc-mobile-{n:020}")
    }

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

/// Normalize a base URL (spec §1.1, Swift `normalizeBaseURL`): trim
/// whitespace, reject empty, append a trailing slash if missing, require an
/// `http`/`https` scheme and a non-empty host. The trailing slash makes
/// subsequent path-segment joins (`endpoint`) behave like Swift's
/// `appendingPathComponent`.
fn normalize_base_url(raw: &str) -> Result<url::Url, SyncError> {
    let invalid = |reason: String| SyncError::InvalidInput { reason };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid("base url is empty".into()));
    }
    let with_slash = if trimmed.ends_with('/') {
        trimmed.to_owned()
    } else {
        format!("{trimmed}/")
    };
    let url =
        url::Url::parse(&with_slash).map_err(|e| invalid(format!("invalid base url: {e}")))?;
    // `url` lowercases the scheme during parsing.
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid(format!(
            "base url scheme must be http(s), got {:?}",
            url.scheme()
        )));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(invalid("base url has no host".into()));
    }
    Ok(url)
}

/// Build an endpoint URL by normalizing `base_url` and appending each segment
/// as a single, percent-encoded path component.
fn endpoint(base_url: &str, segments: &[&str]) -> Result<url::Url, SyncError> {
    let mut url = normalize_base_url(base_url)?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| SyncError::InvalidInput {
                reason: "base_url cannot be a base".into(),
            })?;
        // Drop the trailing empty segment from the normalized trailing slash
        // (and from any base path), then append the endpoint components.
        path.pop_if_empty();
        for s in segments {
            path.push(s);
        }
    }
    Ok(url)
}

/// Reject a value used as a single URL path component before any network call
/// (Swift filename / profileId guards): empty, or containing `/` or `\`.
fn validate_path_component(value: &str, what: &str) -> Result<(), SyncError> {
    if value.is_empty() || value.contains('/') || value.contains('\\') {
        return Err(SyncError::InvalidInput {
            reason: format!("invalid {what}: {value:?}"),
        });
    }
    Ok(())
}

/// Map an HTTP status to a [`SyncError`], or `None` for success. BYTE-FAITHFUL
/// to Swift `SyncError.mapHTTPStatus`: success is EXACTLY `{200, 201, 204}` —
/// any other 2xx (202, 206, …), every 3xx, and every non-401/404 4xx fall
/// through to [`SyncError::ProtocolError`]; 5xx is [`SyncError::ServerError`].
fn map_status(status: u16) -> Option<SyncError> {
    match status {
        200 | 201 | 204 => None,
        401 => Some(SyncError::Unauthorized),
        404 => Some(SyncError::NotFound),
        500..=599 => Some(SyncError::ServerError { status }),
        status => Some(SyncError::ProtocolError { status }),
    }
}

fn network(e: reqwest::Error) -> SyncError {
    SyncError::Network {
        reason: e.to_string(),
    }
}

/// Build a [`SyncError::DecodingFailed`] mapper for a labeled response body.
fn decoding(what: &'static str) -> impl Fn(reqwest::Error) -> SyncError {
    move |e| SyncError::DecodingFailed {
        reason: format!("decode {what}: {e}"),
    }
}

async fn check(resp: reqwest::Response) -> Result<reqwest::Response, SyncError> {
    match map_status(resp.status().as_u16()) {
        None => Ok(resp),
        Some(err) => Err(err),
    }
}

/// Whether a reqwest send error is the retriable class Swift retries once
/// after 300ms: `.timedOut` (any reqwest timeout) or `.networkConnectionLost`
/// (a connection reset/abort/EOF mid-flight, surfaced as a transport-level
/// `io::Error` somewhere in the source chain). Connect-refused / DNS failures
/// are NOT retriable — they map to Swift `.networkUnreachable`, which Swift
/// does not retry.
fn is_retriable(e: &reqwest::Error) -> bool {
    if e.is_timeout() {
        return true;
    }
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(e);
    while let Some(err) = source {
        if let Some(io) = err.downcast_ref::<std::io::Error>() {
            use std::io::ErrorKind::{
                BrokenPipe, ConnectionAborted, ConnectionReset, NotConnected, UnexpectedEof,
            };
            if matches!(
                io.kind(),
                ConnectionReset | ConnectionAborted | BrokenPipe | UnexpectedEof | NotConnected
            ) {
                return true;
            }
        }
        source = err.source();
    }
    false
}

/// Send `req`, retrying ONCE after [`RETRY_DELAY`] iff the first attempt failed
/// with a [retriable](is_retriable) transport error (Swift `perform`'s
/// 300ms-once workaround for stuck `NWConnection` paths). The second attempt's
/// result is returned verbatim — no further retry, matching Swift's
/// `attempt == 1` guard. Cancellation is handled one level up by aborting the
/// whole task, so a cancelled request never reaches the retry.
async fn send_with_retry(req: reqwest::RequestBuilder) -> Result<reqwest::Response, SyncError> {
    // All bodies here are in-memory (json / Vec<u8> / multipart), so
    // `try_clone` always yields `Some`; the `None` arm is a safe fallback.
    let retry = req.try_clone();
    match req.send().await {
        Ok(resp) => Ok(resp),
        Err(e) if is_retriable(&e) => match retry {
            Some(retry_req) => {
                tokio::time::sleep(RETRY_DELAY).await;
                retry_req.send().await.map_err(network)
            }
            None => Err(network(e)),
        },
        Err(e) => Err(network(e)),
    }
}

/// `PUT /file/{name}` with octet-stream body (spec §2.3). Shared by
/// [`MobileSyncClient::put_file`] and the file step of
/// [`MobileSyncClient::put_clipboard`]. Assumes `name` is already validated.
async fn put_file_inner(
    http: &reqwest::Client,
    server: &ServerConfig,
    name: &str,
    body: Vec<u8>,
) -> Result<(), SyncError> {
    let url = endpoint(&server.base_url, &["file", name])?;
    let req = http
        .put(url)
        .basic_auth(&server.username, Some(&server.password))
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(body);
    check(send_with_retry(req).await?).await?;
    Ok(())
}

/// `GET /SyncClipboard.json` against an explicit client. Shared by
/// [`MobileSyncClient::get_latest`] (production client) and
/// [`MobileSyncClient::test_connection`] (which may build a trust-override
/// client). Includes the 300ms retry and the JSON decode.
async fn get_latest_with(
    http: &reqwest::Client,
    server: &ServerConfig,
) -> Result<ClipboardMeta, SyncError> {
    let url = endpoint(&server.base_url, &["SyncClipboard.json"])?;
    let req = http
        .get(url)
        .basic_auth(&server.username, Some(&server.password));
    let resp = check(send_with_retry(req).await?).await?;
    let clip: ProtoClipboard = resp.json().await.map_err(decoding("SyncClipboard.json"))?;
    Ok(ClipboardMeta::from_proto(clip))
}

/// Fold a single-URL `GET` outcome into a [`ProbeResult`] (Swift
/// `ConnectionTester.test`'s catch arms): success/404 → reachable, 401 →
/// auth-failed, everything else (5xx, protocol, decode failure, network) →
/// unreachable.
fn test_outcome(result: Result<ClipboardMeta, SyncError>) -> ProbeResult {
    match result {
        Ok(_) => ProbeResult::Success,
        Err(SyncError::NotFound) => ProbeResult::Success,
        Err(SyncError::Unauthorized) => ProbeResult::AuthFailed,
        Err(_) => ProbeResult::Unreachable,
    }
}

/// One probe candidate: status-only `GET <base>/SyncClipboard.json` with Basic
/// Auth, no retry (Swift `ConnectionTester.probeOne`). A blank URL is
/// [`ProbeResult::MissingFields`]; a URL that fails to normalize, any send
/// error, or a 5xx/other status is [`ProbeResult::Unreachable`].
async fn probe_one(
    http: &reqwest::Client,
    url: &str,
    username: &str,
    password: &str,
) -> ProbeResult {
    if url.trim().is_empty() {
        return ProbeResult::MissingFields;
    }
    let Ok(endpoint_url) = endpoint(url, &["SyncClipboard.json"]) else {
        return ProbeResult::Unreachable;
    };
    let req = http.get(endpoint_url).basic_auth(username, Some(password));
    match req.send().await {
        Err(_) => ProbeResult::Unreachable,
        Ok(resp) => match map_status(resp.status().as_u16()) {
            None => ProbeResult::Success,
            Some(SyncError::NotFound) => ProbeResult::Success,
            Some(SyncError::Unauthorized) => ProbeResult::AuthFailed,
            Some(_) => ProbeResult::Unreachable,
        },
    }
}

/// De-duplicate candidate URLs (Swift `Array(Set(urls))`) keeping first-seen
/// order. Order does not affect the result map but keeps the probe set stable.
fn dedup_preserving_order(urls: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    urls.into_iter()
        .filter(|u| seen.insert(u.clone()))
        .collect()
}

/// Map every URL to one verdict (the empty-credentials / build-failure shapes).
fn uniform_results(urls: &[String], verdict: ProbeResult) -> HashMap<String, ProbeResult> {
    urls.iter().map(|u| (u.clone(), verdict)).collect()
}

/// The §5.3 pick: the first URL in `ordered_urls` (shape order for the current
/// network) whose probe came back reachable, or `None` when none is. PURE and
/// deterministic given `results` — NOT a race; two reachable candidates resolve
/// to whichever ranks earlier. A URL with no entry in `results` (filtered out
/// upstream) is never picked. Port of Swift `ConnectionTester.firstReachable`.
///
/// `results` is the [`ProbeReport::results`] map; the caller is responsible for
/// discarding a report whose `network_epoch` is stale before calling this.
#[uniffi::export]
pub fn first_reachable(
    ordered_urls: Vec<String>,
    results: HashMap<String, ProbeResult>,
) -> Option<String> {
    ordered_urls
        .into_iter()
        .find(|u| results.get(u).is_some_and(ProbeResult::is_reachable))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::sync::atomic::AtomicU32;
    use std::time::Duration;

    use axum::body::Bytes;
    use axum::extract::{Path, State};
    use axum::http::{header, HeaderMap, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post, put};
    use axum::{Json, Router};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct NoopBridge;
    impl PlatformBridge for NoopBridge {
        fn app_group_dir(&self) -> String {
            String::new()
        }
    }

    // ── configurable mock daemon ──────────────────────────────────────────

    /// Knobs for [`spawn_mock`]. Defaults give a healthy daemon: a Text
    /// `SyncClipboard.json`, empty history, empty file bytes.
    #[derive(Default)]
    struct MockConfig {
        /// When set, EVERY (authed) route returns this status with an empty
        /// body — drives the status-mapping tests.
        forced_status: Option<u16>,
        /// Delay applied ONLY to the first `GET /SyncClipboard.json` attempt
        /// (the rest are immediate) — drives the timeout-retry test.
        first_get_delay: Duration,
        /// Delay on `PUT /file/{name}` — drives the drop/cancel-window tests.
        file_delay: Duration,
        /// Body for `GET /SyncClipboard.json`.
        clip: Option<ProtoClipboard>,
        /// Body for `POST /api/history/query`.
        history: Vec<ProtoHistoryRecord>,
        /// Body for `GET /file/{name}` and `GET /api/history/{id}/data`.
        file_bytes: Vec<u8>,
    }

    /// Mock daemon state: Basic-Auth-checked SyncClipboard endpoints recording
    /// the request sequence, captured auth header, and the query body so tests
    /// can assert request wiring without re-checking proto's byte-exactness.
    struct MockState {
        events: Mutex<Vec<String>>,
        expected_auth: String,
        cfg: MockConfig,
        get_attempts: AtomicU32,
        last_auth: Mutex<Option<String>>,
        last_query_body: Mutex<Option<Vec<u8>>>,
        last_query_content_type: Mutex<Option<String>>,
    }

    impl MockState {
        fn events(&self) -> Vec<String> {
            self.events.lock().expect("mock lock").clone()
        }
        fn record(&self, e: impl Into<String>) {
            self.events.lock().expect("mock lock").push(e.into());
        }
        fn get_attempts(&self) -> u32 {
            self.get_attempts.load(Ordering::Relaxed)
        }
    }

    fn default_clip() -> ProtoClipboard {
        ProtoClipboard::new(
            ProtoKind::Text,
            Some("AA".into()),
            "hello from daemon".into(),
            false,
            None,
            Some(0),
        )
    }

    /// Capture + verify the Authorization header.
    fn authed(state: &MockState, headers: &HeaderMap) -> bool {
        let value = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        *state.last_auth.lock().expect("mock lock") = value.clone();
        value.as_deref() == Some(state.expected_auth.as_str())
    }

    /// `None` if the route should proceed, else the forced status response.
    fn gate(state: &MockState, headers: &HeaderMap) -> Option<Response> {
        if !authed(state, headers) {
            return Some(StatusCode::UNAUTHORIZED.into_response());
        }
        state.cfg.forced_status.map(|s| {
            StatusCode::from_u16(s)
                .expect("valid status")
                .into_response()
        })
    }

    async fn mock_get_doc(State(state): State<Arc<MockState>>, headers: HeaderMap) -> Response {
        let attempt = state.get_attempts.fetch_add(1, Ordering::Relaxed);
        state.record("get-doc");
        if let Some(resp) = gate(&state, &headers) {
            return resp;
        }
        if attempt == 0 && !state.cfg.first_get_delay.is_zero() {
            tokio::time::sleep(state.cfg.first_get_delay).await;
        }
        Json(state.cfg.clip.clone().unwrap_or_else(default_clip)).into_response()
    }

    async fn mock_put_doc(
        State(state): State<Arc<MockState>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        if let Some(resp) = gate(&state, &headers) {
            return resp;
        }
        let doc: ProtoClipboard = serde_json::from_slice(&body).expect("valid clipboard json");
        state.record(format!("put-doc:{}", doc.kind.as_wire_str()));
        StatusCode::OK.into_response()
    }

    async fn mock_put_file(
        State(state): State<Arc<MockState>>,
        Path(name): Path<String>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        if let Some(resp) = gate(&state, &headers) {
            return resp;
        }
        tokio::time::sleep(state.cfg.file_delay).await;
        state.record(format!("put-file:{name}:{}", body.len()));
        StatusCode::OK.into_response()
    }

    async fn mock_get_file(
        State(state): State<Arc<MockState>>,
        Path(name): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        if let Some(resp) = gate(&state, &headers) {
            return resp;
        }
        state.record(format!("get-file:{name}"));
        state.cfg.file_bytes.clone().into_response()
    }

    async fn mock_query(
        State(state): State<Arc<MockState>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let content_type = headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        *state.last_query_content_type.lock().expect("mock lock") = content_type;
        *state.last_query_body.lock().expect("mock lock") = Some(body.to_vec());
        if let Some(resp) = gate(&state, &headers) {
            return resp;
        }
        state.record("query");
        Json(state.cfg.history.clone()).into_response()
    }

    async fn mock_history_data(
        State(state): State<Arc<MockState>>,
        Path(profile_id): Path<String>,
        headers: HeaderMap,
    ) -> Response {
        if let Some(resp) = gate(&state, &headers) {
            return resp;
        }
        state.record(format!("history-data:{profile_id}"));
        state.cfg.file_bytes.clone().into_response()
    }

    async fn spawn_mock(cfg: MockConfig) -> (SocketAddr, Arc<MockState>) {
        use base64::Engine as _;
        let state = Arc::new(MockState {
            events: Mutex::new(Vec::new()),
            expected_auth: format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("u:p")
            ),
            cfg,
            get_attempts: AtomicU32::new(0),
            last_auth: Mutex::new(None),
            last_query_body: Mutex::new(None),
            last_query_content_type: Mutex::new(None),
        });
        // axum 0.7: route params use `:name`, not `{name}`.
        let app = Router::new()
            .route("/SyncClipboard.json", get(mock_get_doc).put(mock_put_doc))
            .route("/file/:name", put(mock_put_file).get(mock_get_file))
            .route("/api/history/query", post(mock_query))
            .route("/api/history/:profile_id/data", get(mock_history_data))
            .with_state(state.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let addr = listener.local_addr().expect("mock addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (addr, state)
    }

    /// Minimal mock that always answers the doc GET with a fixed status + body
    /// (no auth check) — for the malformed-JSON decode test.
    async fn spawn_raw_doc_mock(status: u16, body: &'static str) -> SocketAddr {
        let app = Router::new().route(
            "/SyncClipboard.json",
            get(move || async move { (StatusCode::from_u16(status).expect("valid"), body) }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let addr = listener.local_addr().expect("mock addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        addr
    }

    /// Raw TCP mock: the FIRST connection is reset (RST via SO_LINGER 0) after
    /// reading the request; every later connection gets a minimal HTTP/1.1 200
    /// carrying `ok_body`. Exercises the `.networkConnectionLost` retry branch.
    async fn spawn_reset_then_ok_mock(ok_body: String) -> (SocketAddr, Arc<AtomicU32>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let addr = listener.local_addr().expect("mock addr");
        let conns = Arc::new(AtomicU32::new(0));
        let counter = conns.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    break;
                };
                let n = counter.fetch_add(1, Ordering::Relaxed);
                let body = ok_body.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let _ = sock.read(&mut buf).await;
                    if n == 0 {
                        // Force a RST on close so reqwest sees ConnectionReset.
                        // linger-ZERO is the immediate-reset case, not the
                        // block-on-drop one the deprecation warns about.
                        #[allow(deprecated)]
                        let _ = sock.set_linger(Some(Duration::ZERO));
                        drop(sock);
                    } else {
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = sock.write_all(resp.as_bytes()).await;
                        let _ = sock.shutdown().await;
                    }
                });
            }
        });
        (addr, conns)
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
        MobileSyncClient::new(Arc::new(NoopBridge), false).expect("client constructs after init")
    }

    /// Test-only client whose reqwest layer has a short TOTAL timeout so the
    /// retry-on-timeout path fires deterministically without a 10s wait.
    fn new_client_total_timeout(ms: u64) -> Arc<MobileSyncClient> {
        uc_mobile_init();
        MobileSyncClient::construct(
            Arc::new(NoopBridge),
            HttpTimeouts {
                connect: None,
                read: None,
                total: Some(Duration::from_millis(ms)),
            },
            false,
        )
        .expect("client constructs after init")
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

    fn text_meta() -> ClipboardMeta {
        ClipboardMeta {
            kind: ClipboardKind::Text,
            text: "hi".into(),
            data_name: None,
            has_data: false,
            size: 2,
            hash: Some("AA".into()),
        }
    }

    // ── pure helpers ──────────────────────────────────────────────────────

    // Swift: SyncError.mapHTTPStatus (SyncError.swift) — the FULL table.
    #[test]
    fn status_mapping_matches_swift() {
        assert!(map_status(200).is_none());
        assert!(map_status(201).is_none());
        assert!(map_status(204).is_none());
        assert_eq!(map_status(401), Some(SyncError::Unauthorized));
        assert_eq!(map_status(404), Some(SyncError::NotFound));
        assert_eq!(
            map_status(500),
            Some(SyncError::ServerError { status: 500 })
        );
        assert_eq!(
            map_status(503),
            Some(SyncError::ServerError { status: 503 })
        );
        // Everything else — other 2xx, 3xx, non-401/404 4xx — is protocolError.
        for s in [202u16, 206, 300, 302, 400, 403, 405, 418, 451] {
            assert_eq!(
                map_status(s),
                Some(SyncError::ProtocolError { status: s }),
                "status {s}"
            );
        }
    }

    // Swift: SyncClipboardClientTests.test_normalizeBaseURL_* (§1.1).
    #[test]
    fn normalize_base_url_matches_swift() {
        let norm = |s: &str| normalize_base_url(s).map(|u| u.to_string());
        assert_eq!(norm("https://example.com").unwrap(), "https://example.com/");
        assert_eq!(
            norm("https://example.com/").unwrap(),
            "https://example.com/"
        );
        assert_eq!(
            norm("  https://example.com  ").unwrap(),
            "https://example.com/"
        );
        assert_eq!(
            norm("https://nas.local:5033/sync").unwrap(),
            "https://nas.local:5033/sync/"
        );
        assert!(matches!(norm(""), Err(SyncError::InvalidInput { .. })));
        assert!(matches!(
            norm("ftp://example.com"),
            Err(SyncError::InvalidInput { .. })
        ));
        assert!(matches!(
            norm("not-a-url"),
            Err(SyncError::InvalidInput { .. })
        ));
    }

    #[test]
    fn validate_path_component_rejects_separators_and_empty() {
        assert!(validate_path_component("ok.txt", "filename").is_ok());
        for bad in ["", "a/b", "a\\b", "/", "x\\"] {
            assert!(
                matches!(
                    validate_path_component(bad, "filename"),
                    Err(SyncError::InvalidInput { .. })
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn endpoint_normalizes_and_joins_paths() {
        let url = endpoint("http://10.0.0.5:42720/", &["SyncClipboard.json"]).expect("join");
        assert_eq!(url.as_str(), "http://10.0.0.5:42720/SyncClipboard.json");
        // No trailing slash on the base, and a segment needing percent-encoding.
        let url = endpoint("http://10.0.0.5:42720", &["file", "a b.png"]).expect("join");
        assert_eq!(url.as_str(), "http://10.0.0.5:42720/file/a%20b.png");
        // A base PATH is preserved beneath the appended segments.
        let url =
            endpoint("https://nas.local:5033/sync", &["api", "history", "query"]).expect("join");
        assert_eq!(
            url.as_str(),
            "https://nas.local:5033/sync/api/history/query"
        );
        assert!(matches!(
            endpoint("ftp://x/", &["a"]),
            Err(SyncError::InvalidInput { .. })
        ));
    }

    // ── SyncClipboard.json (§2.1/§2.2) ────────────────────────────────────

    #[tokio::test]
    async fn get_latest_decodes_doc() {
        let (addr, _state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        let meta = client
            .get_latest(server_cfg(addr, "p"))
            .await
            .expect("get ok");
        assert_eq!(meta.kind, ClipboardKind::Text);
        assert_eq!(meta.text, "hello from daemon");
        assert_eq!(meta.hash.as_deref(), Some("AA"));
    }

    #[tokio::test]
    async fn put_clipboard_sends_file_before_doc() {
        let (addr, state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        client
            .put_clipboard(server_cfg(addr, "p"), file_meta(), Some(vec![1, 2, 3]))
            .await
            .expect("put ok");
        assert_eq!(state.events(), vec!["put-file:f.bin:3", "put-doc:File"]);
    }

    #[tokio::test]
    async fn put_clipboard_accepts_201_and_204() {
        for status in [201u16, 204] {
            let (addr, _state) = spawn_mock(MockConfig {
                forced_status: Some(status),
                ..Default::default()
            })
            .await;
            let client = new_client();
            client
                .put_clipboard(server_cfg(addr, "p"), text_meta(), None)
                .await
                .unwrap_or_else(|e| panic!("status {status} must succeed, got {e:?}"));
        }
    }

    #[tokio::test]
    async fn wrong_password_maps_to_unauthorized() {
        let (addr, _state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        let err = client
            .get_latest(server_cfg(addr, "wrong"))
            .await
            .expect_err("must 401");
        assert_eq!(err, SyncError::Unauthorized);
    }

    // Swift: test_getClipboard_returns{401,404,500,Other4xx}As* — the mapping
    // end-to-end through a real HTTP round trip.
    #[tokio::test]
    async fn get_latest_maps_http_statuses() {
        let cases = [
            (401u16, SyncError::Unauthorized),
            (404, SyncError::NotFound),
            (500, SyncError::ServerError { status: 500 }),
            (418, SyncError::ProtocolError { status: 418 }),
        ];
        for (status, want) in cases {
            let (addr, _state) = spawn_mock(MockConfig {
                forced_status: Some(status),
                ..Default::default()
            })
            .await;
            let client = new_client();
            let err = client
                .get_latest(server_cfg(addr, "p"))
                .await
                .expect_err("status error");
            assert_eq!(err, want, "status {status}");
        }
    }

    // Swift: test_getClipboard_malformedJSONFailsAsDecodingFailed.
    #[tokio::test]
    async fn get_latest_malformed_json_maps_to_decoding_failed() {
        let addr = spawn_raw_doc_mock(200, "not-json").await;
        let client = new_client();
        let err = client
            .get_latest(server_cfg(addr, "p"))
            .await
            .expect_err("decode failure");
        assert!(
            matches!(err, SyncError::DecodingFailed { .. }),
            "got {err:?}"
        );
    }

    // Swift: test_basicAuthHeader_matchesSpecExample.
    #[tokio::test]
    async fn basic_auth_header_matches_spec() {
        let (addr, state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        let cfg = ServerConfig {
            base_url: format!("http://{addr}"),
            username: "alice".into(),
            password: "secret".into(),
        };
        // The mock expects u:p, so this 401s — we only assert the header bytes.
        let _ = client.get_latest(cfg).await;
        let auth = state.last_auth.lock().expect("mock lock").clone();
        assert_eq!(auth.as_deref(), Some("Basic YWxpY2U6c2VjcmV0"));
    }

    // ── file endpoints (§2.3/§2.4) ────────────────────────────────────────

    #[tokio::test]
    async fn put_file_uploads_bytes() {
        let (addr, state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        client
            .put_file(
                server_cfg(addr, "p"),
                "text_ABC.txt".into(),
                vec![0xDE, 0xAD, 0xBE, 0xEF],
            )
            .await
            .expect("put file ok");
        assert_eq!(state.events(), vec!["put-file:text_ABC.txt:4"]);
    }

    #[tokio::test]
    async fn get_file_returns_bytes_verbatim() {
        let payload: Vec<u8> = (0..=255u16).map(|b| b as u8).collect();
        let (addr, _state) = spawn_mock(MockConfig {
            file_bytes: payload.clone(),
            ..Default::default()
        })
        .await;
        let client = new_client();
        let got = client
            .get_file(server_cfg(addr, "p"), "blob.bin".into())
            .await
            .expect("get file ok");
        assert_eq!(got, payload);
    }

    #[tokio::test]
    async fn get_file_404_maps_to_not_found() {
        let (addr, _state) = spawn_mock(MockConfig {
            forced_status: Some(404),
            ..Default::default()
        })
        .await;
        let client = new_client();
        let err = client
            .get_file(server_cfg(addr, "p"), "x.bin".into())
            .await
            .expect_err("404");
        assert_eq!(err, SyncError::NotFound);
    }

    #[tokio::test]
    async fn file_endpoints_reject_bad_filenames_before_network() {
        let (addr, state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        for bad in ["a/b", "..\\b", ""] {
            let put_err = client
                .put_file(server_cfg(addr, "p"), bad.into(), vec![0])
                .await
                .expect_err("invalid put filename");
            let get_err = client
                .get_file(server_cfg(addr, "p"), bad.into())
                .await
                .expect_err("invalid get filename");
            assert!(matches!(put_err, SyncError::InvalidInput { .. }), "{bad:?}");
            assert!(matches!(get_err, SyncError::InvalidInput { .. }), "{bad:?}");
        }
        assert!(
            state.events().is_empty(),
            "invalid filenames must not reach the network"
        );
    }

    // ── history (§2.7/§2.11) ──────────────────────────────────────────────

    #[tokio::test]
    async fn query_history_decodes_records_and_posts_multipart() {
        let mut rec = ProtoHistoryRecord::new("ABC123", ProtoKind::Text);
        rec.text = Some("hi".into());
        rec.size = Some(2);
        rec.create_time = DateTime::from_timestamp_millis(1_700_000_000_000);
        rec.last_modified = DateTime::from_timestamp_millis(1_700_000_001_000);
        rec.starred = true;
        rec.version = Some(0);
        let (addr, state) = spawn_mock(MockConfig {
            history: vec![rec],
            ..Default::default()
        })
        .await;
        let client = new_client();
        let query = HistoryQuery {
            page: Some(2),
            types: Some(15),
            modified_after_ms: Some(1_700_000_000_000),
            ..Default::default()
        };
        let records = client
            .query_history(server_cfg(addr, "p"), query)
            .await
            .expect("query ok");
        assert_eq!(records.len(), 1);
        let got = &records[0];
        assert_eq!(got.hash, "ABC123");
        assert_eq!(got.kind, ClipboardKind::Text);
        assert_eq!(got.text.as_deref(), Some("hi"));
        assert!(got.starred);
        assert_eq!(got.create_time_ms, Some(1_700_000_000_000));
        assert_eq!(got.last_modified_ms, Some(1_700_000_001_000));
        assert_eq!(got.version, Some(0));

        // Request wiring: multipart content-type + the fields we set, nil
        // fields omitted (byte-exactness of the body is proto-tested).
        let content_type = state
            .last_query_content_type
            .lock()
            .expect("mock lock")
            .clone()
            .expect("content-type captured");
        assert!(
            content_type.starts_with("multipart/form-data; boundary="),
            "got {content_type}"
        );
        let body = state
            .last_query_body
            .lock()
            .expect("mock lock")
            .clone()
            .expect("body captured");
        let body = String::from_utf8(body).expect("utf8 body");
        assert!(body.contains("name=\"page\"\r\n\r\n2\r\n"), "{body}");
        assert!(body.contains("name=\"types\"\r\n\r\n15\r\n"), "{body}");
        assert!(body.contains("name=\"modifiedAfter\""), "{body}");
        assert!(
            !body.contains("name=\"starred\""),
            "nil field must be omitted"
        );
    }

    #[tokio::test]
    async fn query_history_empty_array_is_ok() {
        let (addr, _state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        let records = client
            .query_history(server_cfg(addr, "p"), HistoryQuery::default())
            .await
            .expect("empty page is end-of-list, not an error");
        assert!(records.is_empty());
    }

    #[tokio::test]
    async fn get_history_payload_returns_bytes() {
        let payload = vec![1u8, 2, 3, 4, 5];
        let (addr, _state) = spawn_mock(MockConfig {
            file_bytes: payload.clone(),
            ..Default::default()
        })
        .await;
        let client = new_client();
        let got = client
            .get_history_payload(server_cfg(addr, "p"), "Image-ABCDEF".into())
            .await
            .expect("history data ok");
        assert_eq!(got, payload);
    }

    #[tokio::test]
    async fn get_history_payload_rejects_bad_profile_id() {
        let (addr, state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        let err = client
            .get_history_payload(server_cfg(addr, "p"), "a/b".into())
            .await
            .expect_err("invalid profileId");
        assert!(matches!(err, SyncError::InvalidInput { .. }));
        assert!(
            state.events().is_empty(),
            "no network for invalid profileId"
        );
    }

    // ── retry (§ perform) ─────────────────────────────────────────────────

    // Swift: perform's 300ms retry-once for .timedOut.
    #[tokio::test]
    async fn retry_on_timeout_then_succeeds() {
        let (addr, state) = spawn_mock(MockConfig {
            first_get_delay: Duration::from_millis(400),
            ..Default::default()
        })
        .await;
        let client = new_client_total_timeout(150);
        let meta = client
            .get_latest(server_cfg(addr, "p"))
            .await
            .expect("the retry recovers after the first attempt times out");
        assert_eq!(meta.text, "hello from daemon");
        assert_eq!(state.get_attempts(), 2, "exactly one retry after a timeout");
    }

    // Swift: perform's 300ms retry-once for .networkConnectionLost.
    #[tokio::test]
    async fn retry_on_connection_reset_then_succeeds() {
        let body = r#"{"type":"Text","text":"after-reset","hasData":false}"#.to_string();
        let (addr, conns) = spawn_reset_then_ok_mock(body).await;
        let client = new_client();
        let meta = client
            .get_latest(server_cfg(addr, "p"))
            .await
            .expect("the retry recovers after a connection reset");
        assert_eq!(meta.text, "after-reset");
        assert_eq!(
            conns.load(Ordering::Relaxed),
            2,
            "exactly one retry after a connection reset"
        );
    }

    // Swift: 401 is a status, not a send error — it is never retried.
    #[tokio::test]
    async fn status_errors_are_not_retried() {
        let (addr, state) = spawn_mock(MockConfig {
            forced_status: Some(401),
            ..Default::default()
        })
        .await;
        let client = new_client();
        let err = client
            .get_latest(server_cfg(addr, "p"))
            .await
            .expect_err("401");
        assert_eq!(err, SyncError::Unauthorized);
        assert_eq!(state.get_attempts(), 1, "status errors must not retry");
    }

    // ── cancellation (§5.3, deliberate no-poison divergence) ──────────────

    /// Seam 3: dropping the exported future mid file→metadata window must NOT
    /// interrupt the sequence — the detached task finishes both requests.
    #[tokio::test]
    async fn dropped_put_future_still_completes_file_and_doc() {
        let (addr, state) = spawn_mock(MockConfig {
            file_delay: Duration::from_millis(150),
            ..Default::default()
        })
        .await;
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
        let (addr, _state) = spawn_mock(MockConfig {
            file_delay: Duration::from_millis(500),
            ..Default::default()
        })
        .await;
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

    /// Deliberate divergence from Swift: cancel does NOT poison the long-lived
    /// client — a subsequent request (fresh ServerConfig from the native
    /// shell) proceeds normally instead of throwing `.cancelled`.
    #[tokio::test]
    async fn cancel_does_not_poison_subsequent_requests() {
        let (addr, _state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        client.cancel_in_flight();
        let meta = client
            .get_latest(server_cfg(addr, "p"))
            .await
            .expect("a fresh request after cancel must still work");
        assert_eq!(meta.kind, ClipboardKind::Text);
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

    // ── connectivity probe (A7 / §5.3 Layer 2) ────────────────────────────
    //
    // Swift mirror: ConnectionTesterProbeTests.swift. Those tests route a
    // single MockURLProtocol session by request host; here each candidate is a
    // real axum mock on its own loopback port (one port = one status), and a
    // closed port (`127.0.0.1:1`) stands in for "connection refused".

    fn url_of(addr: SocketAddr) -> String {
        format!("http://{addr}")
    }

    /// Build a `[String: ProbeResult]` map from string literals (firstReachable
    /// + shape-order tests work over synthetic verdicts, no network).
    fn results_map(pairs: &[(&str, ProbeResult)]) -> HashMap<String, ProbeResult> {
        pairs.iter().map(|(u, r)| ((*u).to_string(), *r)).collect()
    }

    // Swift: test_probe_mapsStatusPerCandidate (+ epoch is M3-only).
    #[tokio::test]
    async fn probe_maps_status_per_candidate() {
        let (ok, _s1) = spawn_mock(MockConfig::default()).await; // 200 + body
        let (empty, _s2) = spawn_mock(MockConfig {
            forced_status: Some(404),
            ..Default::default()
        })
        .await;
        let (badauth, _s3) = spawn_mock(MockConfig {
            forced_status: Some(401),
            ..Default::default()
        })
        .await;
        let (broken, _s4) = spawn_mock(MockConfig {
            forced_status: Some(500),
            ..Default::default()
        })
        .await;
        let gone = "http://127.0.0.1:1".to_string();

        let client = new_client();
        let report = client
            .probe(
                vec![
                    url_of(ok),
                    url_of(empty),
                    url_of(badauth),
                    url_of(broken),
                    gone.clone(),
                ],
                "u".into(),
                "p".into(),
                false,
                2000,
                7,
            )
            .await;

        assert_eq!(report.network_epoch, 7, "epoch stamped through verbatim");
        assert_eq!(report.results[&url_of(ok)], ProbeResult::Success);
        assert_eq!(report.results[&url_of(empty)], ProbeResult::Success); // 404 = reachable (§2.1)
        assert_eq!(report.results[&url_of(badauth)], ProbeResult::AuthFailed); // 401 = reachable, creds wrong
        assert_eq!(report.results[&url_of(broken)], ProbeResult::Unreachable); // 500
        assert_eq!(report.results[&gone], ProbeResult::Unreachable); // refused
        assert_eq!(report.results.len(), 5);
    }

    // Swift: test_probe_requestTargetsSyncClipboardJSONWithBasicAuth.
    #[tokio::test]
    async fn probe_targets_syncclipboard_json_with_basic_auth() {
        let (addr, state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        let report = client
            .probe(vec![url_of(addr)], "u".into(), "p".into(), false, 2000, 0)
            .await;
        assert_eq!(report.results[&url_of(addr)], ProbeResult::Success);
        assert!(
            state.events().contains(&"get-doc".to_string()),
            "probe must GET /SyncClipboard.json"
        );
        let auth = state.last_auth.lock().expect("mock lock").clone();
        assert_eq!(auth.as_deref(), Some("Basic dTpw")); // base64("u:p")
    }

    // Swift: test_probe_emptyCredentials_allMissingFields_withoutNetwork.
    #[tokio::test]
    async fn probe_empty_credentials_all_missing_fields_without_network() {
        // No mock installed: a network attempt would surface as Unreachable, so
        // MissingFields proves we never issued one.
        let client = new_client();
        let report = client
            .probe(
                vec!["https://a.example".into(), "https://b.example".into()],
                "u".into(),
                String::new(),
                false,
                2000,
                0,
            )
            .await;
        assert_eq!(
            report.results["https://a.example"],
            ProbeResult::MissingFields
        );
        assert_eq!(
            report.results["https://b.example"],
            ProbeResult::MissingFields
        );
        assert_eq!(report.results.len(), 2);
    }

    // Swift: test_probe_emptyList_returnsEmpty (+ epoch still stamped).
    #[tokio::test]
    async fn probe_empty_list_returns_empty_with_epoch() {
        let client = new_client();
        let report = client
            .probe(vec![], "u".into(), "p".into(), false, 2000, 42)
            .await;
        assert!(report.results.is_empty());
        assert_eq!(report.network_epoch, 42);
    }

    // Swift: test_probe_malformedURL_isUnreachable_blankURL_isMissingFields.
    #[tokio::test]
    async fn probe_malformed_url_unreachable_blank_missing_fields() {
        let client = new_client();
        let report = client
            .probe(
                vec!["not-a-url".into(), "   ".into()],
                "u".into(),
                "p".into(),
                false,
                2000,
                0,
            )
            .await;
        assert_eq!(report.results["not-a-url"], ProbeResult::Unreachable);
        assert_eq!(report.results["   "], ProbeResult::MissingFields);
    }

    // Swift: test_probe_dedupesRepeatedCandidates.
    #[tokio::test]
    async fn probe_dedupes_repeated_candidates() {
        let (addr, _state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        let report = client
            .probe(
                vec![url_of(addr), url_of(addr)],
                "u".into(),
                "p".into(),
                false,
                2000,
                0,
            )
            .await;
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[&url_of(addr)], ProbeResult::Success);
    }

    // §5.3: the short timeout is the "is this path up right now" guard. A mock
    // that stalls past the timeout (and the probe issues exactly one GET — no
    // retry) resolves to Unreachable.
    #[tokio::test]
    async fn probe_times_out_to_unreachable() {
        let (addr, _state) = spawn_mock(MockConfig {
            first_get_delay: Duration::from_millis(500),
            ..Default::default()
        })
        .await;
        let client = new_client();
        let report = client
            .probe(vec![url_of(addr)], "u".into(), "p".into(), false, 100, 0)
            .await;
        assert_eq!(report.results[&url_of(addr)], ProbeResult::Unreachable);
    }

    // trust=true builds a danger-accept client; it must still drive a plain
    // HTTP probe (full self-signed TLS verification is a device/M6 concern).
    #[tokio::test]
    async fn probe_trust_insecure_still_works_over_plain_http() {
        let (addr, _state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        let report = client
            .probe(vec![url_of(addr)], "u".into(), "p".into(), true, 2000, 0)
            .await;
        assert_eq!(report.results[&url_of(addr)], ProbeResult::Success);
    }

    // ── firstReachable pick (pure, §5.3 shape order) ──────────────────────

    // Swift: test_firstReachable_skipsUnreachableHead.
    #[test]
    fn first_reachable_skips_unreachable_head() {
        let ordered = vec![
            "https://lan.example".to_string(),
            "https://ts.example".to_string(),
            "https://wan.example".to_string(),
        ];
        let results = results_map(&[
            ("https://lan.example", ProbeResult::Unreachable),
            ("https://ts.example", ProbeResult::Success),
            ("https://wan.example", ProbeResult::Success),
        ]);
        assert_eq!(
            first_reachable(ordered, results).as_deref(),
            Some("https://ts.example")
        );
    }

    // Swift: test_firstReachable_authFailedCountsAsReachable.
    #[test]
    fn first_reachable_auth_failed_counts_as_reachable() {
        let ordered = vec![
            "https://lan.example".to_string(),
            "https://wan.example".to_string(),
        ];
        let results = results_map(&[
            ("https://lan.example", ProbeResult::AuthFailed),
            ("https://wan.example", ProbeResult::Success),
        ]);
        assert_eq!(
            first_reachable(ordered, results).as_deref(),
            Some("https://lan.example")
        );
    }

    // Swift: test_firstReachable_orderDecidesWhenBothReachable.
    #[test]
    fn first_reachable_order_decides_when_both_reachable() {
        let ordered = vec![
            "https://lan.example".to_string(),
            "https://wan.example".to_string(),
        ];
        let results = results_map(&[
            ("https://lan.example", ProbeResult::Success),
            ("https://wan.example", ProbeResult::Success),
        ]);
        assert_eq!(
            first_reachable(ordered, results).as_deref(),
            Some("https://lan.example")
        );
    }

    // Swift: test_firstReachable_nilWhenNothingReachable.
    #[test]
    fn first_reachable_none_when_nothing_reachable() {
        let ordered = vec![
            "https://lan.example".to_string(),
            "https://wan.example".to_string(),
        ];
        let results = results_map(&[
            ("https://lan.example", ProbeResult::Unreachable),
            ("https://wan.example", ProbeResult::MissingFields),
        ]);
        assert_eq!(first_reachable(ordered, results), None);
    }

    // Swift: test_firstReachable_missingProbeEntryIsNotReachable.
    #[test]
    fn first_reachable_missing_entry_is_not_reachable() {
        assert_eq!(
            first_reachable(vec!["https://lan.example".to_string()], HashMap::new()),
            None
        );
    }

    // Swift: test_probeThenPick_choosesFirstReachableInShapeOrder. Shape order
    // is the proto's `ordered_urls` (M1, byte-checked); the probe verdict here
    // is synthetic so the pick's determinism is what's under test (the real
    // network probe is covered by `probe_then_pick_over_live_mocks`).
    #[test]
    fn probe_then_pick_chooses_first_reachable_in_shape_order() {
        use uc_mobile_proto::{ordered_urls, NetworkContext};
        let urls = vec![
            "https://wan.example".to_string(),
            "http://192.168.1.9:5033".to_string(),
            "https://host.ts.net".to_string(),
        ];
        let ordered = ordered_urls(
            &urls,
            &NetworkContext {
                is_wifi: true,
                ..Default::default()
            },
        );
        assert_eq!(
            ordered,
            vec![
                "http://192.168.1.9:5033".to_string(),
                "https://host.ts.net".to_string(),
                "https://wan.example".to_string(),
            ]
        );
        let results = results_map(&[
            ("http://192.168.1.9:5033", ProbeResult::Unreachable), // LAN down
            ("https://host.ts.net", ProbeResult::Success),         // TS up, empty
            ("https://wan.example", ProbeResult::Success),
        ]);
        // Deterministic: TS wins over WAN because it ranks earlier, not faster.
        assert_eq!(
            first_reachable(ordered, results).as_deref(),
            Some("https://host.ts.net")
        );
    }

    // End-to-end over REAL probed mocks: an unreachable head is skipped for the
    // next reachable candidate in the given order.
    #[tokio::test]
    async fn probe_then_pick_over_live_mocks() {
        let (down, _s1) = spawn_mock(MockConfig {
            forced_status: Some(500),
            ..Default::default()
        })
        .await;
        let (up, _s2) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        let report = client
            .probe(
                vec![url_of(down), url_of(up)],
                "u".into(),
                "p".into(),
                false,
                2000,
                3,
            )
            .await;
        // `down` ranks first but is unreachable, so the pick falls to `up`.
        let ordered = vec![url_of(down), url_of(up)];
        assert_eq!(first_reachable(ordered, report.results), Some(url_of(up)));
    }

    // ── single-URL test_connection (§5.3 Layer 2 single form) ─────────────

    #[tokio::test]
    async fn test_connection_success_on_200() {
        let (addr, _state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        assert_eq!(
            client.test_connection(server_cfg(addr, "p"), false).await,
            ProbeResult::Success
        );
    }

    // §2.1: 404 = "no clipboard published yet" = reachable, which is what the
    // user is testing — so it maps to Success, NOT NotFound.
    #[tokio::test]
    async fn test_connection_404_is_success() {
        let (addr, _state) = spawn_mock(MockConfig {
            forced_status: Some(404),
            ..Default::default()
        })
        .await;
        let client = new_client();
        assert_eq!(
            client.test_connection(server_cfg(addr, "p"), false).await,
            ProbeResult::Success
        );
    }

    #[tokio::test]
    async fn test_connection_wrong_password_is_auth_failed() {
        let (addr, _state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        assert_eq!(
            client
                .test_connection(server_cfg(addr, "wrong"), false)
                .await,
            ProbeResult::AuthFailed
        );
    }

    #[tokio::test]
    async fn test_connection_server_error_is_unreachable() {
        let (addr, _state) = spawn_mock(MockConfig {
            forced_status: Some(500),
            ..Default::default()
        })
        .await;
        let client = new_client();
        assert_eq!(
            client.test_connection(server_cfg(addr, "p"), false).await,
            ProbeResult::Unreachable
        );
    }

    // A reachable server returning an undecodable 2xx body is Unreachable
    // (Swift `test`'s catch-all arm), distinct from the probe which never
    // decodes.
    #[tokio::test]
    async fn test_connection_decode_failure_is_unreachable() {
        let addr = spawn_raw_doc_mock(200, "not-json").await;
        let client = new_client();
        assert_eq!(
            client.test_connection(server_cfg(addr, "p"), false).await,
            ProbeResult::Unreachable
        );
    }

    #[tokio::test]
    async fn test_connection_missing_fields_makes_no_request() {
        let (addr, state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        let cfg = ServerConfig {
            base_url: format!("http://{addr}"),
            username: "u".into(),
            password: String::new(),
        };
        assert_eq!(
            client.test_connection(cfg, false).await,
            ProbeResult::MissingFields
        );
        assert!(
            state.events().is_empty(),
            "missing fields must short-circuit before any request"
        );
    }

    #[tokio::test]
    async fn test_connection_blank_url_is_missing_fields() {
        let client = new_client();
        let cfg = ServerConfig {
            base_url: "   ".into(),
            username: "u".into(),
            password: "p".into(),
        };
        assert_eq!(
            client.test_connection(cfg, false).await,
            ProbeResult::MissingFields
        );
    }

    // Non-empty but unparseable URL → endpoint() rejects → Unreachable (Swift:
    // a failed client constructor maps to .unreachable).
    #[tokio::test]
    async fn test_connection_malformed_url_is_unreachable() {
        let client = new_client();
        let cfg = ServerConfig {
            base_url: "not-a-url".into(),
            username: "u".into(),
            password: "p".into(),
        };
        assert_eq!(
            client.test_connection(cfg, false).await,
            ProbeResult::Unreachable
        );
    }

    #[tokio::test]
    async fn test_connection_trust_insecure_still_works_over_plain_http() {
        let (addr, _state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        assert_eq!(
            client.test_connection(server_cfg(addr, "p"), true).await,
            ProbeResult::Success
        );
    }

    // E区: a production client constructed with trust=true (the settings toggle
    // on) still drives a normal request — danger-accept-certs is a superset of
    // the validating client over plain HTTP.
    #[tokio::test]
    async fn production_client_built_with_trust_drives_plain_http() {
        let (addr, _state) = spawn_mock(MockConfig::default()).await;
        uc_mobile_init();
        let client = MobileSyncClient::new(Arc::new(NoopBridge), true)
            .expect("client constructs after init");
        let meta = client
            .get_latest(server_cfg(addr, "p"))
            .await
            .expect("trust-true production client works over plain http");
        assert_eq!(meta.text, "hello from daemon");
    }

    // E区: toggling `set_trust_insecure_cert` swaps the production client in
    // place (no runtime restart) and the next request still works.
    #[tokio::test]
    async fn set_trust_insecure_cert_swaps_client_and_keeps_working() {
        let (addr, _state) = spawn_mock(MockConfig::default()).await;
        let client = new_client();
        client
            .set_trust_insecure_cert(true)
            .expect("swap to trust-true succeeds");
        let meta = client
            .get_latest(server_cfg(addr, "p"))
            .await
            .expect("client still works after trust swap");
        assert_eq!(meta.hash.as_deref(), Some("AA"));
        // Swap back to validating; a plain-HTTP request is unaffected by TLS
        // policy, so it still succeeds.
        client
            .set_trust_insecure_cert(false)
            .expect("swap back to validating succeeds");
        assert!(client.get_latest(server_cfg(addr, "p")).await.is_ok());
    }
}
