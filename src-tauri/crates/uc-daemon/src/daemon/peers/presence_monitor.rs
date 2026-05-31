//! Presence monitor — bridges application presence events into the daemon
//! WebSocket as `peers.changed` snapshots.
//!
//! ## Why
//!
//! presence 变化由 application 入口转成稳定事件。daemon 不直接订阅
//! `PresencePort`,也不直接读取 core runtime。
//!
//! The frontend's existing `peers` topic subscribers (Setup scan page via
//! `useDeviceDiscovery`) consume the `peers.changed` full-snapshot event
//! and run their own diff. Other historical event types
//! (`peers.connectionChanged`, `peers.nameUpdated`) have no real consumer
//! today, so this monitor intentionally only forwards `peers.changed`.
//!
//! ## Design
//!
//! * Subscribes once at startup; the broadcast channel is process-local
//!   so no reconnect/backoff loop is needed.
//! * On every application presence event, fetches a fresh peer snapshot via the
//!   injected [`PeerSnapshotProvider`] and emits one `peers.changed`
//!   payload. Production derives the snapshot through `AppFacade`,keeping
//!   roster/presence 聚合规则 inside application。
//! * `RecvError::Lagged` is logged and skipped — the next event will
//!   always re-publish the canonical snapshot, so a missed transition
//!   self-heals.
//!
//! The event source and snapshot fetch are abstracted so the loop logic can
//! be unit-tested without constructing a full `AppFacade`.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use uc_application::facade::{
    connection_channel_to_wire, AppFacade, AppPresenceEvent, AppPresenceSubscription,
    AppPresenceSubscriptionError, PeerSnapshotView,
};
use uc_daemon_contract::constants::{ws_event, ws_topic};

use crate::daemon::service::{DaemonService, ServiceHealth};
use uc_webserver::api::types::{DaemonWsEvent, PeerSnapshotDto, PeersChangedFullPayload};

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn emit_ws_event<T: serde::Serialize>(
    event_tx: &broadcast::Sender<DaemonWsEvent>,
    topic: &str,
    event_type: &str,
    payload: T,
) {
    let payload = match serde_json::to_value(payload) {
        Ok(payload) => payload,
        Err(err) => {
            warn!(error = %err, topic, event_type, "failed to encode daemon websocket payload");
            return;
        }
    };

    let _ = event_tx.send(DaemonWsEvent {
        topic: topic.to_string(),
        event_type: event_type.to_string(),
        session_id: None,
        ts: now_ms(),
        payload,
    });
}

/// Source of `PeerSnapshotDto` lists, abstracted so the monitor's loop
/// logic can be unit-tested without wiring a full `AppFacade`.
#[async_trait]
pub(crate) trait PeerSnapshotProvider: Send + Sync {
    async fn fetch(&self) -> anyhow::Result<Vec<PeerSnapshotDto>>;
}

/// Source of application presence events.
#[async_trait]
pub(crate) trait PeerPresenceEventSource: Send + Sync {
    async fn subscribe(&self) -> anyhow::Result<Box<dyn PeerPresenceSubscription>>;
}

#[async_trait]
pub(crate) trait PeerPresenceSubscription: Send {
    async fn recv(&mut self) -> Result<AppPresenceEvent, AppPresenceSubscriptionError>;
}

struct AppFacadePeerSnapshotProvider {
    app_facade: Arc<AppFacade>,
}

#[async_trait]
impl PeerSnapshotProvider for AppFacadePeerSnapshotProvider {
    async fn fetch(&self) -> anyhow::Result<Vec<PeerSnapshotDto>> {
        let peers = self.app_facade.list_peer_snapshots().await?;
        Ok(peers.into_iter().map(peer_snapshot_to_dto).collect())
    }
}

struct AppFacadePresenceEventSource {
    app_facade: Arc<AppFacade>,
}

#[async_trait]
impl PeerPresenceEventSource for AppFacadePresenceEventSource {
    async fn subscribe(&self) -> anyhow::Result<Box<dyn PeerPresenceSubscription>> {
        let subscription = self.app_facade.subscribe_peer_presence_events()?;
        Ok(Box::new(AppFacadePresenceSubscription { subscription }))
    }
}

struct AppFacadePresenceSubscription {
    subscription: AppPresenceSubscription,
}

#[async_trait]
impl PeerPresenceSubscription for AppFacadePresenceSubscription {
    async fn recv(&mut self) -> Result<AppPresenceEvent, AppPresenceSubscriptionError> {
        self.subscription.recv().await
    }
}

pub struct PresenceMonitor {
    event_source: Arc<dyn PeerPresenceEventSource>,
    snapshot_provider: Arc<dyn PeerSnapshotProvider>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
}

impl PresenceMonitor {
    pub fn new(app_facade: Arc<AppFacade>, event_tx: broadcast::Sender<DaemonWsEvent>) -> Self {
        let event_source: Arc<dyn PeerPresenceEventSource> =
            Arc::new(AppFacadePresenceEventSource {
                app_facade: Arc::clone(&app_facade),
            });
        let snapshot_provider: Arc<dyn PeerSnapshotProvider> =
            Arc::new(AppFacadePeerSnapshotProvider {
                app_facade: Arc::clone(&app_facade),
            });
        Self::with_dependencies(event_source, snapshot_provider, event_tx)
    }

    pub(crate) fn with_dependencies(
        event_source: Arc<dyn PeerPresenceEventSource>,
        snapshot_provider: Arc<dyn PeerSnapshotProvider>,
        event_tx: broadcast::Sender<DaemonWsEvent>,
    ) -> Self {
        Self {
            event_source,
            snapshot_provider,
            event_tx,
        }
    }

    async fn publish_snapshot(&self) {
        match self.snapshot_provider.fetch().await {
            Ok(peers) => {
                emit_ws_event(
                    &self.event_tx,
                    ws_topic::PEERS,
                    ws_event::PEERS_CHANGED,
                    PeersChangedFullPayload { peers },
                );
            }
            Err(err) => {
                warn!(
                    error = %err,
                    "presence monitor: failed to fetch peer snapshot for peers.changed"
                );
            }
        }
    }
}

#[async_trait]
impl DaemonService for PresenceMonitor {
    fn name(&self) -> &str {
        "presence-monitor"
    }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        info!("presence monitor starting");
        let mut rx = self.event_source.subscribe().await?;

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    debug!("presence monitor cancelled");
                    return Ok(());
                }
                result = rx.recv() => {
                    match result {
                        Ok(event) => {
                            debug!(
                                device = %event.device_id,
                                state = %event.state,
                                "presence monitor: state change → publishing peers.changed"
                            );
                            self.publish_snapshot().await;
                        }
                        Err(AppPresenceSubscriptionError::Lagged(skipped)) => {
                            warn!(
                                skipped,
                                "presence monitor: dropped events (lagged); next event will re-publish snapshot"
                            );
                        }
                        Err(AppPresenceSubscriptionError::Closed) => {
                            warn!("presence monitor: subscription closed; exiting loop");
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn health_check(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}

fn peer_snapshot_to_dto(peer: PeerSnapshotView) -> PeerSnapshotDto {
    PeerSnapshotDto {
        channel: connection_channel_to_wire(peer.channel).to_string(),
        peer_id: peer.peer_id,
        device_name: peer.device_name,
        addresses: peer.addresses,
        is_paired: peer.is_paired,
        connected: peer.connected,
        pairing_state: peer.pairing_state,
        connection_address: peer.connection_address,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;
    use std::time::Duration;

    use tokio::time::timeout;

    /// Fake presence event source whose `subscribe()` hands out receivers
    /// attached to a caller-controlled broadcast::Sender.
    struct FakePresenceEventSource {
        tx: Mutex<Option<broadcast::Sender<AppPresenceEvent>>>,
    }

    impl FakePresenceEventSource {
        fn new(capacity: usize) -> (Arc<Self>, broadcast::Sender<AppPresenceEvent>) {
            let (tx, _) = broadcast::channel(capacity);
            let source = Arc::new(Self {
                tx: Mutex::new(Some(tx.clone())),
            });
            (source, tx)
        }

        fn close(&self) {
            self.tx.lock().unwrap().take();
        }
    }

    #[async_trait]
    impl PeerPresenceEventSource for FakePresenceEventSource {
        async fn subscribe(&self) -> anyhow::Result<Box<dyn PeerPresenceSubscription>> {
            let rx = self
                .tx
                .lock()
                .unwrap()
                .as_ref()
                .expect("FakePresenceEventSource closed before subscribe")
                .subscribe();
            Ok(Box::new(FakePresenceSubscription { rx }))
        }
    }

    struct FakePresenceSubscription {
        rx: broadcast::Receiver<AppPresenceEvent>,
    }

    #[async_trait]
    impl PeerPresenceSubscription for FakePresenceSubscription {
        async fn recv(&mut self) -> Result<AppPresenceEvent, AppPresenceSubscriptionError> {
            self.rx.recv().await.map_err(|err| match err {
                broadcast::error::RecvError::Lagged(skipped) => {
                    AppPresenceSubscriptionError::Lagged(skipped)
                }
                broadcast::error::RecvError::Closed => AppPresenceSubscriptionError::Closed,
            })
        }
    }

    /// Fake snapshot provider — counts calls and either returns a canned
    /// list or a synthetic error.
    #[derive(Default)]
    struct FakeSnapshotProvider {
        inner: Mutex<FakeSnapshotInner>,
    }

    #[derive(Default)]
    struct FakeSnapshotInner {
        call_count: usize,
        succeed_with: Vec<PeerSnapshotDto>,
        fail: bool,
    }

    impl FakeSnapshotProvider {
        fn returning(peers: Vec<PeerSnapshotDto>) -> Arc<Self> {
            Arc::new(Self {
                inner: Mutex::new(FakeSnapshotInner {
                    call_count: 0,
                    succeed_with: peers,
                    fail: false,
                }),
            })
        }

        fn failing() -> Arc<Self> {
            Arc::new(Self {
                inner: Mutex::new(FakeSnapshotInner {
                    call_count: 0,
                    succeed_with: Vec::new(),
                    fail: true,
                }),
            })
        }

        fn call_count(&self) -> usize {
            self.inner.lock().unwrap().call_count
        }
    }

    #[async_trait]
    impl PeerSnapshotProvider for FakeSnapshotProvider {
        async fn fetch(&self) -> anyhow::Result<Vec<PeerSnapshotDto>> {
            let mut guard = self.inner.lock().unwrap();
            guard.call_count += 1;
            if guard.fail {
                Err(anyhow::anyhow!("synthetic fetch failure"))
            } else {
                Ok(guard.succeed_with.clone())
            }
        }
    }

    fn online_event(device: &str) -> AppPresenceEvent {
        AppPresenceEvent {
            device_id: device.to_string(),
            state: "online".to_string(),
            at_ms: 0,
        }
    }

    fn sample_dto(peer_id: &str) -> PeerSnapshotDto {
        PeerSnapshotDto {
            peer_id: peer_id.to_string(),
            device_name: Some("alpha".to_string()),
            addresses: vec![],
            is_paired: true,
            connected: true,
            pairing_state: "Trusted".to_string(),
            channel: "unknown".to_string(),
            connection_address: None,
        }
    }

    /// Wait for the next `peers.changed` event on the daemon broadcast,
    /// skipping any unrelated events. Returns `None` on timeout.
    async fn next_peers_changed(
        rx: &mut broadcast::Receiver<DaemonWsEvent>,
        wait: Duration,
    ) -> Option<DaemonWsEvent> {
        timeout(wait, async {
            loop {
                match rx.recv().await {
                    Ok(ev)
                        if ev.topic == ws_topic::PEERS
                            && ev.event_type == ws_event::PEERS_CHANGED =>
                    {
                        return ev;
                    }
                    Ok(_) => continue,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => {
                        unreachable!("ws event sender dropped before peers.changed arrived")
                    }
                }
            }
        })
        .await
        .ok()
    }

    #[tokio::test]
    async fn presence_event_publishes_peers_changed() {
        let (presence, presence_tx) = FakePresenceEventSource::new(8);
        let snapshots = FakeSnapshotProvider::returning(vec![sample_dto("peer-a")]);
        let (event_tx, mut event_rx) = broadcast::channel::<DaemonWsEvent>(8);

        let monitor = Arc::new(PresenceMonitor::with_dependencies(
            presence,
            snapshots.clone(),
            event_tx,
        ));
        let cancel = CancellationToken::new();
        let task = {
            let monitor = monitor.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move { monitor.start(cancel).await })
        };

        // Give the monitor a chance to subscribe before we publish.
        tokio::task::yield_now().await;
        presence_tx.send(online_event("device-1")).unwrap();

        let ev = next_peers_changed(&mut event_rx, Duration::from_millis(500))
            .await
            .expect("peers.changed should be published");

        assert_eq!(ev.topic, ws_topic::PEERS);
        assert_eq!(ev.event_type, ws_event::PEERS_CHANGED);
        let peers = ev.payload.get("peers").and_then(|v| v.as_array()).unwrap();
        assert_eq!(peers.len(), 1);
        assert_eq!(
            peers[0].get("peerId").and_then(|v| v.as_str()),
            Some("peer-a")
        );
        assert_eq!(snapshots.call_count(), 1);

        cancel.cancel();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn each_event_triggers_independent_snapshot() {
        let (presence, presence_tx) = FakePresenceEventSource::new(8);
        let snapshots = FakeSnapshotProvider::returning(vec![sample_dto("peer-a")]);
        let (event_tx, mut event_rx) = broadcast::channel::<DaemonWsEvent>(8);

        let monitor = Arc::new(PresenceMonitor::with_dependencies(
            presence,
            snapshots.clone(),
            event_tx,
        ));
        let cancel = CancellationToken::new();
        let task = {
            let monitor = monitor.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move { monitor.start(cancel).await })
        };

        tokio::task::yield_now().await;
        presence_tx.send(online_event("device-1")).unwrap();
        next_peers_changed(&mut event_rx, Duration::from_millis(500))
            .await
            .expect("first peers.changed");

        presence_tx.send(online_event("device-2")).unwrap();
        next_peers_changed(&mut event_rx, Duration::from_millis(500))
            .await
            .expect("second peers.changed");

        assert_eq!(snapshots.call_count(), 2);

        cancel.cancel();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn snapshot_failure_does_not_exit_loop() {
        let (presence, presence_tx) = FakePresenceEventSource::new(8);
        let snapshots = FakeSnapshotProvider::failing();
        let (event_tx, mut event_rx) = broadcast::channel::<DaemonWsEvent>(8);

        let monitor = Arc::new(PresenceMonitor::with_dependencies(
            presence,
            snapshots.clone(),
            event_tx,
        ));
        let cancel = CancellationToken::new();
        let task = {
            let monitor = monitor.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move { monitor.start(cancel).await })
        };

        tokio::task::yield_now().await;
        presence_tx.send(online_event("device-1")).unwrap();
        // Failure path: no peers.changed should be emitted.
        assert!(
            next_peers_changed(&mut event_rx, Duration::from_millis(150))
                .await
                .is_none(),
            "peers.changed should not be emitted on snapshot failure"
        );
        assert_eq!(snapshots.call_count(), 1);

        // Loop must still be alive — a second event also triggers a fetch.
        presence_tx.send(online_event("device-2")).unwrap();
        // Allow the monitor to drain the second event before sampling.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(snapshots.call_count(), 2);

        cancel.cancel();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn cancellation_exits_loop() {
        let (presence, _presence_tx) = FakePresenceEventSource::new(8);
        let snapshots = FakeSnapshotProvider::returning(vec![]);
        let (event_tx, _event_rx) = broadcast::channel::<DaemonWsEvent>(8);

        let monitor = Arc::new(PresenceMonitor::with_dependencies(
            presence, snapshots, event_tx,
        ));
        let cancel = CancellationToken::new();
        let task = {
            let monitor = monitor.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move { monitor.start(cancel).await })
        };

        cancel.cancel();
        timeout(Duration::from_millis(500), task)
            .await
            .expect("monitor should exit promptly after cancel")
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn closed_subscription_exits_loop() {
        let (presence, presence_tx) = FakePresenceEventSource::new(8);
        let snapshots = FakeSnapshotProvider::returning(vec![]);
        let (event_tx, _event_rx) = broadcast::channel::<DaemonWsEvent>(8);

        let monitor = Arc::new(PresenceMonitor::with_dependencies(
            presence.clone(),
            snapshots,
            event_tx,
        ));
        let cancel = CancellationToken::new();
        let task = {
            let monitor = monitor.clone();
            let cancel = cancel.clone();
            tokio::spawn(async move { monitor.start(cancel).await })
        };

        // Let the monitor subscribe, then drop both sender handles
        // (external + the one held inside FakePresenceEventSource) so the
        // subscription returns RecvError::Closed.
        tokio::task::yield_now().await;
        drop(presence_tx);
        presence.close();

        timeout(Duration::from_millis(500), task)
            .await
            .expect("monitor should exit promptly after sender drop")
            .unwrap()
            .unwrap();
    }
}
