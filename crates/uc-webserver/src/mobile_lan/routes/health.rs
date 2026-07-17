//! Public liveness endpoint for the mobile LAN listener.
//!
//! Mobile clients that have gone offline poll this endpoint to discover a
//! reachable route back to the daemon. The answer it gives is deliberately
//! narrow: "a mobile LAN listener is alive at this address and still accepting
//! SyncClipboard requests". It says nothing about credentials, unlock state,
//! clipboard readability, or whether the next real sync will succeed — a
//! successful probe only earns the client the right to attempt one real sync,
//! which stays the authority on sync state.
//!
//! ## Why this is public
//!
//! `/SyncClipboard.json` cannot serve as a recovery probe: every call runs an
//! Argon2id verification plus a clipboard snapshot read. At a 2s cadence
//! across two candidate URLs that is a memory-hard hash roughly once a second,
//! indefinitely, while the daemon is only half-available. A public endpoint at
//! constant cost has strictly better abuse characteristics than one that
//! spends Argon2 CPU before it can reject a caller. The single bit it leaks —
//! that something answers here — is one a TCP connect already reveals.
//!
//! ## Why the state is a bare `CancellationToken`
//!
//! This handler must not reach any business dependency: no facade, no
//! repository port, no secure storage, no blob store, no hasher. Rather than
//! rely on review or tests to keep it that way, the public router carries only
//! the listener's cancellation token as state, so no such dependency is in
//! scope to call. The constant-cost property is enforced by the type system;
//! `healthz_never_reaches_the_auth_ports` below locks the other half — that
//! this route is mounted outside the Basic Auth middleware rather than
//! special-cased inside it.

use axum::extract::State;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use tokio_util::sync::CancellationToken;

/// `GET /healthz` — constant-cost liveness for the mobile LAN listener.
///
/// Returns `204 No Content` while the listener is serving, or `503` with
/// `Retry-After: 1` once cancellation has been signalled but the socket has
/// not finished draining. The draining verdict reads the in-memory token the
/// listener already holds for `with_graceful_shutdown`; it introduces no new
/// state and touches no persistence.
///
/// `no-store` is mandatory on both paths: a proxy that cached a `204` would
/// report a dead daemon as alive for the life of the cache entry, which is
/// precisely the failure this endpoint exists to detect.
pub(super) async fn get_healthz(State(cancel): State<CancellationToken>) -> Response {
    if cancel.is_cancelled() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            [
                (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
                (header::RETRY_AFTER, HeaderValue::from_static("1")),
            ],
        )
            .into_response();
    }

    (
        StatusCode::NO_CONTENT,
        [(header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    //! Health endpoint behaviour + the structural guarantee that it sits
    //! outside Basic Auth.
    //!
    //! These build the full `build_router` assembly rather than a bare
    //! `Router::new().route(...)`, because the property under test is about
    //! how the route is *mounted*, not what the handler returns in isolation.

    use std::sync::Arc;

    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use uc_application::facade::MobileSyncFacade;

    use crate::mobile_lan::routes::build_router;
    use crate::mobile_lan::test_support::{
        auth_header, build_facade_with_auth_trap, build_facade_with_seeded_device,
        fake_cancel_token, fake_sse_source,
    };

    fn app_with(
        facade: Arc<MobileSyncFacade>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> axum::Router {
        build_router(facade, None, fake_sse_source(), cancel)
    }

    async fn seeded_app() -> axum::Router {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        app_with(facade, fake_cancel_token())
    }

    fn get_healthz_request() -> Request<Body> {
        Request::builder()
            .method("GET")
            .uri("/healthz")
            .body(Body::empty())
            .unwrap()
    }

    #[tokio::test]
    async fn unauthenticated_healthz_returns_204_no_store_and_empty_body() {
        let resp = seeded_app()
            .await
            .oneshot(get_healthz_request())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            resp.headers().get("cache-control").unwrap(),
            "no-store",
            "a cached 204 would report a dead daemon as alive"
        );
        let body = to_bytes(resp.into_body(), 64).await.unwrap();
        assert!(
            body.is_empty(),
            "health response must not carry identity, version, or business state"
        );
    }

    #[tokio::test]
    async fn healthz_ignores_authorization_header() {
        let resp = seeded_app()
            .await
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/healthz")
                    .header("Authorization", auth_header("mobile_alice", "WRONG"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "credentials are not consulted, so wrong ones cannot produce a 401 here"
        );
    }

    /// The structural lock: a facade whose Basic Auth ports fail on contact.
    ///
    /// This fails loudly if anyone re-mounts `/healthz` under the auth
    /// middleware — including via a `/healthz` special-case *inside* it, which
    /// a status-code-only assertion would happily pass. The request carries a
    /// well-formed `Authorization` header so that auth, if reached at all, gets
    /// past header parsing and into the trapped device lookup.
    #[tokio::test]
    async fn healthz_never_reaches_the_auth_ports() {
        let resp = app_with(build_facade_with_auth_trap().await, fake_cancel_token())
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/healthz")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    /// The counterpart to the trap test: the same trapped facade proves the
    /// protected routes *do* still run auth, so the split above cannot be
    /// passing merely because the middleware got dropped entirely.
    #[tokio::test]
    async fn protected_routes_still_reach_the_auth_ports() {
        let resp = app_with(build_facade_with_auth_trap().await, fake_cancel_token())
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/SyncClipboard.json")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            resp.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "the trapped device repo should surface as a 500 from the auth middleware"
        );
    }

    #[tokio::test]
    async fn draining_listener_returns_503_with_retry_after() {
        let cancel = fake_cancel_token();
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let app = app_with(facade, cancel.clone());
        cancel.cancel();

        let resp = app.oneshot(get_healthz_request()).await.unwrap();

        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(resp.headers().get("retry-after").unwrap(), "1");
        assert_eq!(resp.headers().get("cache-control").unwrap(), "no-store");
        let body = to_bytes(resp.into_body(), 64).await.unwrap();
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn non_get_methods_return_405() {
        for method in ["POST", "PUT", "DELETE", "PATCH"] {
            let resp = seeded_app()
                .await
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri("/healthz")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(
                resp.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} /healthz should be rejected by the method router"
            );
        }
    }

    /// axum's `get()` answers HEAD from the same handler with the body
    /// stripped. v1 does not require HEAD, but if the framework offers it the
    /// response must stay body-free and side-effect-free.
    #[tokio::test]
    async fn head_healthz_is_body_free() {
        let resp = seeded_app()
            .await
            .oneshot(
                Request::builder()
                    .method("HEAD")
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let body = to_bytes(resp.into_body(), 64).await.unwrap();
        assert!(body.is_empty());
    }

    /// A probe at a 2s cadence runs ~43k times a day per client. If each one
    /// left an INFO behind, the recovery path would quietly turn into a log
    /// writer — which is half the reason `/SyncClipboard.json` is unfit as a
    /// probe in the first place. Counting events at the subscriber is the
    /// direct proof; a load test can only show the log file growing.
    ///
    /// ## Why the subscriber setup looks like this
    ///
    /// tracing caches each callsite's interest in a **process-global** slot.
    /// In a full `cargo test` run, sibling test threads reach the auth
    /// middleware's `info!` callsites first with no subscriber installed,
    /// which caches them as `NEVER` — and the read path short-circuits on
    /// `NEVER` without ever re-consulting a subscriber. The `info!` then
    /// compiles down to a no-op and this test would count a meaningless zero.
    ///
    /// So: install the subscriber thread-locally, touch the callsites once to
    /// force them into the global registry, *then* rebuild the cache. Order
    /// matters — a rebuild only revisits callsites already registered, so
    /// rebuilding first leaves an untouched callsite free to be registered
    /// later by a sibling thread and cached `NEVER` all over again. The
    /// rebuild's single-dispatcher path resolves through `get_default`, which
    /// honours the thread-local, re-marking these callsites `SOMETIMES` —
    /// after which each event consults `enabled` against its own thread's
    /// dispatch. Sibling threads still see their own (absent) subscriber, so
    /// this neither leaks their events in nor perturbs them.
    ///
    /// The runtime must stay single-threaded (`tokio::test`'s default) so the
    /// thread-local dispatch covers every poll of the futures below.
    ///
    /// The "counting works" assertion below guards all of this: if the
    /// mechanism silently stops working, that fails rather than letting the
    /// real assertion pass vacuously.
    #[tokio::test]
    async fn a_thousand_probes_emit_no_info_or_warn_logs() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tracing::Level;

        #[derive(Clone, Default)]
        struct CountingSubscriber {
            noisy: Arc<AtomicUsize>,
        }

        impl tracing::Subscriber for CountingSubscriber {
            fn enabled(&self, md: &tracing::Metadata<'_>) -> bool {
                *md.level() <= Level::INFO
            }
            fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
                tracing::span::Id::from_u64(1)
            }
            fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
            fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
            fn event(&self, event: &tracing::Event<'_>) {
                if *event.metadata().level() <= Level::INFO {
                    self.noisy.fetch_add(1, Ordering::Relaxed);
                }
            }
            fn enter(&self, _: &tracing::span::Id) {}
            fn exit(&self, _: &tracing::span::Id) {}
        }

        let counter = Arc::new(AtomicUsize::new(0));
        let app = seeded_app().await;

        let _guard = tracing::subscriber::set_default(CountingSubscriber {
            noisy: counter.clone(),
        });

        // An authenticated call to a protected route: it registers the auth
        // middleware's INFO callsites (see above) and doubles as the proof
        // that the counter can count — the very logging /healthz must avoid.
        let authenticated_sync_call = || {
            app.clone().oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/SyncClipboard.json")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
        };

        let resp = authenticated_sync_call().await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        tracing::callsite::rebuild_interest_cache();

        counter.store(0, Ordering::Relaxed);
        let resp = authenticated_sync_call().await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            counter.load(Ordering::Relaxed) > 0,
            "one authenticated SyncClipboard.json call must emit the per-request \
             INFO logs that /healthz avoids — if it does not, the subscriber is \
             not observing events and the real assertion below proves nothing"
        );

        counter.store(0, Ordering::Relaxed);
        for _ in 0..1000 {
            let resp = app.clone().oneshot(get_healthz_request()).await.unwrap();
            assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        }

        assert_eq!(
            counter.load(Ordering::Relaxed),
            0,
            "1000 health probes must not emit a single INFO/WARN event"
        );
    }
}
