use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex as TokioMutex, Notify};
use tokio::time::sleep;
use tokio_tungstenite::client_async;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use uc_core::ports::realtime::{
    PairedDevicesChangedEvent, PairingCompleteEvent, PairingFailedEvent, PairingUpdatedEvent,
    PairingVerificationRequiredEvent, PeerChangedEvent, PeerConnectionChangedEvent,
    PeerNameUpdatedEvent, RealtimeEvent, RealtimePeerSummary, RealtimeTopic, RealtimeTopicPort,
    SetupSpaceAccessCompletedEvent, SetupStateChangedEvent, SpaceAccessStateChangedEvent,
};
use uc_daemon::api::auth::DaemonConnectionInfo;
use uc_daemon::api::types::{
    DaemonWsEvent, PairedDevicesChangedPayload, PairingFailurePayload,
    PairingSessionChangedPayload, PairingVerificationPayload, PeerConnectionChangedPayload,
    PeerNameUpdatedPayload, PeersChangedFullPayload, SetupSpaceAccessCompletedPayload,
    SetupStateChangedPayload, SpaceAccessStateChangedPayload,
};

use crate::DaemonConnectionState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeState {
    Disconnected,
    Connecting,
    Subscribing,
    Ready,
    Degraded,
}

#[derive(Debug, Clone)]
pub struct DaemonWsBridgeConfig {
    pub queue_capacity: usize,
    pub terminal_retry_delay: Duration,
    pub backoff_initial: Duration,
    pub backoff_max: Duration,
}

impl Default for DaemonWsBridgeConfig {
    fn default() -> Self {
        Self {
            queue_capacity: 64,
            terminal_retry_delay: Duration::from_millis(50),
            backoff_initial: Duration::from_millis(250),
            backoff_max: Duration::from_millis(30_000),
        }
    }
}

#[derive(Default)]
pub struct ScriptedDaemonWsConnector {
    queued_connections: TokioMutex<VecDeque<Vec<DaemonWsEvent>>>,
    connect_attempts: AtomicUsize,
    subscribe_requests: Mutex<Vec<Vec<String>>>,
    auth_headers: Mutex<Vec<String>>,
}

impl ScriptedDaemonWsConnector {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn queue_connection(&self, events: Vec<DaemonWsEvent>) -> Result<()> {
        self.queued_connections.lock().await.push_back(events);
        Ok(())
    }

    pub fn connect_attempts(&self) -> usize {
        self.connect_attempts.load(Ordering::SeqCst)
    }

    pub fn subscribe_requests(&self) -> Vec<Vec<String>> {
        lock_recover(&self.subscribe_requests).clone()
    }

    pub fn auth_headers(&self) -> Vec<String> {
        lock_recover(&self.auth_headers).clone()
    }

    async fn next_connection(&self) -> Option<Vec<DaemonWsEvent>> {
        self.queued_connections.lock().await.pop_front()
    }

    async fn has_pending_connections(&self) -> bool {
        !self.queued_connections.lock().await.is_empty()
    }

    fn record_connect(&self, auth_header: String) {
        self.connect_attempts.fetch_add(1, Ordering::SeqCst);
        lock_recover(&self.auth_headers).push(auth_header);
    }

    fn record_subscribe(&self, topics: Vec<String>) {
        lock_recover(&self.subscribe_requests).push(topics);
    }
}

pub struct DaemonWsBridge {
    connection_state: DaemonConnectionState,
    scripted_connector: Option<Arc<ScriptedDaemonWsConnector>>,
    config: DaemonWsBridgeConfig,
    state: Arc<RwLock<BridgeState>>,
    subscribers: Arc<TokioMutex<Vec<Arc<Subscriber>>>>,
}

impl DaemonWsBridge {
    pub fn new(connection_state: DaemonConnectionState, config: DaemonWsBridgeConfig) -> Self {
        Self {
            connection_state,
            scripted_connector: None,
            config,
            state: Arc::new(RwLock::new(BridgeState::Disconnected)),
            subscribers: Arc::new(TokioMutex::new(Vec::new())),
        }
    }

    pub fn new_for_test(
        connection_state: DaemonConnectionState,
        connector: Arc<ScriptedDaemonWsConnector>,
        config: DaemonWsBridgeConfig,
    ) -> Self {
        Self {
            connection_state,
            scripted_connector: Some(connector),
            config,
            state: Arc::new(RwLock::new(BridgeState::Disconnected)),
            subscribers: Arc::new(TokioMutex::new(Vec::new())),
        }
    }

    pub async fn run_until_idle(&self) -> Result<()> {
        let connector = self
            .scripted_connector
            .clone()
            .context("run_until_idle is only available for scripted connectors")?;
        let connection = self
            .connection_state
            .get()
            .context("daemon connection not available for scripted bridge")?;
        let topics = self.active_topic_names().await;

        while let Some(events) = connector.next_connection().await {
            self.set_state(BridgeState::Connecting);
            connector.record_connect(format!("Bearer {}", connection.token));
            self.set_state(BridgeState::Subscribing);
            connector.record_subscribe(topics.clone());
            self.set_state(BridgeState::Ready);

            for event in events {
                if let Some(realtime_event) = map_daemon_ws_event(event) {
                    self.dispatch_event(realtime_event).await;
                }
            }

            if connector.has_pending_connections().await {
                self.set_state(BridgeState::Degraded);
            }
        }

        self.set_state(BridgeState::Ready);
        Ok(())
    }

    pub async fn run(self: Arc<Self>, token: CancellationToken) -> Result<()> {
        let mut backoff = self.config.backoff_initial;

        loop {
            if token.is_cancelled() {
                self.set_state(BridgeState::Disconnected);
                return Ok(());
            }

            let topics = self.active_topic_names().await;
            if topics.is_empty() {
                tokio::select! {
                    _ = token.cancelled() => {
                        self.set_state(BridgeState::Disconnected);
                        return Ok(());
                    }
                    _ = sleep(Duration::from_millis(100)) => {}
                }
                continue;
            }

            let connection = match self.connection_state.get() {
                Some(connection) => connection,
                None => {
                    self.set_state(BridgeState::Degraded);
                    tokio::select! {
                        _ = token.cancelled() => {
                            self.set_state(BridgeState::Disconnected);
                            return Ok(());
                        }
                        _ = sleep(backoff_with_jitter(backoff)) => {}
                    }
                    backoff = next_backoff(backoff, self.config.backoff_max);
                    continue;
                }
            };

            self.set_state(BridgeState::Connecting);
            match self.connect_and_process(&connection, &topics, &token).await {
                Ok(()) => {
                    backoff = self.config.backoff_initial;
                }
                Err(err) => {
                    warn!(error = %err, "daemon websocket bridge cycle failed");
                    self.set_state(BridgeState::Degraded);
                    tokio::select! {
                        _ = token.cancelled() => {
                            self.set_state(BridgeState::Disconnected);
                            return Ok(());
                        }
                        _ = sleep(backoff_with_jitter(backoff)) => {}
                    }
                    backoff = next_backoff(backoff, self.config.backoff_max);
                }
            }
        }
    }

    pub fn state(&self) -> BridgeState {
        match self.state.read() {
            Ok(guard) => *guard,
            Err(poisoned) => *poisoned.into_inner(),
        }
    }

    pub async fn subscribe(
        &self,
        consumer: &'static str,
        topics: &[RealtimeTopic],
    ) -> Result<mpsc::Receiver<RealtimeEvent>> {
        self.subscribe_internal(consumer, topics).await
    }

    async fn connect_and_process(
        &self,
        connection: &DaemonConnectionInfo,
        topics: &[String],
        token: &CancellationToken,
    ) -> Result<()> {
        let mut request = connection
            .ws_url
            .as_str()
            .into_client_request()
            .context("failed to build daemon websocket client request")?;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", connection.token).parse()?,
        );

        let ws_url = url::Url::parse(&connection.ws_url)
            .with_context(|| format!("invalid daemon websocket url {}", connection.ws_url))?;
        let host = ws_url
            .host_str()
            .context("daemon websocket url missing host")?;
        let port = ws_url
            .port_or_known_default()
            .context("daemon websocket url missing port")?;
        let tcp_stream = TcpStream::connect((host, port))
            .await
            .with_context(|| format!("failed to open daemon tcp socket {host}:{port}"))?;

        let (stream, _) = client_async(request, tcp_stream).await.with_context(|| {
            format!(
                "failed to connect daemon websocket at {}",
                connection.ws_url
            )
        })?;
        let (mut write, mut read) = stream.split();

        self.set_state(BridgeState::Subscribing);
        write
            .send(Message::Text(
                serde_json::json!({
                    "action": "subscribe",
                    "topics": topics,
                })
                .to_string()
                .into(),
            ))
            .await
            .context("failed to subscribe daemon websocket topics")?;
        self.set_state(BridgeState::Ready);
        info!(topics = ?topics, "daemon realtime bridge subscribed");

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    self.set_state(BridgeState::Disconnected);
                    return Ok(());
                }
                message = read.next() => {
                    match message {
                        Some(Ok(Message::Text(text))) => {
                            match serde_json::from_str::<DaemonWsEvent>(&text) {
                                Ok(event) => {
                                    if let Some(realtime_event) = map_daemon_ws_event(event) {
                                        self.dispatch_event(realtime_event).await;
                                    }
                                }
                                Err(err) => {
                                    warn!(error = %err, "failed to parse daemon websocket event");
                                }
                            }
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            self.set_state(BridgeState::Degraded);
                            return Ok(());
                        }
                        Some(Ok(_)) => {}
                        Some(Err(err)) => {
                            self.set_state(BridgeState::Degraded);
                            return Err(err.into());
                        }
                    }
                }
            }
        }
    }

    async fn active_topic_names(&self) -> Vec<String> {
        let subscribers = self.subscribers.lock().await;
        let mut topics = HashSet::new();
        for subscriber in subscribers.iter() {
            for topic in subscriber.topics.iter() {
                topics.insert(topic_name(topic).to_string());
            }
        }
        let mut topics: Vec<String> = topics.into_iter().collect();
        topics.sort();
        topics
    }

    async fn dispatch_event(&self, event: RealtimeEvent) {
        let subscribers = self.subscribers.lock().await.clone();
        let mut active = Vec::with_capacity(subscribers.len());

        for subscriber in subscribers {
            if subscriber.accepts(&event) {
                subscriber
                    .enqueue(event.clone(), self.config.terminal_retry_delay)
                    .await;
            }
            if !subscriber.is_closed() {
                active.push(subscriber);
            }
        }

        *self.subscribers.lock().await = active;
    }

    fn set_state(&self, state: BridgeState) {
        match self.state.write() {
            Ok(mut guard) => *guard = state,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                *guard = state;
            }
        }
    }

    async fn subscribe_internal(
        &self,
        consumer: &'static str,
        topics: &[RealtimeTopic],
    ) -> Result<mpsc::Receiver<RealtimeEvent>> {
        let (tx, rx) = mpsc::channel(self.config.queue_capacity);
        let subscriber = Arc::new(Subscriber::new(
            consumer,
            topics.iter().copied().collect(),
            tx,
            self.config.queue_capacity,
        ));
        self.subscribers.lock().await.push(subscriber.clone());
        subscriber.spawn_forwarder();
        Ok(rx)
    }
}

#[async_trait]
impl RealtimeTopicPort for DaemonWsBridge {
    async fn subscribe(
        &self,
        consumer: &'static str,
        topics: &[RealtimeTopic],
    ) -> Result<mpsc::Receiver<RealtimeEvent>> {
        self.subscribe_internal(consumer, topics).await
    }
}

struct Subscriber {
    consumer: &'static str,
    topics: HashSet<RealtimeTopic>,
    outbound: mpsc::Sender<RealtimeEvent>,
    pending: TokioMutex<VecDeque<RealtimeEvent>>,
    capacity: usize,
    notify: Notify,
    closed: AtomicBool,
}

impl Subscriber {
    fn new(
        consumer: &'static str,
        topics: HashSet<RealtimeTopic>,
        outbound: mpsc::Sender<RealtimeEvent>,
        capacity: usize,
    ) -> Self {
        Self {
            consumer,
            topics,
            outbound,
            pending: TokioMutex::new(VecDeque::new()),
            capacity,
            notify: Notify::new(),
            closed: AtomicBool::new(false),
        }
    }

    fn spawn_forwarder(self: Arc<Self>) {
        tokio::spawn(async move {
            loop {
                let next_event = loop {
                    if self.closed.load(Ordering::SeqCst) {
                        return;
                    }

                    if let Some(event) = self.pending.lock().await.pop_front() {
                        break event;
                    }

                    self.notify.notified().await;
                };

                if self.outbound.send(next_event).await.is_err() {
                    self.closed.store(true, Ordering::SeqCst);
                    return;
                }
            }
        });
    }

    fn accepts(&self, event: &RealtimeEvent) -> bool {
        self.topics.contains(&event_topic(event))
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    async fn enqueue(&self, event: RealtimeEvent, retry_delay: Duration) {
        if self.try_push(event.clone()).await {
            return;
        }

        if !is_terminal_event(&event) {
            warn!(
                consumer = self.consumer,
                "dropping realtime event under backpressure"
            );
            return;
        }

        sleep(retry_delay).await;
        let mut pending = self.pending.lock().await;
        if pending.len() >= self.capacity {
            if let Some(index) = pending.iter().position(|queued| !is_terminal_event(queued)) {
                pending.remove(index);
            } else {
                pending.pop_front();
            }
        }
        if pending.len() < self.capacity {
            pending.push_back(event);
            self.notify.notify_one();
        } else {
            warn!(
                consumer = self.consumer,
                "terminal realtime event still dropped after retry"
            );
        }
    }

    async fn try_push(&self, event: RealtimeEvent) -> bool {
        let mut pending = self.pending.lock().await;
        if pending.len() >= self.capacity {
            return false;
        }
        pending.push_back(event);
        self.notify.notify_one();
        true
    }
}

pub struct DaemonWsBridgeError {
    pub message: String,
}

fn map_daemon_ws_event(event: DaemonWsEvent) -> Option<RealtimeEvent> {
    let event_type = event.event_type.clone();
    let topic = event.topic.clone();
    let session_id = event.session_id.clone();

    match event.event_type.as_str() {
        "pairing.updated" => {
            match serde_json::from_value::<PairingSessionChangedPayload>(event.payload) {
                Ok(payload) => Some(RealtimeEvent::PairingUpdated(PairingUpdatedEvent {
                    session_id: payload.session_id,
                    status: payload.stage,
                    peer_id: payload.peer_id,
                    device_name: payload.device_name,
                })),
                Err(err) => {
                    warn!(
                        error = %err,
                        event_type = %event_type,
                        topic = %topic,
                        session_id = ?session_id,
                        "failed to decode daemon pairing.updated websocket payload"
                    );
                    None
                }
            }
        }
        "pairing.verification_required" => {
            match serde_json::from_value::<PairingVerificationPayload>(event.payload) {
                Ok(payload) => {
                    let kind = payload.kind.clone();
                    let has_peer_id = payload.peer_id.is_some();
                    let has_code = payload.code.is_some();
                    let has_local_fingerprint = payload.local_fingerprint.is_some();
                    let has_peer_fingerprint = payload.peer_fingerprint.is_some();
                    info!(
                        event_type = %event_type,
                        topic = %topic,
                        session_id = %payload.session_id,
                        kind = %kind,
                        has_peer_id,
                        has_code,
                        has_local_fingerprint,
                        has_peer_fingerprint,
                        "decoded daemon pairing verification websocket payload"
                    );
                    match payload.kind.as_str() {
                        "verification" => Some(RealtimeEvent::PairingVerificationRequired(
                            PairingVerificationRequiredEvent {
                                session_id: payload.session_id,
                                peer_id: payload.peer_id,
                                device_name: payload.device_name,
                                code: payload.code,
                                local_fingerprint: payload.local_fingerprint,
                                peer_fingerprint: payload.peer_fingerprint,
                            },
                        )),
                        "verifying" | "request" => {
                            Some(RealtimeEvent::PairingUpdated(PairingUpdatedEvent {
                                session_id: payload.session_id,
                                status: payload.kind,
                                peer_id: payload.peer_id,
                                device_name: payload.device_name,
                            }))
                        }
                        "complete" => Some(RealtimeEvent::PairingComplete(PairingCompleteEvent {
                            session_id: payload.session_id,
                            peer_id: payload.peer_id,
                            device_name: payload.device_name,
                        })),
                        "failed" => Some(RealtimeEvent::PairingFailed(PairingFailedEvent {
                            session_id: payload.session_id,
                            reason: payload
                                .error
                                .unwrap_or_else(|| "pairing failed".to_string()),
                        })),
                        _ => {
                            warn!(
                                event_type = %event_type,
                                topic = %topic,
                                session_id = ?session_id,
                                kind = %kind,
                                "ignored daemon pairing verification event with unsupported kind"
                            );
                            None
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        error = %err,
                        event_type = %event_type,
                        topic = %topic,
                        session_id = ?session_id,
                        "failed to decode daemon pairing.verification_required websocket payload"
                    );
                    None
                }
            }
        }
        "pairing.complete" => {
            match serde_json::from_value::<PairingSessionChangedPayload>(event.payload) {
                Ok(payload) => Some(RealtimeEvent::PairingComplete(PairingCompleteEvent {
                    session_id: payload.session_id,
                    peer_id: payload.peer_id,
                    device_name: payload.device_name,
                })),
                Err(err) => {
                    warn!(
                        error = %err,
                        event_type = %event_type,
                        topic = %topic,
                        session_id = ?session_id,
                        "failed to decode daemon pairing.complete websocket payload"
                    );
                    None
                }
            }
        }
        "pairing.failed" => serde_json::from_value::<PairingFailurePayload>(event.payload)
            .ok()
            .map(|payload| {
                RealtimeEvent::PairingFailed(PairingFailedEvent {
                    session_id: payload.session_id,
                    reason: payload.reason,
                })
            }),
        "peers.changed" => match serde_json::from_value::<PeersChangedFullPayload>(event.payload) {
            Ok(payload) => Some(RealtimeEvent::PeersChanged(PeerChangedEvent {
                peers: payload
                    .peers
                    .into_iter()
                    .map(|p| RealtimePeerSummary {
                        peer_id: p.peer_id,
                        device_name: p.device_name,
                        connected: p.connected,
                    })
                    .collect(),
            })),
            Err(e) => {
                warn!(error = %e, "Failed to deserialize peers.changed payload");
                None
            }
        },
        "peers.name_updated" => serde_json::from_value::<PeerNameUpdatedPayload>(event.payload)
            .ok()
            .map(|payload| {
                RealtimeEvent::PeersNameUpdated(PeerNameUpdatedEvent {
                    peer_id: payload.peer_id,
                    device_name: payload.device_name,
                })
            }),
        "peers.connection_changed" => {
            serde_json::from_value::<PeerConnectionChangedPayload>(event.payload)
                .ok()
                .map(|payload| {
                    RealtimeEvent::PeersConnectionChanged(PeerConnectionChangedEvent {
                        peer_id: payload.peer_id,
                        connected: payload.connected,
                        device_name: payload.device_name,
                    })
                })
        }
        "paired-devices.changed" => {
            serde_json::from_value::<PairedDevicesChangedPayload>(event.payload)
                .ok()
                .map(|payload| {
                    RealtimeEvent::PairedDevicesChanged(PairedDevicesChangedEvent {
                        devices: vec![uc_core::ports::realtime::RealtimePairedDeviceSummary {
                            device_id: payload.peer_id,
                            device_name: payload.device_name.unwrap_or_default(),
                            last_seen_ts: None,
                        }],
                    })
                })
        }
        "setup.state_changed" => {
            match serde_json::from_value::<SetupStateChangedPayload>(event.payload) {
                Ok(payload) => match serde_json::from_value(payload.state.clone()) {
                    Ok(state) => Some(RealtimeEvent::SetupStateChanged(SetupStateChangedEvent {
                        session_id: payload.session_id,
                        state,
                    })),
                    Err(err) => {
                        warn!(
                            error = %err,
                            raw_state = %payload.state,
                            "failed to decode setup state from daemon websocket event, dropping event"
                        );
                        None
                    }
                },
                Err(err) => {
                    warn!(
                        error = %err,
                        event_type = %event_type,
                        "failed to decode setup.state_changed websocket payload"
                    );
                    None
                }
            }
        }
        "setup.space_access_completed" => {
            serde_json::from_value::<SetupSpaceAccessCompletedPayload>(event.payload)
                .ok()
                .map(|payload| {
                    RealtimeEvent::SetupSpaceAccessCompleted(SetupSpaceAccessCompletedEvent {
                        session_id: payload.session_id,
                        peer_id: payload.peer_id,
                        success: payload.success,
                        reason: payload.reason,
                        ts: payload.ts,
                    })
                })
        }
        "space_access.state_changed" => {
            serde_json::from_value::<SpaceAccessStateChangedPayload>(event.payload)
                .ok()
                .map(|payload| {
                    RealtimeEvent::SpaceAccessStateChanged(SpaceAccessStateChangedEvent {
                        state: payload.state,
                    })
                })
        }
        "space_access.snapshot" => {
            serde_json::from_value::<SpaceAccessStateChangedPayload>(event.payload)
                .ok()
                .map(|payload| {
                    RealtimeEvent::SpaceAccessStateChanged(SpaceAccessStateChangedEvent {
                        state: payload.state,
                    })
                })
        }
        _ => None,
    }
}

fn event_topic(event: &RealtimeEvent) -> RealtimeTopic {
    match event {
        RealtimeEvent::PairingUpdated(_)
        | RealtimeEvent::PairingVerificationRequired(_)
        | RealtimeEvent::PairingFailed(_)
        | RealtimeEvent::PairingComplete(_) => RealtimeTopic::Pairing,
        RealtimeEvent::PeersChanged(_)
        | RealtimeEvent::PeersNameUpdated(_)
        | RealtimeEvent::PeersConnectionChanged(_) => RealtimeTopic::Peers,
        RealtimeEvent::PairedDevicesChanged(_) => RealtimeTopic::PairedDevices,
        RealtimeEvent::SetupStateChanged(_) | RealtimeEvent::SetupSpaceAccessCompleted(_) => {
            RealtimeTopic::Setup
        }
        RealtimeEvent::SpaceAccessStateChanged(_) => RealtimeTopic::SpaceAccess,
    }
}

fn topic_name(topic: &RealtimeTopic) -> &'static str {
    match topic {
        RealtimeTopic::Pairing => "pairing",
        RealtimeTopic::Peers => "peers",
        RealtimeTopic::PairedDevices => "paired-devices",
        RealtimeTopic::Setup => "setup",
        RealtimeTopic::SpaceAccess => "space-access",
    }
}

fn is_terminal_event(event: &RealtimeEvent) -> bool {
    matches!(event, RealtimeEvent::PairingFailed(_))
}

fn next_backoff(current: Duration, max: Duration) -> Duration {
    current.saturating_mul(2).min(max)
}

fn backoff_with_jitter(base: Duration) -> Duration {
    let millis = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_millis() as u64,
        Err(_) => 0,
    };
    let spread = base.as_millis().max(1) as u64;
    base.saturating_add(Duration::from_millis((millis % spread) / 2))
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_daemon::api::types::{PeerSnapshotDto, PeersChangedFullPayload};

    fn make_full_payload_event(peers: Vec<PeerSnapshotDto>) -> DaemonWsEvent {
        DaemonWsEvent {
            topic: "peers".to_string(),
            event_type: "peers.changed".to_string(),
            session_id: None,
            ts: 0,
            payload: serde_json::to_value(PeersChangedFullPayload { peers }).unwrap(),
        }
    }

    #[test]
    fn peers_changed_full_payload_translates_all_peers() {
        let event = make_full_payload_event(vec![
            PeerSnapshotDto {
                peer_id: "peer-1".to_string(),
                device_name: Some("Laptop".to_string()),
                addresses: vec![],
                is_paired: false,
                connected: true,
                pairing_state: "NotPaired".to_string(),
            },
            PeerSnapshotDto {
                peer_id: "peer-2".to_string(),
                device_name: None,
                addresses: vec![],
                is_paired: true,
                connected: false,
                pairing_state: "Trusted".to_string(),
            },
            PeerSnapshotDto {
                peer_id: "peer-3".to_string(),
                device_name: Some("Tablet".to_string()),
                addresses: vec![],
                is_paired: false,
                connected: false,
                pairing_state: "NotPaired".to_string(),
            },
        ]);

        let result = map_daemon_ws_event(event);

        let peers_event = match result {
            Some(RealtimeEvent::PeersChanged(e)) => e,
            other => panic!("expected PeersChanged, got {:?}", other),
        };

        assert_eq!(peers_event.peers.len(), 3, "all 3 peers must be preserved");

        let peer1 = peers_event
            .peers
            .iter()
            .find(|p| p.peer_id == "peer-1")
            .unwrap();
        assert_eq!(peer1.device_name, Some("Laptop".to_string()));
        assert!(peer1.connected);

        let peer2 = peers_event
            .peers
            .iter()
            .find(|p| p.peer_id == "peer-2")
            .unwrap();
        assert_eq!(peer2.device_name, None);
        assert!(!peer2.connected);

        let peer3 = peers_event
            .peers
            .iter()
            .find(|p| p.peer_id == "peer-3")
            .unwrap();
        assert_eq!(peer3.device_name, Some("Tablet".to_string()));
    }

    #[test]
    fn peers_changed_full_payload_empty_list_translates_to_empty_peers() {
        let event = make_full_payload_event(vec![]);
        let result = map_daemon_ws_event(event);

        let peers_event = match result {
            Some(RealtimeEvent::PeersChanged(e)) => e,
            other => panic!("expected PeersChanged, got {:?}", other),
        };

        assert_eq!(
            peers_event.peers.len(),
            0,
            "empty peer list must translate to empty peers"
        );
    }
}
