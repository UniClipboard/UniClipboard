//! Content lock: who may read history-derived content from the daemon.
//!
//! Two facts stay separate:
//!
//! * the **encryption session** (Engine): keys are loaded, background sync and capture work;
//! * the **content grant** (this module): the user authorised showing content in a GUI surface.
//!
//! `unlocked = grant && initialized && session_ready && background_ready`, computed at read time,
//! so losing the session or entering profile recovery revokes visibility without extra coupling.
//! The grant lives in daemon memory only: it is dropped when the daemon restarts, when the
//! process that granted it exits (a GUI crash), and it is re-seeded from
//! `security.auto_unlock_enabled` the first time it is read.
//!
//! ## What it protects, and what it does not
//!
//! Only sessions whose declared client type is in [`GATED_CLIENT_TYPES`] (the desktop GUI and
//! the quick panel helper) are refused. The CLI and other clients keep their existing
//! semantics: they read whenever the encryption session is ready. The client type is declared by
//! the caller at `/auth/connect` and the daemon trusts the same OS user (see the note on
//! `UnlockSpaceRequest`), so this is a lock on the GUI surfaces, **not** a boundary against a
//! local process that holds the daemon token.
//!
//! ## Enforcement
//!
//! One middleware ([`content_gate_middleware`]) sits on the authenticated router and decides by
//! route template, using [`route_class`]. A test walks every path in the OpenAPI document and
//! fails when a route is in neither table, so a new route cannot silently skip the lock.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::{MatchedPath, Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uc_daemon_contract::api::dto::encryption::{ContentLockStatusResponse, UnlockSpaceRequest};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::constants::{http_route, ws_event, ws_topic};
use uc_engine::{Operation, OperationResult};

use crate::api::dto::error::ApiError;
use crate::api::projection::IntoApiDto;
use crate::api::server::DaemonApiState;
use crate::api::types::DaemonWsEvent;
use crate::security::claims::SessionTokenClaims;

/// Stable error code of a refused read. Distinct from `session_locked` (the encryption session
/// is not ready) because the remedy differs: the user has to authorise the content in the app.
pub const CONTENT_LOCKED_CODE: &str = "content_locked";

/// Declared client types that need the grant. Everything else is unchanged.
pub const GATED_CLIENT_TYPES: &[&str] = &["gui", "helper"];

/// The single place that says which clients the lock applies to.
pub fn client_is_gated(client_type: &str) -> bool {
    GATED_CLIENT_TYPES.contains(&client_type)
}

/// How often the daemon checks that the process that granted access is still there.
const HOLDER_CHECK_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, Default)]
struct Grant {
    value: Option<bool>,
    generation: u64,
    holder_pid: Option<u32>,
    /// The last `unlocked` value that was published, to publish only real changes.
    published: Option<bool>,
}

/// The in-memory grant. Cheap to clone: every clone shares one state.
#[derive(Clone, Default)]
pub struct ContentLock {
    inner: Arc<Mutex<Grant>>,
}

impl ContentLock {
    fn with<R>(&self, f: impl FnOnce(&mut Grant) -> R) -> R {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    fn snapshot(&self) -> Grant {
        self.with(|grant| *grant)
    }

    pub fn generation(&self) -> u64 {
        self.snapshot().generation
    }

    /// Grants access on behalf of the process `pid`. Returns the new generation.
    fn grant(&self, pid: u32) -> u64 {
        self.with(|grant| {
            grant.value = Some(true);
            grant.holder_pid = Some(pid);
            grant.generation = grant.generation.wrapping_add(1);
            grant.generation
        })
    }

    /// Withdraws access. Returns the new generation.
    fn revoke(&self) -> u64 {
        self.with(|grant| {
            grant.value = Some(false);
            grant.holder_pid = None;
            grant.generation = grant.generation.wrapping_add(1);
            grant.generation
        })
    }

    /// Forgets the grant, as if the daemon had just started, unless it changed meanwhile.
    fn reset_if_generation(&self, generation: u64) {
        self.with(|grant| {
            if grant.generation == generation {
                grant.value = None;
                grant.holder_pid = None;
                grant.generation = grant.generation.wrapping_add(1);
            }
        });
    }

    /// Seeds an unset grant, unless it changed meanwhile.
    fn seed_if_generation(&self, generation: u64, value: bool) {
        self.with(|grant| {
            if grant.generation == generation && grant.value.is_none() {
                grant.value = Some(value);
                grant.generation = grant.generation.wrapping_add(1);
            }
        });
    }

    /// Records that `unlocked` is what subscribers now know. Returns the generation to publish
    /// when this is news. A change of the underlying facts (session lost, recovery entered)
    /// advances the generation too, so in-flight responses from before it are recognisable.
    fn observe(&self, unlocked: bool, force: bool) -> Option<u64> {
        self.with(|grant| {
            let changed = grant.published != Some(unlocked);
            if changed && !force {
                grant.generation = grant.generation.wrapping_add(1);
            }
            grant.published = Some(unlocked);
            (changed || force).then_some(grant.generation)
        })
    }

    fn holder_pid(&self) -> Option<u32> {
        self.snapshot().holder_pid
    }
}

/// Reads the current answer from the grant and the Engine facts.
pub(crate) async fn resolve_status(
    state: &DaemonApiState,
) -> Result<ContentLockStatusResponse, ApiError> {
    let before = state.content_lock.snapshot();
    let unavailable = |what: &'static str| {
        ApiError::service_unavailable(format!("content lock state unavailable: {what}"))
    };

    let recovery = match state.execute(Operation::QueryProfileRecovery).await {
        Ok(OperationResult::ProfileRecovery(recovery)) => recovery,
        _ => return Err(unavailable("profile recovery")),
    };
    if !recovery.background_ready {
        state.content_lock.reset_if_generation(before.generation);
        return Ok(ContentLockStatusResponse {
            unlocked: false,
            generation: state.content_lock.generation(),
        });
    }
    let encryption = match state.execute(Operation::QueryEncryptionState).await {
        Ok(OperationResult::EncryptionState(view)) => view,
        _ => return Err(unavailable("encryption state")),
    };
    if !encryption.initialized {
        state.content_lock.reset_if_generation(before.generation);
        return Ok(ContentLockStatusResponse {
            unlocked: false,
            generation: state.content_lock.generation(),
        });
    }
    if before.value.is_none() {
        let settings = match state.execute(Operation::QuerySettings).await {
            Ok(OperationResult::Settings(settings)) => (*settings).into_api_dto(),
            _ => return Err(unavailable("settings")),
        };
        state
            .content_lock
            .seed_if_generation(before.generation, settings.security.auto_unlock_enabled);
    }
    let now = state.content_lock.snapshot();
    Ok(ContentLockStatusResponse {
        unlocked: now.value.unwrap_or(false) && encryption.session_ready,
        generation: now.generation,
    })
}

fn locked_error() -> ApiError {
    ApiError {
        status: StatusCode::LOCKED,
        code: CONTENT_LOCKED_CODE.to_string(),
        message: "content is locked; unlock it in the app".to_string(),
        details: None,
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

fn publish(state: &DaemonApiState, status: &ContentLockStatusResponse) {
    let ts = chrono::Utc::now().timestamp_millis();
    let event = DaemonWsEvent {
        topic: ws_topic::CONTENT_LOCK.to_string(),
        event_type: ws_event::CONTENT_LOCK_CHANGED.to_string(),
        session_id: None,
        ts,
        payload: serde_json::json!({
            "unlocked": status.unlocked,
            "generation": status.generation,
        }),
    };
    if state.event_tx.send(event).is_err() {
        debug!("content_lock.changed had no active subscribers");
    }
}

/// Recomputes the answer after the grant or a fact behind it changed, and tells subscribers when
/// it did. `force` publishes even if the boolean did not change (a new grant generation).
pub(crate) async fn publish_current(state: &DaemonApiState, force: bool) {
    match resolve_status(state).await {
        Ok(status) => {
            if let Some(generation) = state.content_lock.observe(status.unlocked, force) {
                publish(
                    state,
                    &ContentLockStatusResponse {
                        unlocked: status.unlocked,
                        generation,
                    },
                );
            }
        }
        Err(error) => warn!(message = %error.message, "content lock state could not be read"),
    }
}

/// Call after something outside this module changed a fact the lock depends on (the session
/// was locked, the space was reset). Runs in the background so the caller is not delayed.
pub(crate) fn notify_facts_changed(state: &DaemonApiState) {
    let state = state.clone();
    tokio::spawn(async move { publish_current(&state, false).await });
}

/// Keeps the lock honest between requests: it follows the encryption events and revokes the
/// grant when the process that granted it is gone.
pub async fn run_watcher(state: DaemonApiState, cancel: CancellationToken) {
    let mut events = state.event_tx.subscribe();
    let mut tick = tokio::time::interval(HOLDER_CHECK_INTERVAL);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tick.tick() => {
                if let Some(pid) = state.content_lock.holder_pid() {
                    if !uc_daemon_process::process_metadata::is_pid_alive(pid) {
                        info!(pid, "the process that granted content access is gone; revoking");
                        state.content_lock.revoke();
                        publish_current(&state, true).await;
                    }
                }
            }
            event = events.recv() => match event {
                Ok(event) if event.topic == ws_topic::ENCRYPTION => publish_current(&state, false).await,
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => publish_current(&state, false).await,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route(http_route::CONTENT_LOCK, get(get_status_handler))
        .route(http_route::CONTENT_LOCK_UNLOCK, post(unlock_handler))
        .route(
            http_route::CONTENT_LOCK_UNLOCK_KEYRING,
            post(unlock_keyring_handler),
        )
        .route(http_route::CONTENT_LOCK_REVOKE, post(revoke_handler))
}

/// GET /content-lock
#[utoipa::path(
    get,
    path = "/content-lock",
    operation_id = "getContentLock",
    tag = "encryption",
    responses(
        (status = 200, description = "Whether content may be shown to GUI-class clients", body = ContentLockStatusEnvelope),
        (status = 503, description = "The state could not be read", body = ApiErrorResponse),
    )
)]
pub(crate) async fn get_status_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<ContentLockStatusResponse>>, ApiError> {
    Ok(Json(ApiEnvelope::now(resolve_status(&state).await?)))
}

/// POST /content-lock/unlock
///
/// Verifies the passphrase (even when the session is already ready) and grants content access.
/// A wrong passphrase leaves everything as it was. The body is never logged.
#[utoipa::path(
    post,
    path = "/content-lock/unlock",
    operation_id = "unlockContent",
    tag = "encryption",
    request_body = UnlockSpaceRequest,
    responses(
        (status = 200, description = "Content unlocked", body = ContentLockStatusEnvelope),
        (status = 403, description = "Wrong passphrase", body = ApiErrorResponse),
        (status = 409, description = "Setup not completed / space not initialized", body = ApiErrorResponse),
        (status = 422, description = "Space key material corrupted", body = ApiErrorResponse),
    )
)]
pub(crate) async fn unlock_handler(
    State(state): State<DaemonApiState>,
    claims: axum::Extension<SessionTokenClaims>,
    Json(req): Json<UnlockSpaceRequest>,
) -> Result<Json<ApiEnvelope<ContentLockStatusResponse>>, ApiError> {
    crate::api::encryption::unlock_space_with_passphrase(&state, req.passphrase).await?;
    state.content_lock.grant(claims.pid);
    info!(
        pid = claims.pid,
        "content unlocked after passphrase verification"
    );
    publish_current(&state, true).await;
    Ok(Json(ApiEnvelope::now(resolve_status(&state).await?)))
}

/// POST /content-lock/unlock-keyring
///
/// Resumes the session from the OS keychain and grants content access. Only for an explicit user
/// action in the app, never called on its own.
#[utoipa::path(
    post,
    path = "/content-lock/unlock-keyring",
    operation_id = "unlockContentFromKeyring",
    tag = "encryption",
    responses(
        (status = 200, description = "Content unlocked when the keychain held a usable key", body = ContentLockStatusEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
    )
)]
pub(crate) async fn unlock_keyring_handler(
    State(state): State<DaemonApiState>,
    claims: axum::Extension<SessionTokenClaims>,
) -> Result<Json<ApiEnvelope<ContentLockStatusResponse>>, ApiError> {
    if crate::api::encryption::resume_session_from_keyring(&state).await? {
        state.content_lock.grant(claims.pid);
        info!(
            pid = claims.pid,
            "content unlocked from the keychain after explicit user action"
        );
        publish_current(&state, true).await;
    }
    Ok(Json(ApiEnvelope::now(resolve_status(&state).await?)))
}

/// POST /content-lock/revoke
///
/// Withdraws the grant. Deliberately does not lock the encryption session: background sync and
/// capture keep working, only what GUI surfaces may show changes.
#[utoipa::path(
    post,
    path = "/content-lock/revoke",
    operation_id = "revokeContentAccess",
    tag = "encryption",
    responses(
        (status = 200, description = "Content access withdrawn", body = ContentLockStatusEnvelope),
    )
)]
pub(crate) async fn revoke_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<ContentLockStatusResponse>>, ApiError> {
    state.content_lock.revoke();
    info!("content access revoked");
    publish_current(&state, true).await;
    Ok(Json(ApiEnvelope::now(resolve_status(&state).await?)))
}

// ---------------------------------------------------------------------------
// Route table and gate
// ---------------------------------------------------------------------------

/// What the lock does with a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteClass {
    /// Returns or acts on history-derived content: refused while locked.
    Content,
    /// Needed to unlock, to render a locked screen, or carries no history-derived content.
    Open,
}

/// OpenAPI-style templates of the routes that touch history-derived content. Everything under
/// `/clipboard/` is here, whatever the verb: reads, and also the actions on entries, because a
/// locked surface has no business acting on content it may not show.
const CONTENT_ROUTES: &[&str] = &[
    "/clipboard/",
    "/search/query",
    "/search/tags",
    "/search/count",
    "/config/export",
];

/// Routes that stay open while locked, with the reason. Kept as data so the audit test can tell
/// "decided open" from "forgotten".
pub const OPEN_ROUTES: &[(&str, &str)] = &[
    (
        "/analytics/capture",
        "UI telemetry events, no history content",
    ),
    ("/auth/connect", "bootstrap of a session"),
    ("/health", "liveness"),
    (
        "/ws",
        "authenticates itself; content topics are filtered per event",
    ),
    ("/content-lock", "the lock itself"),
    ("/content-lock/unlock", "the way to unlock"),
    ("/content-lock/unlock-keyring", "the way to unlock"),
    ("/content-lock/revoke", "the lock itself"),
    ("/config/import", "stages an import, returns no history"),
    ("/config/import/preview", "manifest metadata only"),
    ("/device/me", "device identity, not history"),
    ("/diagnostics/", "diagnostic state and redacted log export"),
    (
        "/encryption/",
        "session management; unlocking must stay reachable",
    ),
    ("/lifecycle/", "daemon lifecycle"),
    ("/member/", "device membership, not history"),
    ("/mobile-sync/", "device management, not history"),
    ("/network/recovery", "network state"),
    ("/paired-devices", "device list"),
    ("/pairing/unpair", "device management"),
    ("/peers", "device list"),
    ("/presence/", "device presence"),
    ("/search/rebuild", "index maintenance, returns no content"),
    ("/search/status", "index availability, no content"),
    ("/settings", "settings, needed to render a locked screen"),
    ("/status", "daemon status"),
    ("/storage/", "sizes and cache maintenance"),
    ("/upgrade/", "upgrade state"),
    ("/v2/setup/", "space setup and pairing"),
];

fn template_matches(template: &str, pattern: &str) -> bool {
    if pattern.ends_with('/') {
        template.starts_with(pattern)
    } else {
        template == pattern || template.starts_with(&format!("{pattern}/"))
    }
}

/// Classifies a route template such as `/clipboard/entries/{id}`. `None` means the route is in
/// neither table, which the audit test turns into a failure.
pub fn route_class(template: &str) -> Option<RouteClass> {
    if CONTENT_ROUTES.iter().any(|p| template_matches(template, p)) {
        return Some(RouteClass::Content);
    }
    if OPEN_ROUTES
        .iter()
        .any(|(p, _)| template_matches(template, p))
    {
        return Some(RouteClass::Open);
    }
    None
}

/// Axum writes path parameters as `:id`, OpenAPI as `{id}`.
fn normalize_template(matched: &str) -> String {
    matched
        .split('/')
        .map(|segment| match segment.strip_prefix(':') {
            Some(name) => format!("{{{name}}}"),
            None => segment.to_string(),
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Refuses content routes for gated clients while content is locked, and refuses a response
/// that finished after the lock changed.
///
/// A route that is in neither table is treated as content: an unknown route fails closed.
pub async fn content_gate_middleware(
    State(state): State<Arc<DaemonApiState>>,
    request: Request,
    next: Next,
) -> Response {
    let gated = request
        .extensions()
        .get::<SessionTokenClaims>()
        .is_some_and(|claims| client_is_gated(&claims.client_type));
    if !gated {
        return next.run(request).await;
    }
    let template = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| normalize_template(matched.as_str()))
        .unwrap_or_else(|| request.uri().path().to_string());
    if route_class(&template) == Some(RouteClass::Open) {
        return next.run(request).await;
    }

    let status = match resolve_status(&state).await {
        Ok(status) => status,
        Err(error) => return error.into_response(),
    };
    if !status.unlocked {
        return locked_error().into_response();
    }
    let response = next.run(request).await;
    // The lock may have changed while the handler ran; the answer must not outlive the grant.
    if state.content_lock.generation() != status.generation {
        return locked_error().into_response();
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use utoipa::OpenApi;

    #[test]
    fn every_documented_route_is_classified() {
        let doc = crate::api::openapi::ApiDoc::openapi();
        let unclassified: Vec<&String> = doc
            .paths
            .paths
            .keys()
            .filter(|path| route_class(path).is_none())
            .collect();
        assert!(
            unclassified.is_empty(),
            "add these routes to CONTENT_ROUTES or OPEN_ROUTES in content_lock.rs, with a reason: {unclassified:?}"
        );
    }

    #[test]
    fn history_derived_routes_are_content() {
        for path in [
            "/clipboard/entries",
            "/clipboard/entries/{id}",
            "/clipboard/entries/{id}/file",
            "/clipboard/entries/{id}/resource",
            "/clipboard/entries/clear",
            "/clipboard/blobs/{blob_id}",
            "/clipboard/thumbnails/{rep_id}",
            "/clipboard/receives",
            "/clipboard/restore/{entry_id}",
            "/clipboard/stats",
            "/clipboard/anything-added-later",
            "/search/query",
            "/search/tags",
            "/search/count",
            "/config/export",
        ] {
            assert_eq!(route_class(path), Some(RouteClass::Content), "{path}");
        }
    }

    #[test]
    fn the_ways_to_unlock_and_to_draw_a_locked_screen_stay_open() {
        for path in [
            "/content-lock",
            "/content-lock/unlock",
            "/content-lock/unlock-keyring",
            "/content-lock/revoke",
            "/encryption/state",
            "/encryption/unlock-with-passphrase",
            "/settings",
            "/health",
            "/ws",
            "/search/status",
            "/v2/setup/state",
        ] {
            assert_eq!(route_class(path), Some(RouteClass::Open), "{path}");
        }
    }

    #[test]
    fn an_unknown_route_is_not_open() {
        assert_eq!(route_class("/brand/new"), None);
        // The gate treats anything that is not explicitly open as content.
        assert_ne!(route_class("/brand/new"), Some(RouteClass::Open));
    }

    #[test]
    fn a_prefix_does_not_open_a_sibling() {
        // "/status" is open; "/statusx" and "/settings-export" are not covered by it.
        assert_eq!(route_class("/status"), Some(RouteClass::Open));
        assert_eq!(route_class("/statusx"), None);
        assert_eq!(route_class("/settings-export"), None);
    }

    #[test]
    fn axum_parameters_become_openapi_parameters() {
        assert_eq!(
            normalize_template("/clipboard/entries/:id/file"),
            "/clipboard/entries/{id}/file"
        );
        assert_eq!(normalize_template("/search/query"), "/search/query");
    }

    #[test]
    fn only_gui_class_clients_are_held_to_the_lock() {
        assert!(client_is_gated("gui"));
        assert!(client_is_gated("helper"));
        assert!(!client_is_gated("cli"));
        assert!(!client_is_gated("other"));
        assert!(!client_is_gated(""));
    }

    #[test]
    fn a_grant_belongs_to_its_holder_and_changes_the_generation() {
        let lock = ContentLock::default();
        let start = lock.generation();
        let granted = lock.grant(42);
        assert_ne!(granted, start);
        assert_eq!(lock.holder_pid(), Some(42));
        assert_eq!(lock.snapshot().value, Some(true));
        let revoked = lock.revoke();
        assert_ne!(revoked, granted);
        assert_eq!(lock.holder_pid(), None);
        assert_eq!(lock.snapshot().value, Some(false));
    }

    #[test]
    fn a_stale_seed_cannot_overwrite_a_newer_grant() {
        let lock = ContentLock::default();
        let seen = lock.generation();
        lock.grant(7);
        lock.seed_if_generation(seen, false);
        assert_eq!(lock.snapshot().value, Some(true));
        lock.reset_if_generation(seen);
        assert_eq!(lock.snapshot().value, Some(true));
    }

    #[test]
    fn seeding_only_fills_an_unset_grant() {
        let lock = ContentLock::default();
        let seen = lock.generation();
        lock.seed_if_generation(seen, true);
        assert_eq!(lock.snapshot().value, Some(true));
        // Seeding does not name a holder, so nothing revokes it when a process exits.
        assert_eq!(lock.holder_pid(), None);
        let after = lock.generation();
        lock.seed_if_generation(after, false);
        assert_eq!(lock.snapshot().value, Some(true));
    }

    #[test]
    fn resetting_returns_to_unset_and_reseeds_later() {
        let lock = ContentLock::default();
        lock.grant(9);
        let seen = lock.generation();
        lock.reset_if_generation(seen);
        assert_eq!(lock.snapshot().value, None);
        assert_eq!(lock.holder_pid(), None);
    }

    #[test]
    fn a_change_of_facts_advances_the_generation_once() {
        let lock = ContentLock::default();
        let first = lock.observe(true, false);
        assert!(first.is_some());
        // Same answer again is not news.
        assert_eq!(lock.observe(true, false), None);
        let before = lock.generation();
        let second = lock.observe(false, false).expect("a change is published");
        assert_ne!(second, before);
        // A forced publication (after a grant) keeps the generation the grant produced.
        let granted = lock.grant(1);
        assert_eq!(lock.observe(true, true), Some(granted));
    }

    #[test]
    fn content_events_are_the_clipboard_and_file_transfer_topics() {
        assert!(crate::api::ws::carries_content_for_tests(
            ws_topic::CLIPBOARD
        ));
        assert!(crate::api::ws::carries_content_for_tests(
            ws_topic::FILE_TRANSFER
        ));
        assert!(!crate::api::ws::carries_content_for_tests(
            ws_topic::CONTENT_LOCK
        ));
        assert!(!crate::api::ws::carries_content_for_tests(ws_topic::PEERS));
    }
}
