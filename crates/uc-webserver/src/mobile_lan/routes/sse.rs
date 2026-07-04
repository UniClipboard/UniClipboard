//! `GET /api/sse/clipboard` — notify-then-pull push channel for mobile-sync.
//!
//! This endpoint carries **signals only**: `{content_id, server_time_ms}`.
//! It never carries clipboard content and never performs dedupe/LWW — the
//! client reacts to every signal by calling the existing pull endpoint
//! (`GET /SyncClipboard.json`) unconditionally. There is no event `seq` and
//! no replay on reconnect: a client that reconnects always does one
//! unconditional pull, which is why the connect handshake orders
//! `subscribe()` strictly before the `hello` frame (see below) — any update
//! published in that narrow window still lands in the receiver's buffer and
//! is delivered as a normal `update` frame instead of being silently missed.
//!
//! Four independent reasons can end the stream, merged in one `select!` per
//! iteration:
//! 1. the connection-scoped cancel token (listener shutdown / eviction);
//! 2. the periodic credential re-check failing (device deleted / password
//!    rotated) — this endpoint does not gate on the mobile-sync feature
//!    toggles, only on credential validity;
//! 3. the broadcast source closing (process-lifetime event, should not
//!    happen in practice);
//! 4. never on `Lagged` — a lagged receiver sends a `resync` frame instead
//!    of tearing down the connection.
//!
//! Event names and payload shapes come from [`uc_mobile_proto::sse_event`],
//! the single source of truth shared with the `uc-mobile` client.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Extension, State};
use axum::http::{header, HeaderName, HeaderValue};
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::{self, Stream, StreamExt};
use serde::Serialize;
use tokio::sync::broadcast;
use tokio::time::{Instant, Interval, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use uc_application::facade::{AuthenticatedDevice, MobileSyncFacade};
use uc_core::clipboard::ActiveClipboardState;
use uc_core::mobile_sync::MobileDeviceId;
use uc_mobile_proto::sse_event::{
    SseHello, SseResync, SseUpdate, SSE_EVENT_HELLO, SSE_EVENT_RESYNC, SSE_EVENT_UPDATE,
    SSE_HEARTBEAT_INTERVAL_SECS,
};

use crate::mobile_lan::sse_registry::SseConnectionRegistry;

use super::MobileLanState;

/// Heartbeat comment cadence. Derived from the wire-protocol constant so the
/// `uc-mobile` client's dead-connection timeout (2× this) cannot drift from
/// what this server actually sends.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(SSE_HEARTBEAT_INTERVAL_SECS);

/// Periodic credential re-check cadence. Each tick verifies the device still
/// exists and its stored password hash matches the one captured at connect
/// time ([`MobileSyncFacade::is_device_credential_current`]) — a repo read
/// plus a string compare, NOT an Argon2 verify, so this stays cheap per
/// connection. It does not check the mobile-sync feature toggles, since
/// those are the listener's cancel-token's job, not this connection-scoped
/// check's.
const REVALIDATE_INTERVAL: Duration = Duration::from_secs(30);

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Build a JSON-payload SSE event, falling back to an empty object if the
/// (internal, always-simple) payload somehow fails to serialize rather than
/// panicking the connection task.
fn event_json(name: &'static str, payload: &impl Serialize) -> Event {
    match serde_json::to_string(payload) {
        Ok(json) => Event::default().event(name).data(json),
        Err(err) => {
            tracing::error!(error = %err, event = name, "mobile_lan sse: failed to serialize event payload");
            Event::default().event(name).data("{}")
        }
    }
}

/// A periodic timer whose first tick is `period` after creation (not
/// immediate) — `tokio::time::interval`'s default first-tick-fires-now
/// behavior would otherwise fire a spurious heartbeat/re-check right at
/// connect time.
fn periodic(period: Duration) -> Interval {
    let mut interval = tokio::time::interval_at(Instant::now() + period, period);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    interval
}

/// Unregisters the connection's registry entry when dropped — covers both
/// the normal end-of-stream path and an early client disconnect (axum drops
/// the response body stream without giving handler code an explicit
/// callback for that case). `unregister` is idempotent against having
/// already been evicted by [`SseConnectionRegistry::register`], so a drop
/// racing a concurrent eviction is harmless.
struct RegistrationGuard {
    registry: Arc<SseConnectionRegistry>,
    device_id: MobileDeviceId,
    conn_id: uuid::Uuid,
}

impl Drop for RegistrationGuard {
    fn drop(&mut self) {
        self.registry.unregister(&self.device_id, self.conn_id);
    }
}

struct SseStreamState {
    rx: broadcast::Receiver<ActiveClipboardState>,
    cancel: CancellationToken,
    heartbeat: Interval,
    revalidate: Interval,
    facade: Arc<MobileSyncFacade>,
    device_id: MobileDeviceId,
    /// The device's Argon2 PHC string as it was at connect time; the periodic
    /// re-check compares the stored hash against this to detect rotation.
    connect_password_hash: String,
    device_username: String,
    // Never read — held purely so `Drop` fires exactly when this state
    // (and thus the connection) goes away.
    _registration: RegistrationGuard,
}

async fn next_event(mut st: SseStreamState) -> Option<(Result<Event, Infallible>, SseStreamState)> {
    loop {
        tokio::select! {
            biased;

            () = st.cancel.cancelled() => {
                tracing::info!(device = %st.device_username, "mobile_lan sse: cancelled, ending stream");
                return None;
            }

            recv = st.rx.recv() => {
                match recv {
                    Ok(state) => {
                        let ev = event_json(SSE_EVENT_UPDATE, &SseUpdate {
                            content_id: state.snapshot_hash,
                            server_time_ms: state.activated_at_ms,
                        });
                        return Some((Ok(ev), st));
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(device = %st.device_username, lagged = n, "mobile_lan sse: receiver lagged, sending resync");
                        let ev = event_json(SSE_EVENT_RESYNC, &SseResync { server_time_ms: now_ms() });
                        return Some((Ok(ev), st));
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        tracing::warn!(device = %st.device_username, "mobile_lan sse: broadcast source closed, ending stream");
                        return None;
                    }
                }
            }

            _ = st.heartbeat.tick() => {
                return Some((Ok(Event::default().comment("ping")), st));
            }

            _ = st.revalidate.tick() => {
                match st.facade.is_device_credential_current(&st.device_id, &st.connect_password_hash).await {
                    Ok(true) => continue,
                    Ok(false) => {
                        tracing::info!(device = %st.device_username, "mobile_lan sse: device revoked or credentials rotated, ending stream");
                        return None;
                    }
                    // Fail closed on a repo error, same as the pre-check
                    // behavior: the client reconnects and re-authenticates.
                    Err(err) => {
                        tracing::warn!(device = %st.device_username, error = %err, "mobile_lan sse: periodic credential re-check failed, ending stream");
                        return None;
                    }
                }
            }
        }
    }
}

pub(super) async fn get_sse_clipboard(
    State(state): State<MobileLanState>,
    Extension(authed): Extension<AuthenticatedDevice>,
) -> Response {
    // F-4: subscribe strictly before sending `hello` — see module docs.
    let rx = state.sse_source.subscribe();
    // Per-connection cancel; listener-wide shutdown cascades to it, and the
    // registry cancels it directly to evict this connection under the
    // per-device cap.
    let cancel = state.cancel.child_token();
    let device_id = authed.device.device_id.clone();
    let device_username = authed.device.username.clone();
    let connect_password_hash = authed.device.password_hash.clone();

    let conn_id = state
        .sse_registry
        .register(device_id.clone(), cancel.clone());
    let registration = RegistrationGuard {
        registry: state.sse_registry.clone(),
        device_id: device_id.clone(),
        conn_id,
    };

    tracing::info!(device = %device_username, "mobile_lan sse: connection established");

    let hello = event_json(
        SSE_EVENT_HELLO,
        &SseHello {
            server_time_ms: now_ms(),
        },
    );
    let hello_stream = stream::once(async move { Ok::<Event, Infallible>(hello) });

    let body_state = SseStreamState {
        rx,
        cancel,
        heartbeat: periodic(HEARTBEAT_INTERVAL),
        revalidate: periodic(REVALIDATE_INTERVAL),
        facade: state.mobile_sync.clone(),
        _registration: registration,
        device_id,
        connect_password_hash,
        device_username,
    };
    let body_stream: std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> =
        Box::pin(stream::unfold(body_state, next_event));

    let mut response = Sse::new(hello_stream.chain(body_stream)).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(
        HeaderName::from_static("x-accel-buffering"),
        HeaderValue::from_static("no"),
    );
    response
}

#[cfg(test)]
mod tests {
    //! Connection-level smoke tests. Exhaustive coverage (lagged/resync,
    //! periodic re-check failure, eviction, headless parity) is Phase 5 —
    //! this module only pins down the handshake ordering and response
    //! shape while the handler is being built.

    use std::time::Duration;

    use super::{HEARTBEAT_INTERVAL, REVALIDATE_INTERVAL};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::Router;
    use futures_util::StreamExt;
    use tower::ServiceExt;

    use uc_core::ids::{DeviceId, EntryId};

    use crate::mobile_lan::routes::build_router;
    use crate::mobile_lan::test_support::{
        auth_header, build_facade_with_seeded_device, fake_cancel_token, fake_sse_source,
    };

    const TEST_TIMEOUT: Duration = Duration::from_secs(2);

    async fn next_frame(
        stream: &mut (impl futures_util::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Unpin),
    ) -> String {
        let chunk = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("frame within timeout")
            .expect("stream not ended")
            .expect("no transport error");
        String::from_utf8(chunk.to_vec()).expect("SSE frames are UTF-8")
    }

    #[tokio::test]
    async fn hello_then_update_on_advance() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let sse_source = fake_sse_source();
        let cancel = fake_cancel_token();
        let app = build_router(facade, None, sse_source.clone(), cancel);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/sse/clipboard")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        assert_eq!(resp.headers().get("cache-control").unwrap(), "no-cache");
        assert_eq!(resp.headers().get("x-accel-buffering").unwrap(), "no");

        let mut stream = resp.into_body().into_data_stream();

        let hello_frame = next_frame(&mut stream).await;
        assert!(hello_frame.contains("event: hello"), "got {hello_frame:?}");
        assert!(hello_frame.contains("server_time_ms"));

        let state = uc_core::clipboard::ActiveClipboardState::new(
            "blake3v1:aa",
            EntryId::new(),
            42,
            DeviceId::new("dev-a"),
        );
        sse_source.send(state).unwrap();

        let update_frame = next_frame(&mut stream).await;
        assert!(
            update_frame.contains("event: update"),
            "got {update_frame:?}"
        );
        assert!(update_frame.contains("blake3v1:aa"));
        assert!(update_frame.contains("\"server_time_ms\":42"));
    }

    #[tokio::test]
    async fn cancel_ends_the_stream() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let sse_source = fake_sse_source();
        let cancel = fake_cancel_token();
        let app = build_router(facade, None, sse_source, cancel.clone());

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/sse/clipboard")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let mut stream = resp.into_body().into_data_stream();
        let _hello = next_frame(&mut stream).await;

        cancel.cancel();

        let ended = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("stream ends within timeout");
        assert!(ended.is_none(), "cancel must end the stream, got {ended:?}");
    }

    /// A third concurrent connection for the same device must evict the
    /// oldest of the two already-open connections (per-device cap = 2).
    /// Covers Phase 4.2 (eviction) — credential rotation (4.3) rides the
    /// same eviction path since a rotated password just means the next
    /// connect authenticates as the same `device_id`.
    #[tokio::test]
    async fn third_connection_for_same_device_evicts_the_oldest() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let sse_source = fake_sse_source();
        let cancel = fake_cancel_token();
        let app = build_router(facade, None, sse_source, cancel);

        let connect = |app: Router| async move {
            let resp = app
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri("/api/sse/clipboard")
                        .header("Authorization", auth_header("mobile_alice", "wonderland"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            resp.into_body().into_data_stream()
        };

        let mut first = connect(app.clone()).await;
        let _ = next_frame(&mut first).await; // hello
        let mut second = connect(app.clone()).await;
        let _ = next_frame(&mut second).await; // hello

        // Third connection triggers eviction of `first`.
        let mut third = connect(app).await;
        let _ = next_frame(&mut third).await; // hello

        let first_ended = tokio::time::timeout(TEST_TIMEOUT, first.next())
            .await
            .expect("evicted stream ends within timeout");
        assert!(
            first_ended.is_none(),
            "oldest connection must be evicted, got {first_ended:?}"
        );

        // `second` and `third` were not evicted (cap is 2, one eviction per
        // over-cap connect) — dropping them here just closes the sockets;
        // the survivor-stays-alive behavior is exercised end-to-end by
        // `hello_then_update_on_advance`.
        drop(second);
        drop(third);
    }

    /// A receiver that falls behind the broadcast channel's capacity gets a
    /// `resync` frame instead of the connection being torn down (F-3), and
    /// keeps receiving further `update`s afterward.
    #[tokio::test]
    async fn lagged_receiver_gets_resync_and_stays_connected() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        // Deliberately tiny capacity so a handful of unread sends overflows it.
        let (sse_source, _keep_alive_rx) = tokio::sync::broadcast::channel(2);
        let cancel = fake_cancel_token();
        let app = build_router(facade, None, sse_source.clone(), cancel);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/sse/clipboard")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let mut stream = resp.into_body().into_data_stream();
        let _hello = next_frame(&mut stream).await;

        // The handler already subscribed during the request above; these
        // sends overflow the receiver's buffer before it is ever polled.
        for i in 0..5u8 {
            let state = uc_core::clipboard::ActiveClipboardState::new(
                format!("blake3v1:{i}"),
                EntryId::new(),
                i as i64,
                DeviceId::new("dev-a"),
            );
            sse_source.send(state).unwrap();
        }

        let resync_frame = next_frame(&mut stream).await;
        assert!(
            resync_frame.contains("event: resync"),
            "got {resync_frame:?}"
        );
        assert!(resync_frame.contains("server_time_ms"));

        // Connection stays alive — the next buffered send still surfaces.
        let next_frame_text = next_frame(&mut stream).await;
        assert!(
            next_frame_text.contains("event: update"),
            "got {next_frame_text:?}"
        );
    }

    /// The periodic credential re-check ends the stream once the device is
    /// gone (username no longer resolves) — mirrors "device deleted" or
    /// "credentials rotated" from the client's perspective.
    #[tokio::test(start_paused = true)]
    async fn periodic_revalidation_failure_ends_the_stream() {
        let facade = build_facade_with_seeded_device("mobile_alice", "wonderland").await;
        let sse_source = fake_sse_source();
        let cancel = fake_cancel_token();
        // Keep our own sender clone alive — `build_router`'s copy is dropped
        // once `oneshot()` returns, and a closed broadcast channel would end
        // the stream via `RecvError::Closed` before the timers ever get a
        // chance to fire, which is not what this test is exercising.
        let app = build_router(facade.clone(), None, sse_source.clone(), cancel);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/sse/clipboard")
                    .header("Authorization", auth_header("mobile_alice", "wonderland"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let mut stream = resp.into_body().into_data_stream();
        let _hello = next_frame(&mut stream).await;

        facade
            .revoke_device(uc_application::facade::RevokeMobileDeviceInput {
                device_id: uc_core::mobile_sync::MobileDeviceId::new("did_seed"),
            })
            .await
            .expect("seeded device revokes");

        // Advance past the heartbeat tick (consumed as a ping) and then past
        // the revalidation tick, which now fails because the device is gone.
        tokio::time::advance(HEARTBEAT_INTERVAL).await;
        let ping_frame = next_frame(&mut stream).await;
        assert!(ping_frame.contains("ping"), "got {ping_frame:?}");

        tokio::time::advance(REVALIDATE_INTERVAL - HEARTBEAT_INTERVAL).await;
        let ended = tokio::time::timeout(TEST_TIMEOUT, stream.next())
            .await
            .expect("stream ends within timeout");
        assert!(
            ended.is_none(),
            "revalidation failure must end the stream, got {ended:?}"
        );
    }
}
