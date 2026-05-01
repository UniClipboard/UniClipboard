//! Iroh-backed implementation of [`PresencePort`] (Slice 2 Phase 1 · T3b).
//!
//! ## Design summary
//!
//! T3a's probe (see `uc-infra/tests/iroh_presence_probe.rs`) established two
//! load-bearing facts about iroh 0.95:
//!
//! 1. [`iroh::Endpoint::conn_type`] is a **cache**, not a liveness probe.
//!    It keeps returning `Direct(SocketAddr)` for seconds after the peer
//!    tears its endpoint down. Using it as an "offline" signal misses the
//!    Phase 1 budget (≤ 10 s) by a wide margin.
//! 2. [`iroh::endpoint::Connection::closed`] resolves within ~100 ms of the
//!    peer disappearing on loopback. This is the reliable offline signal.
//!
//! The adapter therefore:
//!
//! * Holds every successfully-dialed [`Connection`] alive inside a
//!   [`TrackedPeer`] entry keyed by [`DeviceId`].
//! * Spawns a **watchdog task per tracked peer** that awaits
//!   `connection.closed()` and, on completion, removes the entry and
//!   broadcasts a `PresenceEvent { state: Offline, .. }`.
//! * Exposes a second "last observed state" map so `current_state` can
//!   return `Offline` for a peer whose dial failed (that peer is *not* in
//!   the tracked map). `current_state` therefore reads from the last-state
//!   cache first, falling back to the tracked-connection map, and only
//!   yielding `Unknown` when neither knows anything.
//!
//! ## ALPN
//!
//! [`PRESENCE_ALPN`] = `uniclipboard/presence/0`. The accept side runs
//! [`IrohPresenceHandler`], which holds each incoming connection open until
//! the peer closes it — mirroring `spawn_hold_open_acceptor` in the probe.
//! The dial side is invoked from [`IrohPresenceAdapter::ensure_reachable`].

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointAddr};
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, info, instrument, warn};

use uc_core::ids::DeviceId;
use uc_core::ports::{
    ClockPort, PeerAddressRepositoryPort, PresenceError, PresenceEvent, PresencePort,
    ReachabilityState,
};

use super::connect::connect_with_staggered_retry;

/// ALPN identifier for the Slice 2 presence protocol. The accept-side
/// handler performs no application-level handshake — its sole job is to
/// keep the connection open so the dial-side watchdog can observe peer
/// teardown via [`Connection::closed`].
pub const PRESENCE_ALPN: &[u8] = b"uniclipboard/presence/0";

/// Capacity of the [`broadcast`] channel that fans `PresenceEvent`s out to
/// subscribers. 64 sits comfortably above expected burst width (N ≤ 10
/// members flipping state on an unlock); lagging subscribers recover via
/// [`PresencePort::current_state`] per the broadcast contract.
const EVENT_CHANNEL_CAPACITY: usize = 64;

// ============================================================================
// ProtocolHandler (accept side)
// ============================================================================

/// Accept-side handler for [`PRESENCE_ALPN`].
///
/// Holds each inbound connection open until the peer closes it. No frames
/// are read — the whole point of the presence protocol is that the liveness
/// signal is the QUIC connection itself, observed by the dial-side via
/// [`Connection::closed`].
#[derive(Clone, Default)]
pub struct IrohPresenceHandler;

impl std::fmt::Debug for IrohPresenceHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrohPresenceHandler")
            .finish_non_exhaustive()
    }
}

impl IrohPresenceHandler {
    pub fn new() -> Self {
        Self
    }
}

impl ProtocolHandler for IrohPresenceHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let remote = connection.remote_id();
        debug!(remote = %remote, "presence connection accepted; holding open until peer closes");
        let reason = connection.closed().await;
        debug!(
            remote = %remote,
            reason = ?reason,
            "presence connection closed by peer",
        );
        Ok(())
    }
}

// ============================================================================
// Adapter (dial side)
// ============================================================================

/// Iroh-backed [`PresencePort`] implementation.
pub struct IrohPresenceAdapter {
    endpoint: Arc<Endpoint>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    clock: Arc<dyn ClockPort>,
    /// Live iroh connections keyed by `DeviceId` serialised as `String` —
    /// `uc_core::ids::DeviceId` deliberately does not derive `Hash`, so
    /// the adapter projects it down to its stringified form for map keys.
    /// `DeviceId` is reconstructed via `DeviceId::new` at the event
    /// broadcast boundary so the port contract stays strongly typed.
    peers: Arc<Mutex<HashMap<String, TrackedPeer>>>,
    /// Remember the last observed outcome for every device the adapter has
    /// ever probed. Distinct from `peers` because a failed dial should
    /// surface as `Offline` on `current_state` without leaving a live
    /// connection entry behind.
    last_state: Arc<Mutex<HashMap<String, ReachabilityState>>>,
    event_tx: broadcast::Sender<PresenceEvent>,
}

/// Per-device bookkeeping: the live connection we hold open, plus the
/// watchdog task that awaits its demise.
struct TrackedPeer {
    connection: Connection,
    watchdog: JoinHandle<()>,
}

impl Drop for TrackedPeer {
    fn drop(&mut self) {
        // Dropping the connection is the caller's signal to close; aborting
        // the watchdog prevents it from racing on the now-dropped entry.
        self.watchdog.abort();
    }
}

impl IrohPresenceAdapter {
    /// Construct an adapter wired to the given iroh endpoint, peer address
    /// repository, and clock. Returns an owned value; the caller wraps it
    /// in `Arc` before publishing it as `Arc<dyn PresencePort>` so shutdown
    /// semantics match the rest of the iroh adapter family.
    pub fn new(
        endpoint: Arc<Endpoint>,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        Self {
            endpoint,
            peer_addr_repo,
            clock,
            peers: Arc::new(Mutex::new(HashMap::new())),
            last_state: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
        }
    }

    fn now(&self) -> DateTime<Utc> {
        let ms = self.clock.now_ms();
        // `Utc.timestamp_millis_opt` rejects out-of-range values. Any
        // ClockPort implementation feeding out-of-range epoch millis is a
        // defect, but there is no recourse from this code path — fall back
        // to the current wall clock so presence timestamps stay monotonic
        // rather than panic the watchdog.
        match Utc.timestamp_millis_opt(ms).single() {
            Some(dt) => dt,
            None => {
                warn!(
                    ms,
                    "ClockPort returned out-of-range epoch millis; falling back to Utc::now"
                );
                Utc::now()
            }
        }
    }

    fn broadcast(&self, device_id: DeviceId, state: ReachabilityState, at: DateTime<Utc>) {
        // Ignoring `SendError` is intentional: a `broadcast::Sender::send`
        // failure just means no one is subscribed yet. Subscribers catch up
        // via `current_state` which is always in sync with `last_state`.
        let _ = self.event_tx.send(PresenceEvent {
            device_id,
            state,
            at,
        });
    }
}

#[async_trait]
impl PresencePort for IrohPresenceAdapter {
    #[instrument(skip_all, fields(device = %device.as_str()))]
    async fn ensure_reachable(
        &self,
        device: &DeviceId,
    ) -> Result<ReachabilityState, PresenceError> {
        let key = device.as_str().to_string();

        // Step 1: fast-path on an already-tracked live connection.
        {
            let mut peers = self.peers.lock().await;
            if let Some(entry) = peers.get(&key) {
                if entry.connection.close_reason().is_none() {
                    debug!("ensure_reachable: already tracked and alive");
                    return Ok(ReachabilityState::Online);
                }
                // Stale entry — the watchdog should already be in the
                // process of cleaning up, but don't block on it. Remove
                // here so the re-dial path below gets a clean slate.
                if let Some(stale) = peers.remove(&key) {
                    stale.watchdog.abort();
                    debug!("ensure_reachable: evicted stale tracked entry before re-dial");
                }
            }
        }

        // Step 2: look up the stored transport address.
        let record = self
            .peer_addr_repo
            .get(device)
            .await
            .map_err(|err| PresenceError::Internal(format!("peer_addr_repo.get: {err}")))?;
        let record = match record {
            Some(r) => r,
            None => {
                debug!("ensure_reachable: no address record; returning NoAddress");
                return Err(PresenceError::NoAddress(device.clone()));
            }
        };

        // Step 3: decode the opaque blob into the adapter-private
        // `EndpointAddr`. Failure is a data-integrity issue (someone wrote
        // junk into the repo) — surface it as `Internal` without leaking
        // the postcard error type upward. The blob is guaranteed to have
        // had its ephemeral `Ip(...)` direct addresses stripped at
        // pairing-time write (see `persistable_addr::to_persistable_addr`),
        // so what we decode here is `id + Relay(...)`. iroh's built-in
        // pkarr discovery fills in fresh direct addrs at connect time.
        let endpoint_addr: EndpointAddr =
            postcard::from_bytes(&record.addr_blob).map_err(|err| {
                PresenceError::Internal(format!("postcard decode EndpointAddr: {err}"))
            })?;

        // Step 4: dial.
        match connect_with_staggered_retry(
            Arc::clone(&self.endpoint),
            endpoint_addr,
            PRESENCE_ALPN,
            "presence",
        )
        .await
        {
            Ok(connection) => {
                let now = self.now();
                let device_id_for_watchdog = device.clone();
                let peers_for_watchdog = Arc::clone(&self.peers);
                let last_state_for_watchdog = Arc::clone(&self.last_state);
                let event_tx_for_watchdog = self.event_tx.clone();
                let clock_for_watchdog = Arc::clone(&self.clock);
                let connection_for_watchdog = connection.clone();

                let watchdog = spawn_watchdog(
                    peers_for_watchdog,
                    last_state_for_watchdog,
                    event_tx_for_watchdog,
                    clock_for_watchdog,
                    device_id_for_watchdog,
                    connection_for_watchdog,
                );

                {
                    let mut peers = self.peers.lock().await;
                    // If someone else raced us to insert, abort our own
                    // watchdog and keep theirs — the winner gets to manage
                    // the single connection slot per device. This is a
                    // defensive branch; `ensure_reachable` should never be
                    // concurrently called for the same device in Slice 2.
                    if let Some(existing) = peers.get(&key) {
                        if existing.connection.close_reason().is_none() {
                            warn!(
                                "ensure_reachable: concurrent insert detected; \
                                 discarding freshly dialed connection",
                            );
                            watchdog.abort();
                            drop(connection);
                            return Ok(ReachabilityState::Online);
                        }
                    }
                    peers.insert(
                        key.clone(),
                        TrackedPeer {
                            connection,
                            watchdog,
                        },
                    );
                }

                {
                    let mut last = self.last_state.lock().await;
                    last.insert(key.clone(), ReachabilityState::Online);
                }
                info!("ensure_reachable: dial succeeded, peer marked Online");
                self.broadcast(device.clone(), ReachabilityState::Online, now);
                Ok(ReachabilityState::Online)
            }
            Err(err) => {
                // No iroh error type leaks upward — per `uc-infra/AGENTS.md`
                // §9.1 the failure is summarised into `last_state` + an
                // event. The member stays in the repo; the next
                // `ensure_reachable` retry is how recovery happens.
                debug!(error = %err, "ensure_reachable: dial failed, peer marked Offline");
                let now = self.now();
                {
                    let mut last = self.last_state.lock().await;
                    last.insert(key, ReachabilityState::Offline);
                }
                self.broadcast(device.clone(), ReachabilityState::Offline, now);
                Ok(ReachabilityState::Offline)
            }
        }
    }

    #[instrument(skip_all, fields(device = %device.as_str()))]
    async fn current_state(&self, device: &DeviceId) -> ReachabilityState {
        let key = device.as_str();
        // Prefer the last-observed snapshot — it's authoritative for
        // `Offline` (which is not represented in `peers`) and strictly
        // consistent with the live-connection map for `Online` because
        // `ensure_reachable` and the watchdog update both under lock.
        if let Some(state) = self.last_state.lock().await.get(key).copied() {
            return state;
        }
        // Fall back to the tracked-connection map in case something
        // bypassed `last_state` bookkeeping. Under the current API surface
        // this branch is unreachable, but the check is cheap.
        let peers = self.peers.lock().await;
        match peers.get(key) {
            Some(entry) if entry.connection.close_reason().is_none() => ReachabilityState::Online,
            Some(_) => ReachabilityState::Offline,
            None => ReachabilityState::Unknown,
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<PresenceEvent> {
        self.event_tx.subscribe()
    }
}

// ============================================================================
// Watchdog
// ============================================================================

/// Spawn the per-peer watchdog task.
///
/// The task awaits `connection.closed()` — the reliable offline signal
/// established by T3a — then:
///
/// * Removes the `TrackedPeer` entry (which aborts the watchdog's own
///   `JoinHandle` via `Drop`, but since we're the watchdog itself at that
///   point the abort is a no-op).
/// * Writes `Offline` into the `last_state` cache.
/// * Broadcasts a `PresenceEvent { state: Offline, .. }`.
///
/// Errors on the broadcast send are ignored (no subscriber is a valid
/// state; consumers recover via `current_state`).
fn spawn_watchdog(
    peers: Arc<Mutex<HashMap<String, TrackedPeer>>>,
    last_state: Arc<Mutex<HashMap<String, ReachabilityState>>>,
    event_tx: broadcast::Sender<PresenceEvent>,
    clock: Arc<dyn ClockPort>,
    device_id: DeviceId,
    connection: Connection,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let reason = connection.closed().await;
        info!(
            device = %device_id.as_str(),
            reason = ?reason,
            "presence watchdog fired; peer marked Offline",
        );

        let key = device_id.as_str().to_string();

        // Remove the map entry first so concurrent `ensure_reachable`
        // readers observe "not tracked" + "last_state == Offline". The
        // `TrackedPeer::drop` impl will attempt to abort this very task,
        // which is harmless — we're already past the `.await` point.
        {
            let mut map = peers.lock().await;
            map.remove(&key);
        }

        let ms = clock.now_ms();
        let at = Utc
            .timestamp_millis_opt(ms)
            .single()
            .unwrap_or_else(Utc::now);

        {
            let mut last = last_state.lock().await;
            last.insert(key, ReachabilityState::Offline);
        }

        let _ = event_tx.send(PresenceEvent {
            device_id,
            state: ReachabilityState::Offline,
            at,
        });
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap as StdHashMap;
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    use chrono::Utc;
    use iroh::protocol::Router;
    use iroh::RelayMode;
    use tokio::time::timeout;

    use uc_core::ids::DeviceId;
    use uc_core::ports::{PeerAddressError, PeerAddressRecord};

    const DIAL_BUDGET: Duration = Duration::from_secs(5);
    const OFFLINE_BUDGET: Duration = Duration::from_secs(10);

    // -- Fakes ---------------------------------------------------------------

    #[derive(Default)]
    struct FakePeerAddressRepo {
        inner: StdMutex<StdHashMap<String, PeerAddressRecord>>,
    }

    impl FakePeerAddressRepo {
        fn seed(&self, record: PeerAddressRecord) {
            self.inner
                .lock()
                .unwrap()
                .insert(record.device_id.as_str().to_string(), record);
        }
    }

    #[async_trait]
    impl PeerAddressRepositoryPort for FakePeerAddressRepo {
        async fn get(
            &self,
            device: &DeviceId,
        ) -> Result<Option<PeerAddressRecord>, PeerAddressError> {
            Ok(self.inner.lock().unwrap().get(device.as_str()).cloned())
        }

        async fn upsert(&self, record: &PeerAddressRecord) -> Result<(), PeerAddressError> {
            self.inner
                .lock()
                .unwrap()
                .insert(record.device_id.as_str().to_string(), record.clone());
            Ok(())
        }

        async fn list(&self) -> Result<Vec<PeerAddressRecord>, PeerAddressError> {
            Ok(self.inner.lock().unwrap().values().cloned().collect())
        }

        async fn remove(&self, device: &DeviceId) -> Result<(), PeerAddressError> {
            self.inner.lock().unwrap().remove(device.as_str());
            Ok(())
        }
    }

    struct FixedClock;
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            // 2026-01-01T00:00:00Z — chosen so `at` is always the same in
            // every test for easy assertions.
            1_767_225_600_000
        }
    }

    // -- Helpers -------------------------------------------------------------

    async fn bound_endpoint() -> Arc<Endpoint> {
        // Discovery is cleared so dials must rely solely on the
        // `EndpointAddr` blob: an empty `transport_addrs` with relays
        // disabled then has no fallback, which is what
        // `ensure_reachable_after_offline_redials_successfully` relies on.
        // Without this, iroh's default n0/pkarr DNS discovery can resolve
        // the live peer's id back to its real direct addrs and the dial
        // unexpectedly succeeds on environments with outbound DNS (CI).
        Arc::new(
            Endpoint::builder(iroh::endpoint::presets::N0)
                .alpns(vec![PRESENCE_ALPN.to_vec()])
                .relay_mode(RelayMode::Disabled)
                .clear_address_lookup()
                .bind()
                .await
                .expect("bind endpoint"),
        )
    }

    async fn wait_for_direct_addrs(endpoint: &Endpoint) {
        for _ in 0..100 {
            if !endpoint.addr().addrs.is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("endpoint never published direct addresses");
    }

    /// Build endpoint A (dialer), endpoint B (acceptor) with a spawned
    /// `Router` registering [`IrohPresenceHandler`] on [`PRESENCE_ALPN`].
    /// Returns both endpoints, B's encoded blob for the repo, B's
    /// `DeviceId`, and B's `Router` so the test can shut it down later.
    async fn setup_two_endpoints() -> (Arc<Endpoint>, Arc<Endpoint>, Vec<u8>, DeviceId, Router) {
        let endpoint_b = bound_endpoint().await;
        wait_for_direct_addrs(&endpoint_b).await;
        let b_addr = endpoint_b.addr();
        let b_blob = postcard::to_stdvec(&b_addr).expect("postcard encode EndpointAddr");
        let b_device_id = DeviceId::new(format!("endpoint-b-{}", endpoint_b.id().fmt_short()));

        let router_b = Router::builder((*endpoint_b).clone())
            .accept(PRESENCE_ALPN, IrohPresenceHandler::new())
            .spawn();

        let endpoint_a = bound_endpoint().await;
        wait_for_direct_addrs(&endpoint_a).await;

        (endpoint_a, endpoint_b, b_blob, b_device_id, router_b)
    }

    fn record(device: &DeviceId, blob: Vec<u8>) -> PeerAddressRecord {
        PeerAddressRecord {
            device_id: device.clone(),
            addr_blob: blob,
            observed_at: Utc::now(),
        }
    }

    fn build_adapter(
        endpoint: Arc<Endpoint>,
        repo: Arc<dyn PeerAddressRepositoryPort>,
    ) -> IrohPresenceAdapter {
        IrohPresenceAdapter::new(endpoint, repo, Arc::new(FixedClock))
    }

    // -- Tests ---------------------------------------------------------------

    #[tokio::test]
    async fn ensure_reachable_on_known_address_returns_online() {
        let (endpoint_a, endpoint_b, b_blob, b_device_id, router_b) = setup_two_endpoints().await;

        let repo = Arc::new(FakePeerAddressRepo::default());
        repo.seed(record(&b_device_id, b_blob));

        let adapter = build_adapter(endpoint_a.clone(), repo.clone());
        let mut subscriber = adapter.subscribe();

        let state = timeout(DIAL_BUDGET, adapter.ensure_reachable(&b_device_id))
            .await
            .expect("ensure_reachable within budget")
            .expect("ensure_reachable succeeded");
        assert_eq!(state, ReachabilityState::Online);

        assert_eq!(
            adapter.current_state(&b_device_id).await,
            ReachabilityState::Online,
        );

        let event = timeout(Duration::from_secs(1), subscriber.recv())
            .await
            .expect("subscriber received within 1s")
            .expect("event channel not closed");
        assert_eq!(event.device_id, b_device_id);
        assert_eq!(event.state, ReachabilityState::Online);

        // Teardown.
        router_b.shutdown().await.expect("router_b shutdown clean");
        endpoint_a.close().await;
        drop(endpoint_b);
    }

    #[tokio::test]
    async fn ensure_reachable_on_unknown_device_returns_no_address() {
        let endpoint_a = bound_endpoint().await;
        let repo = Arc::new(FakePeerAddressRepo::default());
        let adapter = build_adapter(endpoint_a.clone(), repo);

        let ghost = DeviceId::new("device-with-no-record");
        match adapter.ensure_reachable(&ghost).await {
            Err(PresenceError::NoAddress(id)) => assert_eq!(id.as_str(), ghost.as_str()),
            other => panic!("expected NoAddress, got {other:?}"),
        }

        endpoint_a.close().await;
    }

    #[tokio::test]
    async fn peer_shutdown_triggers_offline_event_within_budget() {
        let (endpoint_a, endpoint_b, b_blob, b_device_id, router_b) = setup_two_endpoints().await;

        let repo = Arc::new(FakePeerAddressRepo::default());
        repo.seed(record(&b_device_id, b_blob));

        let adapter = build_adapter(endpoint_a.clone(), repo);
        let mut subscriber = adapter.subscribe();

        let state = adapter
            .ensure_reachable(&b_device_id)
            .await
            .expect("initial dial succeeded");
        assert_eq!(state, ReachabilityState::Online);

        // Drain the Online event before we force teardown so the next
        // `subscriber.recv()` is guaranteed to be the Offline transition.
        let first = timeout(Duration::from_secs(1), subscriber.recv())
            .await
            .expect("initial online event arrives")
            .expect("event channel open");
        assert_eq!(first.state, ReachabilityState::Online);

        // Tear the acceptor side down.
        router_b.shutdown().await.expect("router_b shutdown clean");
        endpoint_b.close().await;

        let offline = timeout(OFFLINE_BUDGET, subscriber.recv())
            .await
            .expect("offline event within 10s budget")
            .expect("event channel open");
        assert_eq!(offline.state, ReachabilityState::Offline);
        assert_eq!(offline.device_id, b_device_id);

        assert_eq!(
            adapter.current_state(&b_device_id).await,
            ReachabilityState::Offline,
        );

        endpoint_a.close().await;
    }

    #[tokio::test]
    async fn current_state_defaults_to_unknown_before_probe() {
        let endpoint_a = bound_endpoint().await;
        let repo = Arc::new(FakePeerAddressRepo::default());
        let adapter = build_adapter(endpoint_a.clone(), repo);

        let never_seen = DeviceId::new("never-probed");
        assert_eq!(
            adapter.current_state(&never_seen).await,
            ReachabilityState::Unknown,
        );

        endpoint_a.close().await;
    }

    #[tokio::test]
    async fn ensure_reachable_after_offline_redials_successfully() {
        // Simpler coverage for the recovery half: dial against a peer
        // whose addr record points at a well-formed but unreachable
        // `EndpointAddr` (no route back), observe `Offline`, then swap the
        // repo entry for a live peer and redial — expect `Online`.
        //
        // This sidesteps the iroh-secret-identity plumbing that a full
        // restart-on-same-endpoint test would need (keypairs are not
        // rebindable once an endpoint is dropped). See plan §8 for the
        // test-strategy note.
        let (endpoint_a, endpoint_b, b_blob, b_device_id, router_b) = setup_two_endpoints().await;

        let repo = Arc::new(FakePeerAddressRepo::default());

        // Seed with an unroutable address first: craft an `EndpointAddr`
        // whose id is B's but whose transport addr list is empty (relays
        // disabled → no fallback → dial fails quickly).
        let dead_addr = EndpointAddr::new(endpoint_b.id());
        let dead_blob = postcard::to_stdvec(&dead_addr).expect("encode");
        repo.seed(record(&b_device_id, dead_blob));

        let adapter = build_adapter(endpoint_a.clone(), repo.clone());

        let first = timeout(OFFLINE_BUDGET, adapter.ensure_reachable(&b_device_id))
            .await
            .expect("dial resolves within budget")
            .expect("ensure_reachable completed (Offline is Ok)");
        assert_eq!(first, ReachabilityState::Offline);
        assert_eq!(
            adapter.current_state(&b_device_id).await,
            ReachabilityState::Offline,
        );

        // Now swap in the live blob and redial.
        repo.seed(record(&b_device_id, b_blob));
        let second = timeout(DIAL_BUDGET, adapter.ensure_reachable(&b_device_id))
            .await
            .expect("re-dial within budget")
            .expect("re-dial succeeded");
        assert_eq!(second, ReachabilityState::Online);
        assert_eq!(
            adapter.current_state(&b_device_id).await,
            ReachabilityState::Online,
        );

        router_b.shutdown().await.expect("router_b shutdown clean");
        endpoint_a.close().await;
    }
}
