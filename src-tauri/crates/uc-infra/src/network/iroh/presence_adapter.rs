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
//!
//! ## Inbound-driven Online flip
//!
//! Holding the connection open is necessary but not sufficient: a peer that
//! recovers needs us to mark *it* Online without waiting for our next own
//! dial. The handler therefore reverse-resolves `Connection::remote_id()`
//! into a `DeviceId` (same `IdentityFingerprintFactoryPort` + `MemberRepo`
//! lookup the clipboard receiver uses) and, on a Offline → Online
//! transition, writes `last_state[device]=Online` and broadcasts a single
//! `Online` event. Repeat inbound dials from the same peer (every keepalive
//! tick) are idempotent — they don't re-broadcast.
//!
//! Offline detection is **not** mirrored here: the watchdog on our own
//! outbound `Connection` remains the authoritative offline signal, so
//! inbound `connection.closed()` only logs and returns. This keeps a single
//! source of truth for Offline transitions and avoids state-write races
//! between the watchdog and the inbound handler.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointAddr};
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, info, instrument, warn};

use uc_core::ids::DeviceId;
use uc_core::membership::MemberRepositoryPort;
use uc_core::ports::security::IdentityFingerprintFactoryPort;
use uc_core::ports::{
    ClockPort, PeerAddressRepositoryPort, PresenceError, PresenceEvent, PresencePort,
    ReachabilityState,
};
use uc_core::security::IdentityFingerprint;

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

/// Maximum age of a tracked-connection entry before [`IrohPresenceAdapter::
/// ensure_reachable`] refuses the fast-path and forces a fresh dial.
///
/// Without this guard the fast-path only checks `connection.close_reason().
/// is_none()`. A peer that dies *silently* — process crash without a close
/// frame, NAT mapping expiry, network black-hole — keeps `close_reason`
/// as `None` until quinn's own keep-alive PING run hits `max_idle_timeout`
/// (60s) and tears the connection down. During that window the fast-path
/// lies "Online", `current_state` returns Online, `peer_keepalive` records
/// Online, and `clipboard_sync::dispatch_entry` enrolls the dead peer into
/// fan-out — burning the full `FAN_OUT_DEADLINE = 5s` on every clipboard
/// dispatch until the watchdog finally fires.
///
/// 30s is picked to:
/// * stay above `quinn keep_alive_interval = 15s` so two healthy keepalive
///   PINGs always land inside one TTL window — fast-path hit rate on a
///   live connection is unchanged;
/// * stay above `peer_keepalive::BASE_INTERVAL = 25s` so the worker's
///   normal cadence still mostly observes cached Online and doesn't pay
///   a dial on every tick;
/// * stay below `max_idle_timeout = 60s` so we surface a silently-dead
///   peer *before* quinn's idle timer would, halving the worst-case
///   stale window observable to `dispatch_entry`.
const FAST_PATH_TTL: Duration = Duration::from_secs(30);

// ============================================================================
// ProtocolHandler (accept side)
// ============================================================================

/// Shared state between the dial-side adapter and the accept-side handler.
///
/// Both sides write `last_state` and emit `event_tx` events; sharing a
/// single `Arc<Mutex<HashMap>>` for `last_state` is what makes the inbound
/// Online flip race-safe with the outbound watchdog's Offline write — every
/// state mutation goes through the same lock.
struct HandlerState {
    member_repo: Arc<dyn MemberRepositoryPort>,
    fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort>,
    last_state: Arc<Mutex<HashMap<String, ReachabilityState>>>,
    event_tx: broadcast::Sender<PresenceEvent>,
    clock: Arc<dyn ClockPort>,
}

impl HandlerState {
    /// Resolve `remote_pubkey_bytes` (iroh `EndpointId` 32-byte public key)
    /// back to a `SpaceMember.device_id` via the same fingerprint factory
    /// the receiver adapter uses. `None` means "unknown peer" — handler
    /// holds the connection open but does not mutate presence state.
    ///
    /// `member_repo.list()` is acceptable per the Slice 2 N ≤ 10 roster
    /// assumption (see `clipboard_receiver_adapter.rs` for the same
    /// rationale). A dedicated lookup-by-fingerprint index is a Phase 3
    /// concern.
    async fn resolve_device(&self, remote_pubkey_bytes: &[u8; 32]) -> Option<DeviceId> {
        let derived = match self
            .fingerprint_factory
            .from_public_key(remote_pubkey_bytes)
        {
            Ok(fp) => fp,
            Err(err) => {
                warn!(
                    error = %err,
                    "presence accept: fingerprint derivation failed — cannot resolve peer",
                );
                return None;
            }
        };

        let members = match self.member_repo.list().await {
            Ok(ms) => ms,
            Err(err) => {
                warn!(
                    error = %err,
                    "presence accept: member_repo.list failed; treating peer as unknown",
                );
                return None;
            }
        };

        members
            .into_iter()
            .find(|m| fingerprints_equal(&m.identity_fingerprint, &derived))
            .map(|m| m.device_id)
    }

    fn now(&self) -> DateTime<Utc> {
        let ms = self.clock.now_ms();
        Utc.timestamp_millis_opt(ms).single().unwrap_or_else(|| {
            warn!(
                ms,
                "ClockPort returned out-of-range epoch millis; falling back to Utc::now",
            );
            Utc::now()
        })
    }
}

/// Accept-side handler for [`PRESENCE_ALPN`].
///
/// Holds each inbound connection open until the peer closes it. Beyond
/// holding (the original liveness contract), it also reverse-resolves the
/// remote endpoint id to a `DeviceId` and flips
/// `last_state[device] = Online` on the first inbound dial that wasn't
/// already Online — the recovery path that makes peer-keepalive backoff
/// safe to extend.
#[derive(Clone)]
pub struct IrohPresenceHandler {
    state: Arc<HandlerState>,
}

impl std::fmt::Debug for IrohPresenceHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrohPresenceHandler")
            .finish_non_exhaustive()
    }
}

impl ProtocolHandler for IrohPresenceHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let remote = connection.remote_id();
        debug!(remote = %remote, "presence connection accepted; holding open until peer closes");

        let remote_bytes: [u8; 32] = *remote.as_bytes();
        if let Some(device_id) = self.state.resolve_device(&remote_bytes).await {
            let key = device_id.as_str().to_string();
            let now_at = self.state.now();

            // Acquire `last_state` only long enough to insert and observe
            // the previous value; broadcast and logging happen after the
            // lock drops to avoid holding it across `.send`.
            let prev = {
                let mut last = self.state.last_state.lock().await;
                last.insert(key, ReachabilityState::Online)
            };

            if prev != Some(ReachabilityState::Online) {
                let _ = self.state.event_tx.send(PresenceEvent {
                    device_id: device_id.clone(),
                    state: ReachabilityState::Online,
                    at: now_at,
                });
                info!(
                    device = %device_id.as_str(),
                    "inbound presence connection: peer marked Online",
                );
            } else {
                debug!(
                    device = %device_id.as_str(),
                    "inbound presence connection: peer already Online (no event)",
                );
            }
        } else {
            // Unknown peer: hold the connection (matches the receiver
            // adapter's tolerance for unresolved senders) but do not mutate
            // any presence state and do not broadcast.
            debug!(
                remote = %remote,
                "inbound presence connection from unresolved peer; holding without state change",
            );
        }

        let reason = connection.closed().await;
        debug!(
            remote = %remote,
            reason = ?reason,
            "presence connection closed by peer",
        );
        Ok(())
    }
}

/// `IdentityFingerprint` comparison surface — kept as a free function so
/// future swaps to a normalised form land in one place.
fn fingerprints_equal(a: &IdentityFingerprint, b: &IdentityFingerprint) -> bool {
    a == b
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
    /// connection entry behind. Shared with [`HandlerState`] so inbound
    /// connections can flip a peer to Online under the same lock the
    /// outbound watchdog uses to flip to Offline.
    last_state: Arc<Mutex<HashMap<String, ReachabilityState>>>,
    event_tx: broadcast::Sender<PresenceEvent>,
    /// Cheap-clone state for [`IrohPresenceHandler`]. Constructed once in
    /// [`IrohPresenceAdapter::new`] and handed out via
    /// [`IrohPresenceAdapter::handler`].
    handler_state: Arc<HandlerState>,
}

/// Per-device bookkeeping: the live connection we hold open, plus the
/// watchdog task that awaits its demise.
struct TrackedPeer {
    connection: Connection,
    watchdog: JoinHandle<()>,
    /// Monotonic timestamp of the most recent confirmed-live observation
    /// for this entry. Set on insert in [`IrohPresenceAdapter::
    /// dial_and_track`] (dial just succeeded) and refreshed when a fresh
    /// dial races against an already-tracked entry and finds it still
    /// alive (the redial itself is fresh evidence). Consumed by
    /// [`IrohPresenceAdapter::ensure_reachable`]'s fast-path to refuse
    /// returning Online from an entry that has aged past
    /// [`FAST_PATH_TTL`], forcing a re-dial. See [`FAST_PATH_TTL`] for the
    /// silent-death problem this guards against.
    last_verified_at: Instant,
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
    /// repository, member repository, fingerprint factory, and clock.
    /// Returns an owned value; the caller wraps it in `Arc` before
    /// publishing it as `Arc<dyn PresencePort>` so shutdown semantics match
    /// the rest of the iroh adapter family.
    ///
    /// `member_repo` and `fingerprint_factory` are needed by the inbound
    /// handler to reverse-resolve a remote `EndpointId` into a known
    /// `DeviceId`; the same pair is consumed by `IrohClipboardReceiverAdapter`.
    pub fn new(
        endpoint: Arc<Endpoint>,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
        fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let last_state = Arc::new(Mutex::new(HashMap::new()));
        let handler_state = Arc::new(HandlerState {
            member_repo,
            fingerprint_factory,
            last_state: Arc::clone(&last_state),
            event_tx: event_tx.clone(),
            clock: Arc::clone(&clock),
        });
        Self {
            endpoint,
            peer_addr_repo,
            clock,
            peers: Arc::new(Mutex::new(HashMap::new())),
            last_state,
            event_tx,
            handler_state,
        }
    }

    /// Cheap clone-able handle registered with iroh's `RouterBuilder`. Each
    /// inbound connection runs [`IrohPresenceHandler::accept`], which
    /// shares this adapter's `last_state` map and broadcast `Sender` via
    /// `Arc<HandlerState>`.
    pub fn handler(&self) -> IrohPresenceHandler {
        IrohPresenceHandler {
            state: Arc::clone(&self.handler_state),
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

impl IrohPresenceAdapter {
    /// 共享拨号路径：被 `ensure_reachable`（fast-path miss 后）和
    /// `verify_reachable`（强制路径）复用。负责：
    ///
    /// 1. 从 `peer_addr_repo` 读地址 → 解码 `EndpointAddr`
    /// 2. 通过 `connect_with_staggered_retry` 发起 iroh 拨号
    /// 3. 成功：spawn watchdog、写 `peers` map、broadcast `Online`
    ///    （若已存在 alive 条目则丢弃新拨连接，保留旧的——避免主动重连
    ///    扰动同步链路）
    /// 4. 失败：写 `last_state = Offline`、broadcast `Offline`
    ///    （**不**清理已存在的 stale 条目——业务路径下拨号可能因临时网络
    ///    抖动失败，旧连接其实还可用；`verify_reachable` 在外层补偿
    ///    "把假装活着的旧连接 close 掉"的清理动作）
    async fn dial_and_track(&self, device: &DeviceId) -> Result<ReachabilityState, PresenceError> {
        let key = device.as_str().to_string();

        // Look up the stored transport address.
        let record = self
            .peer_addr_repo
            .get(device)
            .await
            .map_err(|err| PresenceError::Internal(format!("peer_addr_repo.get: {err}")))?;
        let record = match record {
            Some(r) => r,
            None => {
                debug!("dial_and_track: no address record; returning NoAddress");
                return Err(PresenceError::NoAddress(device.clone()));
            }
        };

        // Decode the opaque blob into the adapter-private `EndpointAddr`.
        // Failure is a data-integrity issue (someone wrote junk into the
        // repo) — surface it as `Internal` without leaking the postcard
        // error type upward. The blob is guaranteed to have had its
        // ephemeral `Ip(...)` direct addresses stripped at pairing-time
        // write (see `persistable_addr::to_persistable_addr`), so what we
        // decode here is `id + Relay(...)`. iroh's built-in pkarr
        // discovery fills in fresh direct addrs at connect time.
        let endpoint_addr: EndpointAddr =
            postcard::from_bytes(&record.addr_blob).map_err(|err| {
                PresenceError::Internal(format!("postcard decode EndpointAddr: {err}"))
            })?;

        // Dial.
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
                    // If an alive entry exists already (concurrent insert
                    // raced, or `verify_reachable` redialed against a
                    // tracked-but-stale-looking peer), abort our own
                    // watchdog and keep theirs — single connection slot
                    // per device. Refresh `last_verified_at` on the kept
                    // entry: our just-completed dial is fresh evidence
                    // that the peer is reachable *right now*, so the
                    // fast-path TTL clock should reset even though we're
                    // discarding the new connection in favour of the old.
                    if let Some(existing) = peers.get_mut(&key) {
                        if existing.connection.close_reason().is_none() {
                            existing.last_verified_at = Instant::now();
                            debug!(
                                "dial_and_track: alive tracked entry exists; \
                                 discarding freshly dialed connection",
                            );
                            watchdog.abort();
                            drop(connection);

                            // 仍旧 broadcast Online — verify_reachable 调用方
                            // 期望"拨号成功 ⇒ Online 信号回传"。
                            let mut last = self.last_state.lock().await;
                            last.insert(key.clone(), ReachabilityState::Online);
                            drop(last);
                            self.broadcast(device.clone(), ReachabilityState::Online, now);
                            return Ok(ReachabilityState::Online);
                        }
                    }
                    peers.insert(
                        key.clone(),
                        TrackedPeer {
                            connection,
                            watchdog,
                            last_verified_at: Instant::now(),
                        },
                    );
                }

                {
                    let mut last = self.last_state.lock().await;
                    last.insert(key.clone(), ReachabilityState::Online);
                }
                info!("dial_and_track: dial succeeded, peer marked Online");
                self.broadcast(device.clone(), ReachabilityState::Online, now);
                Ok(ReachabilityState::Online)
            }
            Err(err) => {
                // No iroh error type leaks upward — per `uc-infra/AGENTS.md`
                // §9.1 the failure is summarised into `last_state` + an
                // event. The member stays in the repo; the next dial
                // attempt is how recovery happens.
                debug!(error = %err, "dial_and_track: dial failed, peer marked Offline");
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
        //
        // Both predicates must hold to return Online without a fresh dial:
        //
        // * `close_reason().is_none()` — quinn has not (yet) observed the
        //   connection close. This is the original liveness signal from
        //   T3a but it lags silent-death scenarios by up to
        //   `max_idle_timeout = 60s`.
        //
        // * `last_verified_at.elapsed() < FAST_PATH_TTL` — we have *recent*
        //   first-hand evidence the peer is reachable (a successful dial
        //   landed inside the TTL window). Without this, an entry whose
        //   peer silently died can sit in the map looking alive for the
        //   full quinn idle window, lying "Online" to every caller.
        //
        // Either predicate failing routes through eviction + re-dial. The
        // miss is logged with both flags so a stale-TTL eviction is
        // distinguishable from a closed-conn eviction in production.
        {
            let mut peers = self.peers.lock().await;
            if let Some(entry) = peers.get(&key) {
                let still_alive = entry.connection.close_reason().is_none();
                let recently_verified = entry.last_verified_at.elapsed() < FAST_PATH_TTL;
                if still_alive && recently_verified {
                    debug!("ensure_reachable: already tracked and alive");
                    return Ok(ReachabilityState::Online);
                }
                // Stale entry — either quinn has closed it, or the entry
                // has aged past FAST_PATH_TTL without re-verification.
                // Evict so the re-dial path below starts from a clean
                // slate (and so `dial_and_track`'s "alive entry already
                // exists" branch doesn't accidentally preserve a corpse).
                if let Some(stale) = peers.remove(&key) {
                    stale.watchdog.abort();
                    debug!(
                        still_alive,
                        recently_verified,
                        "ensure_reachable: evicted stale tracked entry before re-dial",
                    );
                }
            }
        }

        // Step 2-4: dial via shared path.
        self.dial_and_track(device).await
    }

    #[instrument(skip_all, fields(device = %device.as_str()))]
    async fn verify_reachable(
        &self,
        device: &DeviceId,
    ) -> Result<ReachabilityState, PresenceError> {
        // 跳过 fast-path —— 即便已有 alive 连接也强制重拨验证可达性。
        let result = self.dial_and_track(device).await?;

        // 拨号失败 ⇒ 对端真不可达。把任何"看起来还活着"的 tracked 条目
        // 从 peers map 移除并主动 close，避免下次 `ensure_reachable` 又
        // 走 fast-path 撒谎说 Online。
        //
        // 不显式 abort watchdog：`Connection::close()` 让 watchdog 的
        // `connection.closed().await` 立即 resolve 跑 cleanup（remove 已
        // noop, last_state 已是 Offline, 再发一次冗余 Offline 事件幂等
        // 无害）。当 `stale` 离作用域时 `TrackedPeer::drop` 兜底 abort，
        // 此时 watchdog 多半已 fire 完毕，abort 也是 noop。这比"先 abort
        // 后 close"避免 watchdog cleanup 半途被砍的 race。
        if matches!(result, ReachabilityState::Offline) {
            let stale = {
                let mut peers = self.peers.lock().await;
                peers.remove(device.as_str())
            };
            if let Some(stale) = stale {
                stale.connection.close(0u32.into(), b"verify_failed");
                debug!("verify_reachable: closed stale connection after dial failure");
            }
        }

        Ok(result)
    }

    #[instrument(skip_all, fields(device = %device.as_str()))]
    async fn mark_offline(&self, device: &DeviceId) {
        let key = device.as_str().to_string();

        // 1) Evict the live-connection slot. The peer is held to be dead by
        //    an external observer — anything we cached as alive is now a lie
        //    that the fast-path in `ensure_reachable` would happily serve.
        //    Close the connection explicitly so the watchdog's
        //    `connection.closed().await` resolves and its cleanup runs
        //    (remove already noop, last_state already Offline below, the
        //    redundant Offline event is idempotent).
        //
        //    Order matches `verify_reachable`'s failure path: remove from
        //    map first, then close — avoids the watchdog cleanup half-killed
        //    race (TrackedPeer::drop will abort the watchdog if it hasn't
        //    fired yet, and that's fine; we don't depend on it running).
        let stale = {
            let mut peers = self.peers.lock().await;
            peers.remove(&key)
        };
        if let Some(stale) = stale {
            stale.connection.close(0u32.into(), b"mark_offline");
            debug!("mark_offline: closed stale tracked connection");
        }

        // 2) Persist Offline in last_state. Skip the broadcast if the device
        //    was already Offline (idempotency contract). Hold the lock across
        //    the prev-vs-new compare-and-set so a racing inbound Online flip
        //    doesn't slip a duplicate Offline through.
        let should_broadcast = {
            let mut last = self.last_state.lock().await;
            let prev = last.insert(key, ReachabilityState::Offline);
            prev != Some(ReachabilityState::Offline)
        };

        if should_broadcast {
            let now = self.now();
            debug!("mark_offline: peer marked Offline");
            self.broadcast(device.clone(), ReachabilityState::Offline, now);
        }
    }

    // 故意不挂 `#[instrument]`:`current_state()` 仅做 in-memory map
    // lookup(`last_state` / `peers`),没有外部 I/O,但被 roster /
    // list_with_presence / ensure_reachable_all 在热路径上反复调用,
    // 14 天观测到 ~20 万次 span 落到 Sentry。`ensure_reachable` /
    // `verify_reachable` 真做拨号,继续保留 instrument(uc-infra §10.1
    // 强制要求关键 adapter 有 tracing)。
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
    use uc_core::membership::{MemberRepositoryPort, MembershipError, SpaceMember};
    use uc_core::ports::{PeerAddressError, PeerAddressRecord};
    use uc_core::MemberSyncPreferences;

    use crate::security::Sha256IdentityFingerprintFactory;

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

    /// In-memory `MemberRepositoryPort` for handler-side identity
    /// resolution. Tests that exercise dial-only paths leave it empty;
    /// tests that exercise inbound `Online` flips seed it with a member
    /// whose `identity_fingerphint` matches the dialing endpoint's pubkey.
    #[derive(Default)]
    struct MemMemberRepo {
        inner: StdMutex<StdHashMap<String, SpaceMember>>,
    }

    impl MemMemberRepo {
        fn seed(&self, member: SpaceMember) {
            self.inner
                .lock()
                .unwrap()
                .insert(member.device_id.as_str().to_string(), member);
        }
    }

    #[async_trait]
    impl MemberRepositoryPort for MemMemberRepo {
        async fn get(&self, device: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
            Ok(self.inner.lock().unwrap().get(device.as_str()).cloned())
        }
        async fn list(&self) -> Result<Vec<SpaceMember>, MembershipError> {
            Ok(self.inner.lock().unwrap().values().cloned().collect())
        }
        async fn save(&self, member: &SpaceMember) -> Result<(), MembershipError> {
            self.inner
                .lock()
                .unwrap()
                .insert(member.device_id.as_str().to_string(), member.clone());
            Ok(())
        }
        async fn remove(&self, device_id: &DeviceId) -> Result<bool, MembershipError> {
            Ok(self
                .inner
                .lock()
                .unwrap()
                .remove(device_id.as_str())
                .is_some())
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
    ///
    /// The B-side adapter is a decoy used solely to produce a working
    /// inbound handler — its `last_state` map is not observed by the
    /// dial-side tests below. Tests that exercise the inbound Online flip
    /// build their own adapter explicitly via
    /// [`build_adapter_with_member_repo`].
    async fn setup_two_endpoints() -> (Arc<Endpoint>, Arc<Endpoint>, Vec<u8>, DeviceId, Router) {
        let endpoint_b = bound_endpoint().await;
        wait_for_direct_addrs(&endpoint_b).await;
        let b_addr = endpoint_b.addr();
        let b_blob = postcard::to_stdvec(&b_addr).expect("postcard encode EndpointAddr");
        let b_device_id = DeviceId::new(format!("endpoint-b-{}", endpoint_b.id().fmt_short()));

        let decoy_adapter = IrohPresenceAdapter::new(
            Arc::clone(&endpoint_b),
            Arc::new(FakePeerAddressRepo::default()),
            Arc::new(MemMemberRepo::default()),
            Arc::new(Sha256IdentityFingerprintFactory),
            Arc::new(FixedClock),
        );
        let router_b = Router::builder((*endpoint_b).clone())
            .accept(PRESENCE_ALPN, decoy_adapter.handler())
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

    /// Build an adapter for the dial-side tests. Inbound resolution is
    /// not exercised here — the empty `MemMemberRepo` is enough to keep
    /// the handler constructible.
    fn build_adapter(
        endpoint: Arc<Endpoint>,
        repo: Arc<dyn PeerAddressRepositoryPort>,
    ) -> IrohPresenceAdapter {
        IrohPresenceAdapter::new(
            endpoint,
            repo,
            Arc::new(MemMemberRepo::default()),
            Arc::new(Sha256IdentityFingerprintFactory),
            Arc::new(FixedClock),
        )
    }

    /// Build an adapter wired to a caller-supplied `MemberRepositoryPort`
    /// so inbound-flip tests can seed the dialing endpoint's identity.
    fn build_adapter_with_member_repo(
        endpoint: Arc<Endpoint>,
        repo: Arc<dyn PeerAddressRepositoryPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
    ) -> IrohPresenceAdapter {
        IrohPresenceAdapter::new(
            endpoint,
            repo,
            member_repo,
            Arc::new(Sha256IdentityFingerprintFactory),
            Arc::new(FixedClock),
        )
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

    // -- Inbound-driven Online flip ------------------------------------------

    /// Build a `SpaceMember` whose `identity_fingerprint` matches the
    /// pubkey of `endpoint`, so the presence handler can reverse-resolve an
    /// inbound `Connection::remote_id()` from that endpoint back to
    /// `device_id`.
    fn member_for_endpoint(endpoint: &Endpoint, device_id: &str) -> SpaceMember {
        let factory = Sha256IdentityFingerprintFactory;
        let fp = factory
            .from_public_key(endpoint.id().as_bytes())
            .expect("derive fingerprint from endpoint pubkey");
        SpaceMember {
            device_id: DeviceId::new(device_id),
            device_name: device_id.to_string(),
            identity_fingerprint: fp,
            joined_at: Utc::now(),
            sync_preferences: MemberSyncPreferences::default(),
        }
    }

    /// Verdict — when an Offline (or Unknown) peer dials us at
    /// `PRESENCE_ALPN`, the handler reverse-resolves the remote pubkey to
    /// the seeded `DeviceId`, writes `last_state[device]=Online`, and
    /// emits exactly one `Online` event.
    #[tokio::test]
    async fn accept_from_known_peer_flips_offline_to_online() {
        let endpoint_a = bound_endpoint().await;
        wait_for_direct_addrs(&endpoint_a).await;
        let endpoint_b = bound_endpoint().await;
        wait_for_direct_addrs(&endpoint_b).await;

        let a_member = member_for_endpoint(&endpoint_a, "device-a");
        let a_device_id = a_member.device_id.clone();
        let b_member_repo = Arc::new(MemMemberRepo::default());
        b_member_repo.seed(a_member);

        let b_peer_addr_repo: Arc<dyn PeerAddressRepositoryPort> =
            Arc::new(FakePeerAddressRepo::default());
        let b_member_repo_dyn: Arc<dyn MemberRepositoryPort> = b_member_repo;
        let b_adapter = build_adapter_with_member_repo(
            Arc::clone(&endpoint_b),
            b_peer_addr_repo,
            b_member_repo_dyn,
        );
        let mut subscriber = b_adapter.subscribe();

        // Before any inbound dial, B has no opinion on A's reachability.
        assert_eq!(
            b_adapter.current_state(&a_device_id).await,
            ReachabilityState::Unknown,
        );

        let router_b = Router::builder((*endpoint_b).clone())
            .accept(PRESENCE_ALPN, b_adapter.handler())
            .spawn();

        // A dials B directly — this exercises B's accept handler without
        // pulling in A's own adapter. The connection is held open by the
        // handler until the test drops it.
        let b_addr = endpoint_b.addr();
        let conn = timeout(DIAL_BUDGET, endpoint_a.connect(b_addr, PRESENCE_ALPN))
            .await
            .expect("connect within budget")
            .expect("A dial B succeeds");

        let event = timeout(Duration::from_secs(3), subscriber.recv())
            .await
            .expect("inbound Online event arrives within 3s")
            .expect("event channel open");
        assert_eq!(event.device_id, a_device_id);
        assert_eq!(event.state, ReachabilityState::Online);

        assert_eq!(
            b_adapter.current_state(&a_device_id).await,
            ReachabilityState::Online,
        );

        drop(conn);
        router_b.shutdown().await.ok();
        endpoint_a.close().await;
    }

    /// Verdict — repeated inbound dials from the same already-Online peer
    /// must NOT re-broadcast. Each keepalive tick from a stable peer would
    /// otherwise spam subscribers with duplicate events.
    #[tokio::test]
    async fn accept_already_online_does_not_rebroadcast() {
        let endpoint_a = bound_endpoint().await;
        wait_for_direct_addrs(&endpoint_a).await;
        let endpoint_b = bound_endpoint().await;
        wait_for_direct_addrs(&endpoint_b).await;

        let a_member = member_for_endpoint(&endpoint_a, "device-a");
        let b_member_repo = Arc::new(MemMemberRepo::default());
        b_member_repo.seed(a_member);

        let b_adapter = build_adapter_with_member_repo(
            Arc::clone(&endpoint_b),
            Arc::new(FakePeerAddressRepo::default()) as Arc<dyn PeerAddressRepositoryPort>,
            b_member_repo as Arc<dyn MemberRepositoryPort>,
        );
        let mut subscriber = b_adapter.subscribe();

        let router_b = Router::builder((*endpoint_b).clone())
            .accept(PRESENCE_ALPN, b_adapter.handler())
            .spawn();

        let b_addr = endpoint_b.addr();
        let conn1 = timeout(
            DIAL_BUDGET,
            endpoint_a.connect(b_addr.clone(), PRESENCE_ALPN),
        )
        .await
        .expect("first connect within budget")
        .expect("first dial succeeds");
        let first = timeout(Duration::from_secs(3), subscriber.recv())
            .await
            .expect("first event arrives")
            .expect("channel open");
        assert_eq!(first.state, ReachabilityState::Online);

        // Second dial — B's `last_state[A]` is already `Online`, so the
        // handler must skip the broadcast.
        let conn2 = timeout(DIAL_BUDGET, endpoint_a.connect(b_addr, PRESENCE_ALPN))
            .await
            .expect("second connect within budget")
            .expect("second dial succeeds");

        // Drain attempt with a tight deadline: any re-broadcast would
        // arrive within milliseconds of the second connection landing.
        let no_event = timeout(Duration::from_millis(500), subscriber.recv()).await;
        assert!(
            no_event.is_err(),
            "expected no second event, got {:?}",
            no_event.ok().map(|r| r.map(|e| (e.device_id, e.state))),
        );

        drop(conn1);
        drop(conn2);
        router_b.shutdown().await.ok();
        endpoint_a.close().await;
    }

    /// Verdict — an inbound dial from a peer whose pubkey is NOT in
    /// `member_repo` must hold the connection but leave presence state
    /// untouched and emit no event. Mirrors the receiver adapter's
    /// "unknown peer" tolerance.
    #[tokio::test]
    async fn accept_unknown_peer_does_not_touch_state() {
        let endpoint_a = bound_endpoint().await;
        wait_for_direct_addrs(&endpoint_a).await;
        let endpoint_b = bound_endpoint().await;
        wait_for_direct_addrs(&endpoint_b).await;

        // member_repo deliberately empty — A's fingerprint will not
        // resolve to any DeviceId.
        let b_adapter = build_adapter_with_member_repo(
            Arc::clone(&endpoint_b),
            Arc::new(FakePeerAddressRepo::default()) as Arc<dyn PeerAddressRepositoryPort>,
            Arc::new(MemMemberRepo::default()) as Arc<dyn MemberRepositoryPort>,
        );
        let mut subscriber = b_adapter.subscribe();

        let router_b = Router::builder((*endpoint_b).clone())
            .accept(PRESENCE_ALPN, b_adapter.handler())
            .spawn();

        let b_addr = endpoint_b.addr();
        let conn = timeout(DIAL_BUDGET, endpoint_a.connect(b_addr, PRESENCE_ALPN))
            .await
            .expect("connect within budget")
            .expect("dial succeeds");

        // Give the handler time to run resolve_device, then assert no
        // event landed.
        let no_event = timeout(Duration::from_millis(500), subscriber.recv()).await;
        assert!(
            no_event.is_err(),
            "unknown peer must not produce a presence event",
        );

        // No DeviceId was ever associated with A, so any current_state
        // probe returns Unknown — the canonical "no opinion yet" verdict.
        assert_eq!(
            b_adapter
                .current_state(&DeviceId::new("not-a-real-id"))
                .await,
            ReachabilityState::Unknown,
        );

        drop(conn);
        router_b.shutdown().await.ok();
        endpoint_a.close().await;
    }
}
