//! 反向"传输进度"通道:接收端把 fetch blob 的字节级进度推回数据来源端
//! (sender),让 sender UI 能展示对端真实接收进度。
//!
//! ## 协议形态
//!
//! * **方向**:receiver → sender(仅一向)。
//! * **传输**:专用 ALPN [`TRANSFER_PROGRESS_ALPN`] 上的 iroh `accept_uni`
//!   单向流;每帧一个 uni stream,connection 在 transfer 期间持续复用。
//! * **wire**:见 [`super::transfer_progress_wire`] —— 固定 34 字节定长帧。
//!
//! ## 身份验证
//!
//! 与 [`super::clipboard_receiver_adapter`] 一致:sender 端 handler 用
//! `Connection::remote_id()` 拿到对端 Ed25519 公钥,经
//! `IdentityFingerprintFactoryPort` 派生出 fingerprint,在
//! `MemberRepositoryPort` 中查匹配的 `SpaceMember`。陌生 peer 的连接被
//! 直接丢弃,不进入广播,避免被伪造进度污染 UI。
//!
//! ## 失败语义
//!
//! Reporter (receiver 端 client) 的 `report` 方法是 fire-and-forget:
//! 上报失败仅记录日志,不让 fetch 主路径感知。这呼应
//! [`OutboundProgressReporterPort`] 的契约 —— 进度反向通道断了对接收端
//! 业务并不致命。

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler};
use iroh::{Endpoint, EndpointAddr};
use tokio::sync::{broadcast, Mutex};
use tracing::{debug, instrument, trace, warn};

use uc_core::file_transfer::{OutboundProgressReporterPort, OutboundProgressStatus};
use uc_core::ids::DeviceId;
use uc_core::membership::MemberRepositoryPort;
use uc_core::ports::security::IdentityFingerprintFactoryPort;
use uc_core::ports::PeerAddressRepositoryPort;

use super::connect::connect_with_staggered_retry;
use super::transfer_progress_wire::{
    self, transfer_id_from_bytes, transfer_id_to_bytes, ProgressFrame,
};

/// ALPN identifier for the reverse-direction transfer progress protocol.
/// Disjoint from the forward clipboard / presence / blobs ALPNs so the
/// shared Router multiplexes correctly.
pub const TRANSFER_PROGRESS_ALPN: &[u8] = b"uniclipboard/transfer-progress/0";

/// Capacity of the inbound progress broadcast. Sized for typical fan-in:
/// even a 10-peer space rarely produces more than a few in-flight transfers
/// concurrently, and the application-side worker drains the queue
/// continuously (one event per frame).
const PROGRESS_BROADCAST_CAPACITY: usize = 256;

/// 一帧从 receiver 推回来的进度,身份验证已完成,wire 字段已映射到领域类型。
///
/// `from_device` 是已通过 `MemberRepositoryPort` 验证过的对端 DeviceId。
/// `transfer_id` 是 sender 端的 EntryId(UUID v4 字符串),sender 用它
/// 索引本地 entry 把进度送到对应的 UI 行。
#[derive(Debug, Clone)]
pub struct InboundProgressEvent {
    pub from_device: DeviceId,
    pub transfer_id: String,
    pub bytes_transferred: u64,
    pub total_bytes: Option<u64>,
    pub status: OutboundProgressStatus,
}

/// Sender 端 + receiver 端的统一组件。它装得下:
///
/// * **Sender 端**:接收 receiver 推回来的 progress 帧,广播给应用层。
/// * **Receiver 端**:实现 [`OutboundProgressReporterPort`],把本地
///   fetch 字节进度通过反向 ALPN 推给 sender。
///
/// 同一 endpoint 同时持有两个角色 —— 设备 A 复制内容并发给设备 B 时,
/// A 是 sender(走 accept handler),B 是 receiver(走 reporter)。在 B
/// 复制内容并发给 A 的对称场景里,角色互换,但还是同一对象。
pub struct IrohTransferProgressAdapter {
    event_tx: broadcast::Sender<InboundProgressEvent>,
    handler_state: Arc<HandlerState>,
    reporter: Arc<ReporterImpl>,
}

struct HandlerState {
    member_repo: Arc<dyn MemberRepositoryPort>,
    fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort>,
    event_tx: broadcast::Sender<InboundProgressEvent>,
}

impl IrohTransferProgressAdapter {
    pub fn new(
        endpoint: Arc<Endpoint>,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
        member_repo: Arc<dyn MemberRepositoryPort>,
        fingerprint_factory: Arc<dyn IdentityFingerprintFactoryPort>,
    ) -> Self {
        let (event_tx, _) = broadcast::channel(PROGRESS_BROADCAST_CAPACITY);
        Self {
            event_tx: event_tx.clone(),
            handler_state: Arc::new(HandlerState {
                member_repo,
                fingerprint_factory,
                event_tx,
            }),
            reporter: Arc::new(ReporterImpl {
                endpoint,
                peer_addr_repo,
                connections: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Sender 端的 [`ProtocolHandler`]。注册到 Router 上接收 receiver 推送。
    pub fn handler(&self) -> IrohTransferProgressHandler {
        IrohTransferProgressHandler {
            state: Arc::clone(&self.handler_state),
        }
    }

    /// Sender 端订阅 progress 事件流。application 层起一个 worker 接事件
    /// 翻译为 host event。
    pub fn subscribe(&self) -> broadcast::Receiver<InboundProgressEvent> {
        self.event_tx.subscribe()
    }

    /// Receiver 端 reporter,作为端口实现交给 application。
    pub fn reporter(&self) -> Arc<dyn OutboundProgressReporterPort> {
        Arc::clone(&self.reporter) as Arc<dyn OutboundProgressReporterPort>
    }
}

// ============================================================================
// Sender 端:accept handler
// ============================================================================

#[derive(Clone)]
pub struct IrohTransferProgressHandler {
    state: Arc<HandlerState>,
}

impl std::fmt::Debug for IrohTransferProgressHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IrohTransferProgressHandler")
            .finish_non_exhaustive()
    }
}

impl ProtocolHandler for IrohTransferProgressHandler {
    async fn accept(&self, connection: Connection) -> Result<(), AcceptError> {
        let remote = connection.remote_id();
        let remote_bytes: [u8; 32] = *remote.as_bytes();

        // 1. Resolve remote identity. Unknown peer → drop the connection
        //    silently. We deliberately don't write any response (this is a
        //    push-only direction: receiver doesn't expect any data back).
        let from_device = match self.state.resolve_device(&remote_bytes).await {
            Some(d) => d,
            None => {
                warn!(remote = %remote, "transfer progress: unknown peer fingerprint; dropping");
                return Ok(());
            }
        };

        debug!(
            from_device = %from_device.as_str(),
            "transfer progress: accepted connection from known peer",
        );

        // 2. Loop accepting uni-streams. Receiver writes one frame per
        //    stream; the connection itself stays open for the duration of
        //    the transfer (and beyond, until QUIC idle timeout). When the
        //    receiver closes the connection or transport tears down,
        //    accept_uni() errors and we drop out of the loop.
        loop {
            let mut recv = match connection.accept_uni().await {
                Ok(stream) => stream,
                Err(err) => {
                    debug!(
                        from_device = %from_device.as_str(),
                        error = %err,
                        "transfer progress: connection closed",
                    );
                    break;
                }
            };

            match transfer_progress_wire::read_frame(&mut recv).await {
                Ok(frame) => {
                    let event = InboundProgressEvent {
                        from_device: from_device.clone(),
                        transfer_id: transfer_id_from_bytes(&frame.transfer_id_bytes),
                        bytes_transferred: frame.bytes_transferred,
                        total_bytes: frame.total_bytes,
                        status: frame.status,
                    };
                    trace!(
                        from_device = %event.from_device.as_str(),
                        transfer_id = %event.transfer_id,
                        bytes = event.bytes_transferred,
                        "transfer progress: frame received",
                    );
                    if self.state.event_tx.send(event).is_err() {
                        debug!("transfer progress: no subscribers; frame dropped");
                    }
                }
                Err(err) => {
                    warn!(
                        from_device = %from_device.as_str(),
                        error = %err,
                        "transfer progress: frame decode failed",
                    );
                    // Bad frame doesn't tear down the whole connection;
                    // just drop this stream and wait for the next one.
                    continue;
                }
            }
        }

        Ok(())
    }
}

impl HandlerState {
    /// Resolve `remote_id()` bytes to a known SpaceMember's DeviceId.
    /// Mirrors `clipboard_receiver_adapter::HandlerState::resolve_device`
    /// but doesn't share code with it (different broadcast types,
    /// different state struct).
    async fn resolve_device(&self, remote_pubkey_bytes: &[u8; 32]) -> Option<DeviceId> {
        let derived = self
            .fingerprint_factory
            .from_public_key(remote_pubkey_bytes)
            .ok()?;
        let members = self.member_repo.list().await.ok()?;
        members
            .into_iter()
            .find(|m| m.identity_fingerprint == derived)
            .map(|m| m.device_id)
    }
}

// ============================================================================
// Receiver 端:reporter (实现 OutboundProgressReporterPort)
// ============================================================================

struct ReporterImpl {
    endpoint: Arc<Endpoint>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    /// Per-target connection cache, keyed by `DeviceId.as_str()` since the
    /// domain id type doesn't derive Hash. iroh `Connection` is internally
    /// `Arc<Inner>` so cloning is cheap; reusing a connection across
    /// frames avoids a fresh QUIC handshake (and a fresh hole-punch in
    /// the worst case) per 256KB progress tick. A dead connection is
    /// detected via `close_reason()` and replaced lazily.
    connections: Mutex<HashMap<String, Connection>>,
}

#[async_trait]
impl OutboundProgressReporterPort for ReporterImpl {
    #[instrument(skip(self), fields(target = %target.as_str(), bytes = bytes_transferred))]
    async fn report(
        &self,
        target: &DeviceId,
        transfer_id: &str,
        bytes_transferred: u64,
        total_bytes: Option<u64>,
        status: OutboundProgressStatus,
    ) {
        let Some(transfer_id_bytes) = transfer_id_to_bytes(transfer_id) else {
            warn!(
                transfer_id,
                "progress reporter: transfer_id is not a uuid; skipping"
            );
            return;
        };
        let frame = ProgressFrame {
            transfer_id_bytes,
            bytes_transferred,
            total_bytes,
            status,
        };
        if let Err(err) = self.send_frame(target, &frame).await {
            warn!(error = %err, "progress reporter: send failed");
        }
    }
}

impl ReporterImpl {
    async fn resolve_addr(&self, target: &DeviceId) -> Option<EndpointAddr> {
        let record = self.peer_addr_repo.get(target).await.ok().flatten()?;
        match postcard::from_bytes::<EndpointAddr>(&record.addr_blob) {
            Ok(addr) => Some(addr),
            Err(err) => {
                warn!(
                    target = %target.as_str(),
                    error = %err,
                    "progress reporter: peer_addr_repo blob decode failed",
                );
                None
            }
        }
    }

    /// Get a live connection for `target`. Caches connections per peer;
    /// re-dials on first use or after the cached connection has been
    /// closed by the remote / transport.
    async fn acquire_connection(&self, target: &DeviceId) -> Result<Connection, ReporterError> {
        let target_key = target.as_str().to_string();
        // Fast path: cached connection still alive.
        {
            let map = self.connections.lock().await;
            if let Some(conn) = map.get(&target_key) {
                if conn.close_reason().is_none() {
                    return Ok(conn.clone());
                }
            }
        }

        let addr = self
            .resolve_addr(target)
            .await
            .ok_or(ReporterError::Offline)?;
        let connection = connect_with_staggered_retry(
            Arc::clone(&self.endpoint),
            addr,
            TRANSFER_PROGRESS_ALPN,
            "transfer-progress",
        )
        .await
        .map_err(ReporterError::Dial)?;

        self.connections
            .lock()
            .await
            .insert(target_key, connection.clone());
        Ok(connection)
    }

    async fn send_frame(
        &self,
        target: &DeviceId,
        frame: &ProgressFrame,
    ) -> Result<(), ReporterError> {
        let connection = self.acquire_connection(target).await?;
        let mut send = match connection.open_uni().await {
            Ok(s) => s,
            Err(err) => {
                // Cached connection raced us into a closed state. Evict
                // and let the next `report` re-dial; we drop *this* frame
                // intentionally — progress events are stateless ticks,
                // skipping one is harmless.
                self.connections.lock().await.remove(target.as_str());
                return Err(ReporterError::Io(format!("open_uni: {err}")));
            }
        };
        transfer_progress_wire::write_frame(&mut send, frame)
            .await
            .map_err(|err| ReporterError::Io(format!("write_frame: {err}")))?;
        send.finish()
            .map_err(|err| ReporterError::Io(format!("send.finish: {err}")))?;
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
enum ReporterError {
    #[error("offline (no peer addr or unreachable)")]
    Offline,
    #[error("dial failed: {0}")]
    Dial(String),
    #[error("io: {0}")]
    Io(String),
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
    use iroh::{Endpoint, RelayMode, SecretKey};

    use uc_core::membership::{MembershipError, SpaceMember};
    use uc_core::ports::{PeerAddressError, PeerAddressRecord};
    use uc_core::MemberSyncPreferences;

    use crate::security::Sha256IdentityFingerprintFactory;

    // ----- test doubles ------------------------------------------------------

    #[derive(Default)]
    struct MemMemberRepo {
        inner: StdMutex<StdHashMap<String, SpaceMember>>,
    }
    #[async_trait]
    impl MemberRepositoryPort for MemMemberRepo {
        async fn get(&self, device_id: &DeviceId) -> Result<Option<SpaceMember>, MembershipError> {
            Ok(self.inner.lock().unwrap().get(device_id.as_str()).cloned())
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

    #[derive(Default)]
    struct MemPeerAddrRepo {
        inner: StdMutex<StdHashMap<String, PeerAddressRecord>>,
    }
    #[async_trait]
    impl PeerAddressRepositoryPort for MemPeerAddrRepo {
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

    async fn bind_endpoint_with(seed: [u8; 32]) -> Arc<Endpoint> {
        Arc::new(
            Endpoint::builder(iroh::endpoint::presets::N0)
                .secret_key(SecretKey::from_bytes(&seed))
                .alpns(vec![TRANSFER_PROGRESS_ALPN.to_vec()])
                .relay_mode(RelayMode::Disabled)
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

    fn make_member(seed: [u8; 32], device_id: &str) -> SpaceMember {
        let factory = Sha256IdentityFingerprintFactory;
        let sk = SecretKey::from_bytes(&seed);
        let fp = factory
            .from_public_key(sk.public().as_bytes())
            .expect("derive fingerprint for test member");
        SpaceMember {
            device_id: DeviceId::new(device_id),
            device_name: "Test".to_string(),
            identity_fingerprint: fp,
            joined_at: Utc::now(),
            sync_preferences: MemberSyncPreferences::default(),
        }
    }

    /// Spin up a sender (accept side) endpoint with the progress handler
    /// installed and return its addr + a subscriber to inbound events.
    async fn spawn_sender_side(
        seed: [u8; 32],
        member_repo: Arc<dyn MemberRepositoryPort>,
    ) -> (
        Arc<Endpoint>,
        Router,
        broadcast::Receiver<InboundProgressEvent>,
    ) {
        let endpoint = bind_endpoint_with(seed).await;
        wait_for_direct_addrs(&endpoint).await;
        let adapter = IrohTransferProgressAdapter::new(
            Arc::clone(&endpoint),
            // Sender side doesn't use peer_addr_repo (no reporter calls
            // happen here in this test), so any impl is fine.
            Arc::new(MemPeerAddrRepo::default()),
            member_repo,
            Arc::new(Sha256IdentityFingerprintFactory),
        );
        let rx = adapter.subscribe();
        let router = Router::builder((*endpoint).clone())
            .accept(TRANSFER_PROGRESS_ALPN, adapter.handler())
            .spawn();
        (endpoint, router, rx)
    }

    /// Single happy-path frame. Receiver-side reporter dials sender,
    /// pushes one InProgress frame, sender broadcasts the corresponding
    /// `InboundProgressEvent` with the sender-side resolved DeviceId.
    #[tokio::test]
    async fn reporter_pushes_frame_and_sender_broadcasts_inbound_event() {
        let sender_seed = [0x11u8; 32];
        let receiver_seed = [0x22u8; 32];

        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        let receiver_member = make_member(receiver_seed, "receiver-x");
        member_repo.save(&receiver_member).await.unwrap();

        let (sender_endpoint, sender_router, mut rx) =
            spawn_sender_side(sender_seed, Arc::clone(&member_repo)).await;
        let sender_addr = sender_endpoint.addr();

        // Receiver side uses an adapter pointing back at the sender's addr.
        let receiver_endpoint = bind_endpoint_with(receiver_seed).await;
        wait_for_direct_addrs(&receiver_endpoint).await;

        let peer_addr_repo: Arc<dyn PeerAddressRepositoryPort> =
            Arc::new(MemPeerAddrRepo::default());
        peer_addr_repo
            .upsert(&PeerAddressRecord {
                device_id: DeviceId::new("sender-x"),
                addr_blob: postcard::to_stdvec(&sender_addr).unwrap(),
                observed_at: Utc::now(),
            })
            .await
            .unwrap();

        let receiver_adapter = IrohTransferProgressAdapter::new(
            Arc::clone(&receiver_endpoint),
            Arc::clone(&peer_addr_repo),
            Arc::new(MemMemberRepo::default()),
            Arc::new(Sha256IdentityFingerprintFactory),
        );
        let reporter = receiver_adapter.reporter();

        let transfer_uuid = "11111111-2222-4333-8444-555555555555";
        reporter
            .report(
                &DeviceId::new("sender-x"),
                transfer_uuid,
                42 * 1024 * 1024,
                Some(80 * 1024 * 1024),
                OutboundProgressStatus::InProgress,
            )
            .await;

        let event = tokio::time::timeout(Duration::from_secs(3), rx.recv())
            .await
            .expect("inbound event arrives")
            .expect("subscriber sees event");

        assert_eq!(event.from_device.as_str(), "receiver-x");
        assert_eq!(event.transfer_id, transfer_uuid);
        assert_eq!(event.bytes_transferred, 42 * 1024 * 1024);
        assert_eq!(event.total_bytes, Some(80 * 1024 * 1024));
        assert!(matches!(event.status, OutboundProgressStatus::InProgress));

        sender_router.shutdown().await.ok();
    }

    /// Unknown receiver — sender's member_repo is empty so the handler
    /// drops the connection without broadcasting anything.
    #[tokio::test]
    async fn unknown_receiver_drops_connection_without_broadcast() {
        let sender_seed = [0x33u8; 32];
        let receiver_seed = [0x44u8; 32];

        let member_repo: Arc<dyn MemberRepositoryPort> = Arc::new(MemMemberRepo::default());
        // intentionally empty — receiver fingerprint won't resolve
        let (sender_endpoint, sender_router, mut rx) =
            spawn_sender_side(sender_seed, Arc::clone(&member_repo)).await;
        let sender_addr = sender_endpoint.addr();

        let receiver_endpoint = bind_endpoint_with(receiver_seed).await;
        wait_for_direct_addrs(&receiver_endpoint).await;

        let peer_addr_repo: Arc<dyn PeerAddressRepositoryPort> =
            Arc::new(MemPeerAddrRepo::default());
        peer_addr_repo
            .upsert(&PeerAddressRecord {
                device_id: DeviceId::new("sender-y"),
                addr_blob: postcard::to_stdvec(&sender_addr).unwrap(),
                observed_at: Utc::now(),
            })
            .await
            .unwrap();

        let receiver_adapter = IrohTransferProgressAdapter::new(
            Arc::clone(&receiver_endpoint),
            Arc::clone(&peer_addr_repo),
            Arc::new(MemMemberRepo::default()),
            Arc::new(Sha256IdentityFingerprintFactory),
        );
        let reporter = receiver_adapter.reporter();

        reporter
            .report(
                &DeviceId::new("sender-y"),
                "11111111-2222-4333-8444-555555555555",
                100,
                Some(1000),
                OutboundProgressStatus::InProgress,
            )
            .await;

        // No subscriber event should arrive.
        let polled = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
        assert!(polled.is_err(), "unknown receiver must not broadcast");

        sender_router.shutdown().await.ok();
    }

    /// Reporter with a peer that has no addr — silently logs and returns.
    #[tokio::test]
    async fn reporter_silently_skips_when_target_has_no_address() {
        let receiver_endpoint = bind_endpoint_with([0x55u8; 32]).await;
        wait_for_direct_addrs(&receiver_endpoint).await;
        let peer_addr_repo: Arc<dyn PeerAddressRepositoryPort> =
            Arc::new(MemPeerAddrRepo::default());
        let adapter = IrohTransferProgressAdapter::new(
            Arc::clone(&receiver_endpoint),
            peer_addr_repo,
            Arc::new(MemMemberRepo::default()),
            Arc::new(Sha256IdentityFingerprintFactory),
        );
        let reporter = adapter.reporter();

        // Should not panic, should not block, just logs and returns.
        reporter
            .report(
                &DeviceId::new("never-paired"),
                "11111111-2222-4333-8444-555555555555",
                10,
                Some(100),
                OutboundProgressStatus::InProgress,
            )
            .await;
    }

    /// Non-uuid transfer_id → reporter logs + drops without dialing.
    #[tokio::test]
    async fn reporter_skips_non_uuid_transfer_id() {
        let receiver_endpoint = bind_endpoint_with([0x66u8; 32]).await;
        wait_for_direct_addrs(&receiver_endpoint).await;
        let peer_addr_repo: Arc<dyn PeerAddressRepositoryPort> =
            Arc::new(MemPeerAddrRepo::default());
        let adapter = IrohTransferProgressAdapter::new(
            Arc::clone(&receiver_endpoint),
            peer_addr_repo,
            Arc::new(MemMemberRepo::default()),
            Arc::new(Sha256IdentityFingerprintFactory),
        );
        let reporter = adapter.reporter();

        reporter
            .report(
                &DeviceId::new("anybody"),
                "not-a-uuid",
                10,
                Some(100),
                OutboundProgressStatus::InProgress,
            )
            .await;
    }
}
