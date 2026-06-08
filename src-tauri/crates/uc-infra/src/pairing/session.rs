//! Iroh-backed implementation of [`PairingSessionPort`] + [`PairingEventPort`].
//!
//! Joiner side (P7c.2):
//!
//! 1. `dial_by_invitation(code)` asks the shared [`RendezvousClient`] to
//!    resolve the code into a sponsor ticket
//!    ([`crate::rendezvous::client`]'s `/v1/pairings/resolve`),
//!    deserializes the opaque payload into an iroh [`EndpointAddr`],
//!    dials the sponsor with ALPN [`PAIRING_ALPN`], and opens a
//!    bi-directional stream.
//! 2. `send` / `recv_next` ride the stream with a 4-byte big-endian length
//!    prefix followed by a postcard-encoded [`PairingSessionMessage`] (see
//!    [`super::wire`]).
//! 3. `close` releases the stored session.
//!
//! Sponsor side (P7c.3):
//!
//! * [`IrohPairingSessionAdapter::install_handler`] registers a
//!   [`PairingProtocolHandler`] on a given [`iroh::protocol::RouterBuilder`].
//!   The handler accepts the first bi-stream, reads one framed
//!   [`PairingSessionMessage`], stashes the live [`Connection`] + streams
//!   under a freshly minted [`PairingSessionId`], and emits
//!   [`PairingSessionEvent::Incoming`] to the subscriber installed via
//!   [`PairingEventPort::subscribe`].
//! * Subscription is single-consumer: a second `subscribe()` call replaces
//!   the previous sender (the old receiver then observes channel close).
//!
//! [`PairingSessionPort`]: uc_core::ports::pairing::PairingSessionPort
//! [`PairingEventPort`]: uc_core::ports::pairing::PairingEventPort
//! [`EndpointAddr`]: iroh::EndpointAddr
//! [`PairingSessionMessage`]: uc_core::pairing::PairingSessionMessage
//! [`Connection`]: iroh::endpoint::Connection
//! [`PairingSessionEvent::Incoming`]: uc_core::ports::pairing::PairingSessionEvent::Incoming

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use iroh::endpoint::{Connection, RecvStream, SendStream};
use iroh::protocol::{AcceptError, ProtocolHandler, RouterBuilder};
use iroh::{Endpoint, EndpointAddr};
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use tracing::{debug, info, instrument, warn};

use uc_core::pairing::{InvitationCode, PairingSessionMessage};
use uc_core::ports::pairing::{
    DialError, DialOutcome, DiscoveryChannel, PairingEventPort, PairingSessionEvent,
    PairingSessionId, PairingSessionPort, SessionError,
};

use super::wire::{self, WireDecodeError};
use crate::network::iroh::connect_with_staggered_retry;
use crate::rendezvous::{RendezvousClient, RendezvousHttpError};

/// Bound for the [`PairingEventPort`] channel. 32 is comfortably above the
/// expected inbound rate (a human approves pairing at most every few
/// seconds) while still bounded so a stuck subscriber exerts back-pressure
/// instead of unbounded memory growth.
const EVENT_CHANNEL_CAPACITY: usize = 32;

/// ALPN identifier for the Slice 1 pairing protocol (F-014).
pub const PAIRING_ALPN: &[u8] = b"/uniclipboard/pairing/1";

const FRAME_LEN_BYTES: usize = 4;

/// Sponsor-side per-step timeout for the inbound accept path.
///
/// 30s 是配合 joiner 侧 180s `handshake_ttl` 设的:每个 IO 等待
/// (accept_bi / read frame length / read frame payload)在 DERP relay
/// + iroh path migration 抖动下,30s 内若不见数据基本就是连接坏了,继续
/// 等只会吊死。失败立刻 drop 让 iroh router 释放 socket,joiner 那侧
/// retry 比慢失败健康。
const INBOUND_STEP_TIMEOUT: Duration = Duration::from_secs(30);

// ============================================================================
// Adapter
// ============================================================================

/// Iroh-backed pairing session adapter.
pub struct IrohPairingSessionAdapter {
    endpoint: Arc<Endpoint>,
    /// Shared rendezvous HTTP gateway. Used only for `dial_by_invitation`
    /// (joiner-side resolve call); the sponsor-side accept path doesn't
    /// touch rendezvous.
    rendezvous: Arc<RendezvousClient>,
    sessions: Mutex<HashMap<PairingSessionId, Arc<SessionSlot>>>,
    next_session_seq: AtomicU64,
    /// Sender side of the currently installed [`PairingEventPort`]
    /// subscription. Filled by [`subscribe`](PairingEventPort::subscribe)
    /// and drained by the sponsor-side handler. `None` means no subscriber
    /// is listening yet; incoming sessions are dropped in that window with
    /// a warn-level log (§10 of `uc-infra/AGENTS.md` — failures must be
    /// observable).
    incoming_tx: Mutex<Option<mpsc::Sender<PairingSessionEvent>>>,
}

struct SessionSlot {
    send: Mutex<SendStream>,
    recv: Mutex<RecvStream>,
    // Hold the connection so it stays alive for the session's lifetime.
    _connection: Connection,
}

impl IrohPairingSessionAdapter {
    /// Build an adapter wired to the given iroh endpoint and shared
    /// rendezvous gateway. Hand the same [`RendezvousClient`] `Arc` to
    /// every rendezvous-aware adapter in the process so they share
    /// connection pool, timeout, and user-agent.
    pub fn new(endpoint: Arc<Endpoint>, rendezvous: Arc<RendezvousClient>) -> Self {
        Self {
            endpoint,
            rendezvous,
            sessions: Mutex::new(HashMap::new()),
            next_session_seq: AtomicU64::new(0),
            incoming_tx: Mutex::new(None),
        }
    }

    fn mint_session_id(&self) -> PairingSessionId {
        let seq = self.next_session_seq.fetch_add(1, Ordering::Relaxed);
        PairingSessionId::new(format!("{}:{seq}", self.endpoint.id().fmt_short()))
    }

    /// Races the cloud channel and the LAN channel; first successful
    /// resolution wins. If either branch errors out, we keep waiting on
    /// the other one rather than failing fast — `InvitationNotFound`
    /// only surfaces when *both* channels report no match.
    ///
    /// In LAN-only mode the cloud branch is skipped entirely (no HTTP
    /// request) and only the LAN branch is awaited.
    async fn resolve_invitation(
        &self,
        code: &InvitationCode,
    ) -> Result<(EndpointAddr, DiscoveryChannel), DialError> {
        use futures_util::future::{select, Either};
        use std::pin::pin;

        if crate::network::iroh::runtime_consts::lan_only() {
            debug!(code = %code.as_str(), "LAN-only mode: skipping cloud channel");
            return self
                .resolve_via_mdns(code)
                .await
                .map(|addr| (addr, DiscoveryChannel::Lan));
        }

        let cloud_fut = self.resolve_via_cloud(code);
        let mdns_fut = self.resolve_via_mdns(code);
        let cloud_fut = pin!(cloud_fut);
        let mdns_fut = pin!(mdns_fut);

        let (resolved, channel) = match select(cloud_fut, mdns_fut).await {
            Either::Left((Ok(addr), _)) => {
                debug!(code = %code.as_str(), channel = "cloud", "discovery race winner");
                (addr, DiscoveryChannel::Cloud)
            }
            Either::Right((Ok(addr), _)) => {
                debug!(code = %code.as_str(), channel = "lan", "discovery race winner");
                (addr, DiscoveryChannel::Lan)
            }
            Either::Left((Err(cloud_err), pending_mdns)) => {
                debug!(
                    code = %code.as_str(),
                    cloud_err = ?cloud_err,
                    "cloud channel failed; waiting for LAN channel"
                );
                match pending_mdns.await {
                    Ok(addr) => (addr, DiscoveryChannel::Lan),
                    Err(mdns_err) => {
                        warn!(
                            code = %code.as_str(),
                            cloud = ?cloud_err,
                            lan = ?mdns_err,
                            "all discovery channels failed"
                        );
                        return Err(prefer_dial_error(cloud_err, mdns_err));
                    }
                }
            }
            Either::Right((Err(mdns_err), pending_cloud)) => {
                debug!(
                    code = %code.as_str(),
                    lan_err = ?mdns_err,
                    "LAN channel failed; waiting for cloud channel"
                );
                match pending_cloud.await {
                    Ok(addr) => (addr, DiscoveryChannel::Cloud),
                    Err(cloud_err) => {
                        warn!(
                            code = %code.as_str(),
                            cloud = ?cloud_err,
                            lan = ?mdns_err,
                            "all discovery channels failed"
                        );
                        return Err(prefer_dial_error(cloud_err, mdns_err));
                    }
                }
            }
        };

        info!(
            code = %code.as_str(),
            sponsor = %resolved.id.fmt_short(),
            transport_addr_count = resolved.addrs.len(),
            ?channel,
            "pairing invitation resolved; sponsor address ready"
        );
        Ok((resolved, channel))
    }

    async fn resolve_via_cloud(&self, code: &InvitationCode) -> Result<EndpointAddr, DialError> {
        let resp = self
            .rendezvous
            .resolve_pairing(code.as_str())
            .await
            .map_err(map_resolve_err)?;
        serde_json::from_str::<EndpointAddr>(&resp.sponsor_ticket)
            .map_err(|err| DialError::Internal(format!("sponsor ticket decode: {err}")))
    }

    async fn resolve_via_mdns(&self, code: &InvitationCode) -> Result<EndpointAddr, DialError> {
        use std::time::Duration as StdDuration;

        use crate::pairing::MdnsPairingResolver;

        // LAN window: longer than typical mDNS announce cadence (2s) so
        // joiners that just opened the dial reliably catch at least one
        // announce; shorter than the user's patience for the cloud
        // fallback (cloud usually answers in <500ms when reachable).
        const LAN_WINDOW: StdDuration = StdDuration::from_secs(5);

        let self_node_id = self.endpoint.id().to_string();
        let ticket_hex = MdnsPairingResolver::resolve(
            &tokio::runtime::Handle::current(),
            &self_node_id,
            code.as_str(),
            LAN_WINDOW,
        )
        .await
        .map_err(|err| DialError::Internal(format!("mDNS resolver: {err}")))?;

        let Some(ticket_hex) = ticket_hex else {
            // Window elapsed without a match: treat as NotFound. The
            // caller (race driver) will combine this with the cloud
            // branch's outcome to decide the final error variant.
            return Err(DialError::InvitationNotFound);
        };

        // mDNS ticket wire format: `hex(postcard(EndpointAddr))`.
        // Matches the sponsor's `encode_mdns_ticket` exactly; JSON is
        // used only on the cloud channel where TXT-size constraints
        // don't apply.
        let ticket_bytes = hex::decode(&ticket_hex)
            .map_err(|err| DialError::Internal(format!("LAN ticket hex decode: {err}")))?;
        postcard::from_bytes::<EndpointAddr>(&ticket_bytes)
            .map_err(|err| DialError::Internal(format!("LAN ticket postcard decode: {err}")))
    }

    /// Install a ready-built session into the map and return the minted id.
    /// Shared between the real `dial_by_invitation` path and sponsor-side
    /// accept (P7c.3).
    pub(crate) async fn register_session(
        &self,
        connection: Connection,
        send: SendStream,
        recv: RecvStream,
    ) -> PairingSessionId {
        let id = self.mint_session_id();
        let slot = Arc::new(SessionSlot {
            send: Mutex::new(send),
            recv: Mutex::new(recv),
            _connection: connection,
        });
        self.sessions.lock().await.insert(id.clone(), slot);
        id
    }

    async fn session(&self, id: &PairingSessionId) -> Result<Arc<SessionSlot>, SessionError> {
        self.sessions
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| SessionError::NotFound(id.clone()))
    }

    /// Register [`PairingProtocolHandler`] for [`PAIRING_ALPN`] on the given
    /// iroh [`RouterBuilder`]. Consumes and returns the builder so callers
    /// can chain additional protocols before `.spawn()`.
    ///
    /// The adapter's own [`Endpoint`] must match the one the builder was
    /// constructed with — otherwise the handler will never see inbound
    /// connections. We don't assert equality here because iroh's
    /// [`Endpoint`] identity is internal; instead the wiring layer
    /// (uc-application / uc-bootstrap) is responsible for passing the same
    /// endpoint to both.
    pub fn install_handler(self: &Arc<Self>, builder: RouterBuilder) -> RouterBuilder {
        builder.accept(
            PAIRING_ALPN,
            PairingProtocolHandler {
                adapter: Arc::clone(self),
            },
        )
    }

    /// Sponsor-side inbound path. Runs on a fresh tokio task spawned by the
    /// iroh router for each accepted connection.
    async fn handle_incoming(&self, connection: Connection) {
        let remote = connection.remote_id();
        info!(
            remote = %remote,
            "pairing incoming connection received; waiting for bi stream"
        );
        let (send, mut recv) = match timeout(INBOUND_STEP_TIMEOUT, connection.accept_bi()).await {
            Ok(Ok(pair)) => pair,
            Ok(Err(err)) => {
                warn!(
                    error = %err,
                    remote = %remote,
                    "pairing accept_bi failed; dropping connection",
                );
                return;
            }
            Err(_) => {
                warn!(
                    remote = %remote,
                    timeout_ms = INBOUND_STEP_TIMEOUT.as_millis() as u64,
                    "pairing accept_bi timed out; dropping connection",
                );
                return;
            }
        };
        info!(
            remote = %remote,
            "pairing bi stream accepted; waiting for first frame length"
        );

        let mut len_buf = [0u8; FRAME_LEN_BYTES];
        match timeout(INBOUND_STEP_TIMEOUT, recv.read_exact(&mut len_buf)).await {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => {
                warn!(
                    error = %err,
                    remote = %remote,
                    "pairing first-frame length read failed; dropping connection",
                );
                return;
            }
            Err(_) => {
                warn!(
                    remote = %remote,
                    timeout_ms = INBOUND_STEP_TIMEOUT.as_millis() as u64,
                    "pairing first-frame length read timed out; dropping connection",
                );
                return;
            }
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        info!(
            remote = %remote,
            frame_len = len,
            "pairing first-frame length read"
        );

        let mut payload = vec![0u8; len];
        match timeout(INBOUND_STEP_TIMEOUT, recv.read_exact(&mut payload)).await {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => {
                warn!(
                    error = %err,
                    remote = %remote,
                    frame_len = len,
                    "pairing first-frame payload read failed; dropping connection",
                );
                return;
            }
            Err(_) => {
                warn!(
                    remote = %remote,
                    frame_len = len,
                    timeout_ms = INBOUND_STEP_TIMEOUT.as_millis() as u64,
                    "pairing first-frame payload read timed out; dropping connection",
                );
                return;
            }
        }

        let message = match wire::decode(&payload) {
            Ok(m) => m,
            Err(err) => {
                warn!(
                    error = %err,
                    remote = %remote,
                    "pairing first-frame decode failed; dropping connection",
                );
                return;
            }
        };
        let message_kind = message_kind(&message);
        info!(
            remote = %remote,
            frame_len = len,
            message_kind,
            "pairing first-frame decoded"
        );

        let session = self.register_session(connection, send, recv).await;
        info!(
            session = %session,
            remote = %remote,
            message_kind,
            "pairing session accepted"
        );

        let tx_snapshot = self.incoming_tx.lock().await.clone();
        let Some(tx) = tx_snapshot else {
            warn!(
                session = %session,
                "pairing event dropped: no subscriber installed",
            );
            // Keep the session registered: if a subscriber attaches later
            // it won't see *this* event, but the operator-visible warn above
            // is the signal. Cleanup is the caller's job via `close()`.
            return;
        };
        if let Err(err) = tx
            .send(PairingSessionEvent::Incoming {
                session: session.clone(),
                message,
            })
            .await
        {
            warn!(
                session = %session,
                error = %err,
                "pairing event receiver dropped before delivery",
            );
            return;
        }
        info!(
            session = %session,
            message_kind,
            "pairing incoming event delivered to orchestrator"
        );

        // Sponsor-side recv pump: after the first frame fires `Incoming`,
        // every subsequent frame from the joiner (e.g. ChallengeResponse)
        // must surface as `MessageReceived` so the inbound orchestrator
        // drives state forward. The joiner side (dial_by_invitation) does
        // not spawn a pump because `JoinerHandshakeCoordinator` polls
        // `recv_next` explicitly; mixing the two would deadlock on
        // `SessionSlot.recv`.
        self.spawn_recv_pump(session, tx).await;
    }

    /// Spawn a tokio task that drains subsequent frames from the session's
    /// recv stream and emits `MessageReceived` / `Closed` events. Exits on
    /// peer FIN or an unrecoverable read error. The task holds an `Arc` to
    /// the session slot, so `close()` removing the map entry is not enough
    /// to stop it — the pump naturally exits when the peer closes their
    /// send side, which happens on every clean handshake termination
    /// (sponsor `close()` → joiner sees FIN → joiner closes → sponsor
    /// recv sees FIN).
    async fn spawn_recv_pump(
        &self,
        session: PairingSessionId,
        tx: mpsc::Sender<PairingSessionEvent>,
    ) {
        let slot = match self.sessions.lock().await.get(&session).cloned() {
            Some(slot) => slot,
            None => {
                // Session disappeared between register and pump spawn —
                // nothing to drain.
                return;
            }
        };
        tokio::spawn(async move {
            loop {
                let frame = {
                    let mut recv = slot.recv.lock().await;
                    read_next_frame(&mut recv).await
                };
                match frame {
                    Ok(Some(message)) => {
                        if tx
                            .send(PairingSessionEvent::MessageReceived {
                                session: session.clone(),
                                message,
                            })
                            .await
                            .is_err()
                        {
                            // Subscriber gone — no point continuing to
                            // drain; nothing consumes the events.
                            return;
                        }
                    }
                    Ok(None) => {
                        // Peer half-closed cleanly — emit Closed with no
                        // reason and exit.
                        let _ = tx
                            .send(PairingSessionEvent::Closed {
                                session: session.clone(),
                                reason: None,
                            })
                            .await;
                        return;
                    }
                    Err(err) => {
                        // Non-EOF read error — surface the reason text so
                        // the orchestrator can log it. Includes the
                        // `SessionError::Closed` case for FIN via Reset.
                        let reason = match err {
                            SessionError::Closed => None,
                            other => Some(other.to_string()),
                        };
                        let _ = tx
                            .send(PairingSessionEvent::Closed {
                                session: session.clone(),
                                reason,
                            })
                            .await;
                        return;
                    }
                }
            }
        });
    }
}

/// Read one length-prefixed frame off `recv`. Returns `Ok(None)` on clean
/// peer half-close (matches `recv_next`'s `None` contract). Extracted so
/// both [`IrohPairingSessionAdapter::spawn_recv_pump`] and
/// [`<IrohPairingSessionAdapter as PairingSessionPort>::recv_next`] share
/// the same wire framing.
async fn read_next_frame(
    recv: &mut RecvStream,
) -> Result<Option<PairingSessionMessage>, SessionError> {
    let mut len_buf = [0u8; FRAME_LEN_BYTES];
    match recv.read_exact(&mut len_buf).await {
        Ok(()) => {}
        Err(iroh::endpoint::ReadExactError::FinishedEarly(0)) => return Ok(None),
        Err(err) => return Err(map_read_err(err)),
    }
    let len = u32::from_be_bytes(len_buf) as usize;

    let mut payload = vec![0u8; len];
    recv.read_exact(&mut payload).await.map_err(map_read_err)?;

    wire::decode(&payload).map(Some).map_err(|err| match err {
        WireDecodeError::Postcard(_)
        | WireDecodeError::UnsupportedVersion { .. }
        | WireDecodeError::InvalidFingerprint(_)
        | WireDecodeError::InvalidSpacePersonId(_) => {
            SessionError::Internal(format!("wire decode: {err}"))
        }
    })
}

// ============================================================================
// ProtocolHandler
// ============================================================================

/// Thin wrapper that adapts [`IrohPairingSessionAdapter`] to the iroh
/// [`ProtocolHandler`] trait. Kept as a dedicated struct (rather than
/// `impl ProtocolHandler for IrohPairingSessionAdapter`) because the
/// handler needs `Debug` and `'static`, which cleanly match a thin wrapper
/// holding an `Arc<IrohPairingSessionAdapter>`.
#[derive(Clone)]
pub(crate) struct PairingProtocolHandler {
    adapter: Arc<IrohPairingSessionAdapter>,
}

impl std::fmt::Debug for PairingProtocolHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingProtocolHandler")
            .finish_non_exhaustive()
    }
}

impl ProtocolHandler for PairingProtocolHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        self.adapter.handle_incoming(connection).await;
        Ok(())
    }
}

#[async_trait]
impl PairingEventPort for IrohPairingSessionAdapter {
    async fn subscribe(&self) -> anyhow::Result<mpsc::Receiver<PairingSessionEvent>> {
        let (tx, rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let mut guard = self.incoming_tx.lock().await;
        if guard.is_some() {
            debug!("pairing subscriber replaced; previous receiver will observe close");
        }
        *guard = Some(tx);
        Ok(rx)
    }
}

#[async_trait]
impl PairingSessionPort for IrohPairingSessionAdapter {
    #[instrument(skip_all, fields(code = %code.as_str()))]
    async fn dial_by_invitation(&self, code: &InvitationCode) -> Result<DialOutcome, DialError> {
        let (sponsor_addr, channel) = self.resolve_invitation(code).await?;
        let sponsor_id = sponsor_addr.id.fmt_short().to_string();
        let transport_addr_count = sponsor_addr.addrs.len();
        info!(
            sponsor = %sponsor_id,
            transport_addr_count,
            "pairing sponsor address resolved; dialing"
        );

        let connection = connect_with_staggered_retry(
            Arc::clone(&self.endpoint),
            sponsor_addr,
            PAIRING_ALPN,
            "pairing",
        )
        .await
        .map_err(|err| {
            warn!(
                error = %err,
                sponsor = %sponsor_id,
                "pairing sponsor connect failed"
            );
            DialError::SponsorUnreachable
        })?;
        info!(
            sponsor = %sponsor_id,
            "pairing sponsor connection established; opening bi stream"
        );

        let (send, recv) = connection.open_bi().await.map_err(|err| {
            warn!(
                error = %err,
                sponsor = %sponsor_id,
                "pairing open_bi failed"
            );
            DialError::Internal(format!("open_bi failed: {err}"))
        })?;
        info!(
            sponsor = %sponsor_id,
            "pairing bi stream opened"
        );

        let session = self.register_session(connection, send, recv).await;
        info!(
            session = %session,
            sponsor = %sponsor_id,
            "pairing outbound session registered"
        );
        Ok(DialOutcome {
            session_id: session,
            channel,
        })
    }

    #[instrument(skip_all, fields(session = %session))]
    async fn send(
        &self,
        session: &PairingSessionId,
        message: PairingSessionMessage,
    ) -> Result<(), SessionError> {
        let slot = self.session(session).await?;
        let message_kind = message_kind(&message);
        let payload = wire::encode(&message)
            .map_err(|err| SessionError::Internal(format!("wire encode: {err}")))?;
        let len: u32 = payload
            .len()
            .try_into()
            .map_err(|_| SessionError::Internal(format!("payload too large: {}", payload.len())))?;

        let mut send = slot.send.lock().await;
        send.write_all(&len.to_be_bytes())
            .await
            .map_err(map_write_err)?;
        send.write_all(&payload).await.map_err(map_write_err)?;
        info!(
            session = %session,
            message_kind,
            frame_len = payload.len(),
            "pairing frame sent"
        );
        Ok(())
    }

    #[instrument(skip_all, fields(session = %session))]
    async fn recv_next(
        &self,
        session: &PairingSessionId,
    ) -> Result<Option<PairingSessionMessage>, SessionError> {
        let slot = self.session(session).await?;
        let mut recv = slot.recv.lock().await;
        let frame = read_next_frame(&mut recv).await?;
        match &frame {
            Some(message) => info!(
                session = %session,
                message_kind = message_kind(message),
                "pairing frame received"
            ),
            None => info!(
                session = %session,
                "pairing recv observed peer half-close"
            ),
        }
        Ok(frame)
    }

    #[instrument(skip_all, fields(session = %session))]
    async fn close(&self, session: &PairingSessionId, reason: Option<String>) {
        let mut map = self.sessions.lock().await;
        if let Some(slot) = map.remove(session) {
            // Try to half-close the send side so the peer sees EOF.
            if let Ok(mut send) = slot.send.try_lock() {
                let _ = send.finish();
            }
            debug!(?reason, "pairing session closed");
        }
    }

    /// Slice 2 Phase 1 · T5：返回本端 [`EndpointAddr`] 的 postcard 不透明
    /// 字节。handshake coordinator 在发出 `JoinerRequest` / `SponsorConfirm`
    /// 前调用此方法填充 `transport_address_blob`，对端接到后直接写入
    /// `PeerAddressRepositoryPort`。
    ///
    /// `endpoint.addr()` 当下观察值里包含 magicsock 这次进程绑定的随机
    /// UDP 端口；那是 ephemeral 信息，进程一重启就失效。我们在这里通过
    /// [`crate::network::iroh::persistable_addr::to_persistable_addr`]
    /// 把 `Ip(...)` 直连项剥掉，只把 NodeId + 长寿命的 `Relay(...)` 写
    /// 进 wire/repo。读侧解码后直接给 `endpoint.connect`，iroh 内置的
    /// pkarr discovery 会在每次 connect 时拉取对端**当前发布**的直连
    /// 地址——正确的 contract 是"持久化身份+ relay hint，让 discovery
    /// 负责寻址"。详见 `persistable_addr` 模块文档。
    ///
    /// 返回 `None` 表示编码失败（理论上不会发生——postcard 对
    /// `EndpointAddr` 的序列化是 total），此时对端会以空 blob 兜底跳过
    /// upsert。
    async fn local_transport_address_blob(&self) -> Option<Vec<u8>> {
        let raw = self.endpoint.addr();
        let addr = crate::network::iroh::persistable_addr::to_persistable_addr(raw);
        match postcard::to_stdvec(&addr) {
            Ok(bytes) => Some(bytes),
            Err(err) => {
                warn!(error = %err, "postcard encode EndpointAddr failed; skipping address publish");
                None
            }
        }
    }
}

fn message_kind(message: &PairingSessionMessage) -> &'static str {
    match message {
        PairingSessionMessage::Request(_) => "Request",
        PairingSessionMessage::KeyslotOffer(_) => "KeyslotOffer",
        PairingSessionMessage::ChallengeResponse(_) => "ChallengeResponse",
        PairingSessionMessage::Confirm(_) => "Confirm",
        PairingSessionMessage::Reject(_) => "Reject",
    }
}

// ============================================================================
// Error mappers
// ============================================================================

fn map_write_err(err: iroh::endpoint::WriteError) -> SessionError {
    use iroh::endpoint::WriteError;
    match err {
        WriteError::ClosedStream | WriteError::Stopped(_) => SessionError::Closed,
        other => SessionError::Internal(format!("stream write: {other}")),
    }
}

fn map_read_err(err: iroh::endpoint::ReadExactError) -> SessionError {
    use iroh::endpoint::{ReadError, ReadExactError};
    match err {
        ReadExactError::FinishedEarly(_) => SessionError::Closed,
        ReadExactError::ReadError(ReadError::ClosedStream)
        | ReadExactError::ReadError(ReadError::Reset(_)) => SessionError::Closed,
        other => SessionError::Internal(format!("stream read: {other}")),
    }
}

/// Pick the more informative error when both discovery channels failed.
///
/// Priority:
/// * `InvitationExpired` wins over anything else — at least one channel
///   matched a real record, so it's the most informative outcome. It
///   lets the UI distinguish "stale code" from "wrong code" and prompt
///   the sponsor to issue a fresh one. A timed-out LAN branch reports
///   `InvitationNotFound`, so checking it first would wrongly mask a
///   cloud-side `Expired` as a typo.
/// * `InvitationNotFound` next — the most actionable message when no
///   channel matched anything ("check your code for typos").
/// * `ServiceUnavailable` next — accurately describes "neither channel
///   was up to answer."
/// * Otherwise, prefer the cloud-side error; cloud errors carry more
///   structured slugs than the mDNS branch.
fn prefer_dial_error(cloud: DialError, lan: DialError) -> DialError {
    use DialError::*;
    if matches!(&cloud, InvitationExpired) || matches!(&lan, InvitationExpired) {
        return InvitationExpired;
    }
    if matches!(&cloud, InvitationNotFound) || matches!(&lan, InvitationNotFound) {
        return InvitationNotFound;
    }
    if matches!(&cloud, ServiceUnavailable) && matches!(&lan, ServiceUnavailable) {
        return ServiceUnavailable;
    }
    cloud
}

/// Project rendezvous HTTP errors into the subset of `DialError` the
/// joiner-side resolve call can plausibly hit. The sponsor side never
/// produces [`DialError`], so `Unexpected`/`Parse` are reported as
/// `Internal` rather than reinterpreted.
fn map_resolve_err(err: RendezvousHttpError) -> DialError {
    match err {
        RendezvousHttpError::NotFound => DialError::InvitationNotFound,
        RendezvousHttpError::Gone => DialError::InvitationExpired,
        RendezvousHttpError::Transport(_) | RendezvousHttpError::ServiceUnavailable(_) => {
            DialError::ServiceUnavailable
        }
        // 409 on /resolve means the rendezvous entry has reached a terminal
        // state (sponsor side already called /consume; see `pairing_inbound`
        // orchestrator: it consumes as soon as the joiner's `Request` is
        // matched, which happens *before* the KeyslotOffer is sent). From
        // the joiner's POV the code is no longer usable — collapse to
        // `InvitationNotFound`, mirroring how `map_consume_err` already
        // treats 404/410/409 as a single "lifecycle terminal" bucket.
        RendezvousHttpError::Conflict => DialError::InvitationNotFound,
        RendezvousHttpError::Unexpected { status, slug } => {
            DialError::Internal(format!("rendezvous resolve: status {status} slug={slug}"))
        }
        RendezvousHttpError::Parse(msg) => {
            DialError::Internal(format!("rendezvous resolve parse: {msg}"))
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::protocol::Router;
    use iroh::RelayMode;
    use std::time::Duration;
    use tokio::task::JoinHandle;
    use uc_core::ids::DeviceId;
    use uc_core::pairing::JoinerRequest;
    use uc_core::security::IdentityFingerprint;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TEST_TIMEOUT: Duration = Duration::from_secs(5);

    // ── prefer_dial_error tests ─────────────────────────────────────────
    //
    // The race driver in `resolve_invitation` calls `prefer_dial_error`
    // when both channels failed; the choice it makes determines the
    // error variant the UI sees. These pin the priority order so the
    // user-facing message stays predictable across refactors.

    #[test]
    fn prefer_invitation_not_found_when_either_branch_says_so() {
        assert!(matches!(
            prefer_dial_error(DialError::ServiceUnavailable, DialError::InvitationNotFound),
            DialError::InvitationNotFound
        ));
        assert!(matches!(
            prefer_dial_error(DialError::InvitationNotFound, DialError::ServiceUnavailable),
            DialError::InvitationNotFound
        ));
    }

    #[test]
    fn prefer_expired_over_service_unavailable() {
        assert!(matches!(
            prefer_dial_error(DialError::ServiceUnavailable, DialError::InvitationExpired),
            DialError::InvitationExpired
        ));
    }

    #[test]
    fn prefer_expired_over_not_found() {
        // The realistic race: cloud reports `Gone` → `InvitationExpired`
        // (the directory matched a real record past its TTL) while the LAN
        // branch only times out → `InvitationNotFound`. The expired signal
        // is strictly more informative, so it must win regardless of which
        // channel produced which error.
        assert!(matches!(
            prefer_dial_error(DialError::InvitationExpired, DialError::InvitationNotFound),
            DialError::InvitationExpired
        ));
        assert!(matches!(
            prefer_dial_error(DialError::InvitationNotFound, DialError::InvitationExpired),
            DialError::InvitationExpired
        ));
    }

    #[test]
    fn prefer_service_unavailable_when_both_channels_down() {
        assert!(matches!(
            prefer_dial_error(DialError::ServiceUnavailable, DialError::ServiceUnavailable),
            DialError::ServiceUnavailable
        ));
    }

    #[test]
    fn prefer_cloud_internal_message_when_no_user_friendly_variant() {
        match prefer_dial_error(
            DialError::Internal("cloud-detail".into()),
            DialError::Internal("lan-detail".into()),
        ) {
            DialError::Internal(msg) => assert_eq!(msg, "cloud-detail"),
            other => panic!("expected Internal(cloud), got {other:?}"),
        }
    }

    async fn bound_endpoint() -> Arc<Endpoint> {
        Arc::new(
            Endpoint::builder(iroh::endpoint::presets::N0)
                .alpns(vec![PAIRING_ALPN.to_vec()])
                .relay_mode(RelayMode::Disabled)
                .bind()
                .await
                .expect("bind endpoint"),
        )
    }

    /// Build an adapter plus a rendezvous client pointed at `base_url`.
    /// For tests that drive `dial_by_invitation` against a mock server.
    fn adapter_with_rendezvous(
        endpoint: Arc<Endpoint>,
        base_url: impl Into<String>,
    ) -> IrohPairingSessionAdapter {
        IrohPairingSessionAdapter::new(
            endpoint,
            Arc::new(RendezvousClient::with_base_url(base_url)),
        )
    }

    /// Adapter with a dummy rendezvous client that is never exercised —
    /// for sponsor-side tests and ghost-session tests that never call
    /// `dial_by_invitation`. We still construct a real client (instead
    /// of an Option) so the production constructor signature stays tight.
    fn adapter_no_rendezvous(endpoint: Arc<Endpoint>) -> IrohPairingSessionAdapter {
        IrohPairingSessionAdapter::new(
            endpoint,
            Arc::new(RendezvousClient::with_base_url("http://unused.invalid")),
        )
    }

    async fn wait_for_direct_addrs(endpoint: &Endpoint) {
        // Give the magicsock a beat to publish local direct addresses.
        for _ in 0..50 {
            if !endpoint.addr().addrs.is_empty() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    /// Spawn a sponsor-side loopback that accepts the first incoming bi-stream
    /// and echoes framed bytes back. Returns the join handle so the test can
    /// drop it after assertions.
    fn spawn_echo_sponsor(endpoint: Arc<Endpoint>) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                let connection = match incoming.await {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                tokio::spawn(async move {
                    while let Ok((mut send, mut recv)) = connection.accept_bi().await {
                        loop {
                            let mut len_buf = [0u8; FRAME_LEN_BYTES];
                            if recv.read_exact(&mut len_buf).await.is_err() {
                                break;
                            }
                            let len = u32::from_be_bytes(len_buf) as usize;
                            let mut payload = vec![0u8; len];
                            if recv.read_exact(&mut payload).await.is_err() {
                                break;
                            }
                            if send.write_all(&len_buf).await.is_err() {
                                break;
                            }
                            if send.write_all(&payload).await.is_err() {
                                break;
                            }
                        }
                    }
                });
            }
        })
    }

    async fn mock_resolve(server: &MockServer, code: &str, ticket: String) {
        let body = serde_json::json!({
            "sponsorTicket": ticket,
            "sponsorEndpointId": "ignored-for-tests",
            "expiresAtMs": 0,
        });
        Mock::given(method("POST"))
            .and(path("/v1/pairings/resolve"))
            .and(wiremock::matchers::body_partial_json(
                serde_json::json!({ "code": code }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }

    fn sample_fingerprint() -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap()
    }

    fn sample_request() -> PairingSessionMessage {
        PairingSessionMessage::Request(JoinerRequest {
            invitation_code: InvitationCode::new("CODE-9999"),
            device_id: DeviceId::new("joiner-1"),
            device_name: "Joiner".into(),
            identity_fingerprint: sample_fingerprint(),
            nonce: vec![7; 8],
            transport_address_blob: vec![],
        })
    }

    #[tokio::test]
    async fn dial_send_recv_close_round_trip() {
        let sponsor_endpoint = bound_endpoint().await;
        wait_for_direct_addrs(&sponsor_endpoint).await;
        let sponsor_addr = sponsor_endpoint.addr();
        let ticket = serde_json::to_string(&sponsor_addr).unwrap();
        let _echo = spawn_echo_sponsor(sponsor_endpoint.clone());

        let rendezvous = MockServer::start().await;
        mock_resolve(&rendezvous, "CODE-9999", ticket).await;

        let joiner_endpoint = bound_endpoint().await;
        wait_for_direct_addrs(&joiner_endpoint).await;
        let adapter = adapter_with_rendezvous(joiner_endpoint, rendezvous.uri());

        let outcome = adapter
            .dial_by_invitation(&InvitationCode::new("CODE-9999"))
            .await
            .expect("dial");
        assert_eq!(outcome.channel, DiscoveryChannel::Cloud);
        let session = outcome.session_id;

        let msg = sample_request();
        adapter.send(&session, msg.clone()).await.expect("send");

        let echoed = adapter
            .recv_next(&session)
            .await
            .expect("recv")
            .expect("message");
        match (msg, echoed) {
            (PairingSessionMessage::Request(a), PairingSessionMessage::Request(b)) => {
                assert_eq!(a.invitation_code.as_str(), b.invitation_code.as_str());
                assert_eq!(a.device_id.as_str(), b.device_id.as_str());
                assert_eq!(a.identity_fingerprint, b.identity_fingerprint);
                assert_eq!(a.nonce, b.nonce);
            }
            (a, b) => panic!("variant mismatch: {a:?} vs {b:?}"),
        }

        adapter.close(&session, Some("done".into())).await;

        match adapter.send(&session, sample_request()).await {
            Err(SessionError::NotFound(id)) => assert_eq!(id.as_str(), session.as_str()),
            other => panic!("expected NotFound after close, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dial_maps_404_to_invitation_not_found() {
        let rendezvous = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/resolve"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&rendezvous)
            .await;

        let endpoint = bound_endpoint().await;
        let adapter = adapter_with_rendezvous(endpoint, rendezvous.uri());

        match adapter
            .dial_by_invitation(&InvitationCode::new("UNKNOWN"))
            .await
        {
            Err(DialError::InvitationNotFound) => {}
            other => panic!("expected InvitationNotFound, got {other:?}"),
        }
    }

    // The next three tests pin `map_resolve_err` / sponsor-ticket
    // decoding directly. Earlier revisions drove them through
    // `dial_by_invitation`, but that path now races cloud and LAN; on a
    // host where the LAN socket binds cleanly (which is the desired
    // behaviour, see F16), LAN times out as `InvitationNotFound` and
    // `prefer_dial_error` masks the cloud-side variant. Testing the
    // mapping helper directly avoids that confound and runs in
    // microseconds instead of waiting on the 5s LAN window.

    #[test]
    fn map_resolve_err_maps_gone_to_invitation_expired() {
        match map_resolve_err(RendezvousHttpError::Gone) {
            DialError::InvitationExpired => {}
            other => panic!("expected InvitationExpired, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dial_maps_409_to_invitation_not_found() {
        // Sponsor consumes the rendezvous entry as soon as it matches the
        // joiner's first Request (see `pairing_inbound::orchestrator`).
        // A subsequent /resolve on the same code therefore returns 409
        // `pairing_already_consumed`. From the joiner's POV the code is
        // dead — surface it as `InvitationNotFound` so the daemon returns
        // HTTP 404 with a meaningful message ("invitation not found")
        // instead of a 500 internal error.
        let rendezvous = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/pairings/resolve"))
            .respond_with(ResponseTemplate::new(409))
            .mount(&rendezvous)
            .await;

        let endpoint = bound_endpoint().await;
        let adapter = adapter_with_rendezvous(endpoint, rendezvous.uri());

        match adapter
            .dial_by_invitation(&InvitationCode::new("CONSUMED"))
            .await
        {
            Err(DialError::InvitationNotFound) => {}
            other => panic!("expected InvitationNotFound, got {other:?}"),
        }
    }

    #[test]
    fn map_resolve_err_maps_5xx_and_transport_to_service_unavailable() {
        match map_resolve_err(RendezvousHttpError::ServiceUnavailable(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
        )) {
            DialError::ServiceUnavailable => {}
            other => panic!("expected ServiceUnavailable from 503, got {other:?}"),
        }
        match map_resolve_err(RendezvousHttpError::Transport("dns failed".into())) {
            DialError::ServiceUnavailable => {}
            other => panic!("expected ServiceUnavailable from Transport, got {other:?}"),
        }
    }

    #[test]
    fn sponsor_ticket_decode_failure_maps_to_internal() {
        // Mirrors the inline mapping in `resolve_via_cloud` so the
        // user-facing slug stays predictable. If the format changes, the
        // assertion below pins the substring callers grep for.
        let result: Result<EndpointAddr, DialError> =
            serde_json::from_str::<EndpointAddr>("not-valid-json")
                .map_err(|err| DialError::Internal(format!("sponsor ticket decode: {err}")));

        match result {
            Err(DialError::Internal(msg)) => assert!(msg.contains("sponsor ticket decode")),
            other => panic!("expected Internal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_on_unknown_session_returns_not_found() {
        let endpoint = bound_endpoint().await;
        let adapter = adapter_no_rendezvous(endpoint);
        let ghost = PairingSessionId::new("no-such-session");

        match adapter.send(&ghost, sample_request()).await {
            Err(SessionError::NotFound(id)) => assert_eq!(id.as_str(), "no-such-session"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    // Note: a dedicated "peer finishes without sending → recv_next returns None"
    // test lives in P7c.3 where the sponsor-side handler opens a real bi-stream
    // before closing. iroh bi-streams require the dialer to write first for
    // `accept_bi()` on the responder to resolve, so a sponsor that finishes
    // without ever reading cannot be modelled faithfully here.

    // ========================================================================
    // Sponsor-side (P7c.3)
    // ========================================================================

    /// Frame the given message exactly the way
    /// [`PairingSessionPort::send`] does — used by the raw joiner in
    /// sponsor-side tests (we exercise the dialer half by hand to keep the
    /// assertion focused on the handler, not on `dial_by_invitation`).
    async fn write_framed(send: &mut SendStream, message: &PairingSessionMessage) {
        let payload = wire::encode(message).expect("wire encode");
        let len: u32 = payload.len().try_into().expect("payload fits u32");
        send.write_all(&len.to_be_bytes()).await.expect("write len");
        send.write_all(&payload).await.expect("write payload");
    }

    async fn with_timeout<F, T>(label: &'static str, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        tokio::time::timeout(TEST_TIMEOUT, fut)
            .await
            .unwrap_or_else(|_| panic!("{label} timed out after {:?}", TEST_TIMEOUT))
    }

    #[tokio::test]
    async fn sponsor_handler_emits_incoming_event_with_decoded_first_frame() {
        // Sponsor side: adapter + router on the same endpoint.
        let sponsor_endpoint = bound_endpoint().await;
        wait_for_direct_addrs(&sponsor_endpoint).await;
        let sponsor_addr = sponsor_endpoint.addr();

        let sponsor_adapter = Arc::new(adapter_no_rendezvous(sponsor_endpoint.clone()));
        let mut rx = sponsor_adapter.subscribe().await.expect("subscribe");

        let router = sponsor_adapter
            .install_handler(Router::builder((*sponsor_endpoint).clone()))
            .spawn();

        // Joiner side: raw connect + open_bi + one framed message.
        let joiner_endpoint = bound_endpoint().await;
        wait_for_direct_addrs(&joiner_endpoint).await;

        let connection = with_timeout(
            "joiner connect",
            joiner_endpoint.connect(sponsor_addr, PAIRING_ALPN),
        )
        .await
        .expect("connect");

        let (mut send, _recv) = with_timeout("open_bi", connection.open_bi())
            .await
            .expect("open_bi");

        let request = sample_request();
        write_framed(&mut send, &request).await;

        // Sponsor observes the Incoming event with the decoded payload.
        let event = with_timeout("recv event", rx.recv())
            .await
            .expect("event channel closed");

        match event {
            PairingSessionEvent::Incoming { session, message } => {
                assert!(!session.as_str().is_empty(), "session id should be minted",);
                match (request, message) {
                    (
                        PairingSessionMessage::Request(expected),
                        PairingSessionMessage::Request(got),
                    ) => {
                        assert_eq!(
                            expected.invitation_code.as_str(),
                            got.invitation_code.as_str()
                        );
                        assert_eq!(expected.device_id.as_str(), got.device_id.as_str());
                        assert_eq!(expected.identity_fingerprint, got.identity_fingerprint);
                        assert_eq!(expected.nonce, got.nonce);
                    }
                    (a, b) => panic!("variant mismatch: {a:?} vs {b:?}"),
                }
            }
            other => panic!("expected Incoming, got {other:?}"),
        }

        // Clean shutdown: router.shutdown() triggers ProtocolHandler::shutdown
        // on all registered handlers and closes the endpoint.
        with_timeout("router shutdown", router.shutdown())
            .await
            .expect("router shutdown");
    }

    #[tokio::test]
    async fn subscribe_replaces_previous_sender() {
        let endpoint = bound_endpoint().await;
        let adapter = Arc::new(adapter_no_rendezvous(endpoint));

        let mut first_rx = adapter.subscribe().await.expect("first subscribe");
        let mut second_rx = adapter.subscribe().await.expect("second subscribe");

        // The previous receiver must observe close (the old sender was
        // dropped when the new subscribe() overwrote the slot).
        match with_timeout("first rx close", first_rx.recv()).await {
            None => {}
            Some(ev) => panic!("expected channel close, got {ev:?}"),
        }

        // The new receiver is wired up but quiet (no connections yet).
        assert!(
            tokio::time::timeout(Duration::from_millis(50), second_rx.recv())
                .await
                .is_err(),
            "second receiver should be idle",
        );
    }
}
