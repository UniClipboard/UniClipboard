use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;

use crate::realtime::{
    ClipboardIncomingPendingEvent, ClipboardNewContentEvent, FileTransferProgressEvent,
    FileTransferStatusChangedEvent, PairingCompleteEvent, PairingFailedEvent, PairingUpdatedEvent,
    PairingVerificationRequiredEvent, PeerChangedEvent, PeerConnectionChangedEvent,
    PeerNameUpdatedEvent, RealtimeEvent, RealtimePeerSummary, RealtimeTopic, RealtimeTopicPort,
    SpaceMembersChangedEvent,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex as TokioMutex, Notify};
use tokio::time::sleep;
use tokio_tungstenite::client_async;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, info_span, instrument, warn, Instrument};
use uc_daemon_contract::api::auth::DaemonConnectionInfo;
use uc_daemon_contract::api::types::{
    DaemonWsEvent, FileTransferProgressPayload, PairingFailurePayload,
    PairingSessionChangedPayload, PairingVerificationPayload, PeerConnectionChangedPayload,
    PeerNameUpdatedPayload, PeersChangedFullPayload, SpaceMembersChangedPayload,
};
use uc_daemon_contract::constants::{pairing_stage, ws_event, ws_topic};

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
        let _connection = self
            .connection_state
            .get()
            .context("daemon connection not available for scripted bridge")?;
        let topics = self.active_topic_names().await;

        while let Some(events) = connector.next_connection().await {
            self.set_state(BridgeState::Connecting);
            connector.record_connect(format!("Session test-session-token"));
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

    #[instrument(
        name = "daemon_ws_bridge.run",
        level = "info",
        skip(self, token),
        fields(component = "daemon_ws_bridge")
    )]
    pub async fn run(self: Arc<Self>, token: CancellationToken) -> Result<()> {
        let mut backoff = self.config.backoff_initial;

        loop {
            if token.is_cancelled() {
                self.set_state(BridgeState::Disconnected);
                return Ok(());
            }

            let topics = self.active_topic_names().await;
            if topics.is_empty() {
                debug!(
                    event = "bridge.idle_no_topics",
                    "bridge has no active subscribers; waiting before next connection cycle"
                );
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
                    warn!(
                        event = "bridge.connection_unavailable",
                        topics_count = topics.len(),
                        backoff_ms = backoff.as_millis() as u64,
                        "daemon connection unavailable for websocket bridge"
                    );
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
                    info!(
                        event = "bridge.connection_cycle_completed",
                        topics_count = topics.len(),
                        "daemon websocket bridge connection cycle completed"
                    );
                    backoff = self.config.backoff_initial;
                }
                Err(err) => {
                    warn!(
                        event = "bridge.connection_cycle_failed",
                        error = %err,
                        backoff_ms = backoff.as_millis() as u64,
                        topics_count = topics.len(),
                        "daemon websocket bridge cycle failed"
                    );
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
        let span = info_span!(
            "daemon_ws_bridge.connect_and_process",
            daemon_pid = connection.pid,
            ws_url = %connection.ws_url,
            topics_count = topics.len(),
        );

        async move {
            info!(
                event = "bridge.connect_started",
                topics = ?topics,
                "starting daemon websocket bridge connection"
            );

            // Exchange bearer → session JWT lazily on first WS connect attempt.
            // Uses the same session token cache as HTTP requests.
            let http = reqwest::Client::new();
            let session_token =
                crate::http::get_session_token(&http, &self.connection_state, connection.pid)
                    .await?;
            debug!(
                event = "bridge.session_token_acquired",
                daemon_pid = connection.pid,
                "acquired daemon websocket session token"
            );

            let mut request = connection
                .ws_url
                .as_str()
                .into_client_request()
                .context("failed to build daemon websocket client request")?;
            request.headers_mut().insert(
                "Authorization",
                format!("Session {}", session_token).parse()?,
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
            debug!(
                event = "bridge.tcp_connected",
                host = %host,
                port,
                "connected daemon websocket tcp socket"
            );

            let (stream, _) = client_async(request, tcp_stream).await.with_context(|| {
                format!(
                    "failed to connect daemon websocket at {}",
                    connection.ws_url
                )
            })?;
            info!(
                event = "bridge.ws_connected",
                ws_url = %connection.ws_url,
                "daemon websocket handshake completed"
            );
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
            info!(
                event = "bridge.subscribed",
                topics = ?topics,
                topics_count = topics.len(),
                "daemon realtime bridge subscribed"
            );

            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        info!(
                            event = "bridge.cancelled",
                            "daemon websocket bridge cancelled"
                        );
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
                                        warn!(
                                            event = "bridge.event_parse_failed",
                                            error = %err,
                                            "failed to parse daemon websocket event"
                                        );
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) => {
                                info!(
                                    event = "bridge.ws_closed",
                                    close_kind = "close_frame",
                                    "daemon websocket closed by peer"
                                );
                                self.set_state(BridgeState::Degraded);
                                return Ok(());
                            }
                            None => {
                                info!(
                                    event = "bridge.ws_closed",
                                    close_kind = "stream_end",
                                    "daemon websocket stream ended"
                                );
                                self.set_state(BridgeState::Degraded);
                                return Ok(());
                            }
                            Some(Ok(_)) => {}
                            Some(Err(err)) => {
                                self.set_state(BridgeState::Degraded);
                                warn!(
                                    event = "bridge.ws_read_failed",
                                    error = %err,
                                    "daemon websocket read failed"
                                );
                                return Err(err.into());
                            }
                        }
                    }
                }
            }
        }
        .instrument(span)
        .await
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
        let event_topic = event_topic(&event);
        let subscribers = self.subscribers.lock().await.clone();
        let subscriber_count = subscribers.len();
        let mut active = Vec::with_capacity(subscribers.len());
        let mut delivered_count = 0usize;

        for subscriber in subscribers {
            if subscriber.accepts(&event) {
                subscriber
                    .enqueue(event.clone(), self.config.terminal_retry_delay)
                    .await;
                delivered_count += 1;
            }
            if !subscriber.is_closed() {
                active.push(subscriber);
            }
        }

        debug!(
            event = "bridge.dispatch_completed",
            event_topic = topic_name(&event_topic),
            subscriber_count,
            delivered_count,
            active_count = active.len(),
            "daemon websocket bridge dispatched realtime event"
        );

        *self.subscribers.lock().await = active;
    }

    fn set_state(&self, next: BridgeState) {
        let prev = match self.state.write() {
            Ok(mut guard) => {
                let prev = *guard;
                *guard = next;
                prev
            }
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                let prev = *guard;
                *guard = next;
                prev
            }
        };

        if prev != next {
            info!(
                event = "bridge.state_changed",
                from_state = ?prev,
                to_state = ?next,
                "daemon websocket bridge state changed"
            );
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
        let topics_for_log: Vec<&'static str> = topics.iter().map(topic_name).collect();
        let mut subscribers = self.subscribers.lock().await;
        subscribers.push(subscriber.clone());
        info!(
            event = "bridge.subscriber_added",
            consumer,
            topics = ?topics_for_log,
            queue_capacity = self.config.queue_capacity,
            subscriber_count = subscribers.len(),
            "registered daemon websocket bridge subscriber"
        );
        drop(subscribers);
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
                        debug!(
                            event = "bridge.forwarder_stopped",
                            consumer = self.consumer,
                            reason = "subscriber_closed",
                            "daemon websocket subscriber forwarder stopped"
                        );
                        return;
                    }

                    if let Some(event) = self.pending.lock().await.pop_front() {
                        break event;
                    }

                    self.notify.notified().await;
                };

                if self.outbound.send(next_event).await.is_err() {
                    self.closed.store(true, Ordering::SeqCst);
                    info!(
                        event = "bridge.forwarder_stopped",
                        consumer = self.consumer,
                        reason = "receiver_dropped",
                        "daemon websocket subscriber forwarder stopped"
                    );
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

        let event_topic = event_topic(&event);

        if !is_terminal_event(&event) {
            let queue_len = self.pending.lock().await.len();
            warn!(
                event = "bridge.event_dropped",
                consumer = self.consumer,
                event_topic = topic_name(&event_topic),
                queue_len,
                capacity = self.capacity,
                is_terminal = false,
                reason = "backpressure",
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
            error!(
                event = "bridge.terminal_event_dropped",
                consumer = self.consumer,
                event_topic = topic_name(&event_topic),
                queue_len = pending.len(),
                capacity = self.capacity,
                retry_delay_ms = retry_delay.as_millis() as u64,
                is_terminal = true,
                reason = "backpressure_after_retry",
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

/// Log a successful bridge routing decision so the mapping is observable without reading code.
///
/// Fields are safe to log: session identity and event class names only — no secrets or payloads.
fn log_bridge_routing(
    source_topic: &str,
    source_event_type: &str,
    session_id: Option<&str>,
    payload_kind: Option<&str>,
    routed_event_class: &'static str,
) {
    debug!(
        event = "bridge.route_succeeded",
        source_topic,
        source_event_type,
        session_id = session_id.unwrap_or(""),
        payload_kind = payload_kind.unwrap_or(""),
        routed_event_class,
        "daemon websocket bridge routed event"
    );
}

fn log_decode_failed(
    source_topic: &str,
    source_event_type: &str,
    session_id: Option<&str>,
    payload_type: &'static str,
    error: &serde_json::Error,
) {
    warn!(
        event = "bridge.decode_failed",
        source_topic,
        source_event_type,
        session_id = session_id.unwrap_or(""),
        payload_type,
        error = %error,
        "failed to decode websocket payload"
    );
}

fn map_daemon_ws_event(event: DaemonWsEvent) -> Option<RealtimeEvent> {
    let event_type = event.event_type.clone();
    let topic = event.topic.clone();
    let session_id = event.session_id.clone();

    debug!(
        event = "bridge.ws_event_received",
        source_topic = %topic,
        source_event_type = %event_type,
        session_id = session_id.as_deref().unwrap_or(""),
        "received daemon websocket event"
    );

    match event.event_type.as_str() {
        ws_event::PAIRING_UPDATED => {
            match serde_json::from_value::<PairingSessionChangedPayload>(event.payload) {
                Ok(payload) => {
                    debug!(
                        event = "bridge.payload_decoded",
                        source_topic = %topic,
                        source_event_type = %event_type,
                        session_id = %payload.session_id,
                        payload_type = "PairingSessionChangedPayload",
                        stage = %payload.stage,
                        has_peer_id = payload.peer_id.is_some(),
                        has_device_name = payload.device_name.is_some(),
                        "decoded websocket payload"
                    );
                    log_bridge_routing(
                        &topic,
                        &event_type,
                        Some(&payload.session_id),
                        Some(&payload.stage),
                        "PairingUpdated",
                    );
                    Some(RealtimeEvent::PairingUpdated(PairingUpdatedEvent {
                        session_id: payload.session_id,
                        status: payload.stage,
                        peer_id: payload.peer_id,
                        device_name: payload.device_name,
                    }))
                }
                Err(err) => {
                    log_decode_failed(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        "PairingSessionChangedPayload",
                        &err,
                    );
                    None
                }
            }
        }
        ws_event::PAIRING_VERIFICATION_REQUIRED => {
            match serde_json::from_value::<PairingVerificationPayload>(event.payload) {
                Ok(payload) => {
                    let kind = payload.kind.clone();
                    let has_peer_id = payload.peer_id.is_some();
                    let has_code = payload.code.is_some();
                    let has_local_fingerprint = payload.local_fingerprint.is_some();
                    let has_peer_fingerprint = payload.peer_fingerprint.is_some();
                    debug!(
                        event = "bridge.payload_decoded",
                        source_topic = %topic,
                        source_event_type = %event_type,
                        session_id = %payload.session_id,
                        payload_type = "PairingVerificationPayload",
                        kind = %kind,
                        has_peer_id,
                        has_code,
                        has_local_fingerprint,
                        has_peer_fingerprint,
                        "decoded websocket payload"
                    );
                    match payload.kind.as_str() {
                        pairing_stage::VERIFICATION => {
                            log_bridge_routing(
                                &topic,
                                &event_type,
                                Some(&payload.session_id),
                                Some(&kind),
                                "PairingVerificationRequired",
                            );
                            Some(RealtimeEvent::PairingVerificationRequired(
                                PairingVerificationRequiredEvent {
                                    session_id: payload.session_id,
                                    peer_id: payload.peer_id,
                                    device_name: payload.device_name,
                                    code: payload.code,
                                    local_fingerprint: payload.local_fingerprint,
                                    peer_fingerprint: payload.peer_fingerprint,
                                },
                            ))
                        }
                        pairing_stage::VERIFYING | pairing_stage::REQUEST => {
                            log_bridge_routing(
                                &topic,
                                &event_type,
                                Some(&payload.session_id),
                                Some(&kind),
                                "PairingUpdated",
                            );
                            Some(RealtimeEvent::PairingUpdated(PairingUpdatedEvent {
                                session_id: payload.session_id,
                                status: payload.kind,
                                peer_id: payload.peer_id,
                                device_name: payload.device_name,
                            }))
                        }
                        pairing_stage::COMPLETE => {
                            log_bridge_routing(
                                &topic,
                                &event_type,
                                Some(&payload.session_id),
                                Some(&kind),
                                "PairingComplete",
                            );
                            Some(RealtimeEvent::PairingComplete(PairingCompleteEvent {
                                session_id: payload.session_id,
                                peer_id: payload.peer_id,
                                device_name: payload.device_name,
                            }))
                        }
                        pairing_stage::FAILED => {
                            log_bridge_routing(
                                &topic,
                                &event_type,
                                Some(&payload.session_id),
                                Some(&kind),
                                "PairingFailed",
                            );
                            Some(RealtimeEvent::PairingFailed(PairingFailedEvent {
                                session_id: payload.session_id,
                                reason: payload
                                    .error
                                    .unwrap_or_else(|| "pairing failed".to_string()),
                            }))
                        }
                        _ => {
                            warn!(
                                event = "bridge.route_unsupported",
                                source_event_type = %event_type,
                                source_topic = %topic,
                                session_id = session_id.as_deref().unwrap_or(""),
                                kind = %kind,
                                dropped = true,
                                "websocket event kind has no registered route"
                            );
                            None
                        }
                    }
                }
                Err(err) => {
                    log_decode_failed(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        "PairingVerificationPayload",
                        &err,
                    );
                    None
                }
            }
        }
        ws_event::PAIRING_COMPLETE => {
            match serde_json::from_value::<PairingSessionChangedPayload>(event.payload) {
                Ok(payload) => {
                    debug!(
                        event = "bridge.payload_decoded",
                        source_topic = %topic,
                        source_event_type = %event_type,
                        session_id = %payload.session_id,
                        payload_type = "PairingSessionChangedPayload",
                        stage = %payload.stage,
                        has_peer_id = payload.peer_id.is_some(),
                        has_device_name = payload.device_name.is_some(),
                        "decoded websocket payload"
                    );
                    log_bridge_routing(
                        &topic,
                        &event_type,
                        Some(&payload.session_id),
                        Some(&payload.stage),
                        "PairingComplete",
                    );
                    Some(RealtimeEvent::PairingComplete(PairingCompleteEvent {
                        session_id: payload.session_id,
                        peer_id: payload.peer_id,
                        device_name: payload.device_name,
                    }))
                }
                Err(err) => {
                    log_decode_failed(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        "PairingSessionChangedPayload",
                        &err,
                    );
                    None
                }
            }
        }
        ws_event::PAIRING_FAILED => {
            match serde_json::from_value::<PairingFailurePayload>(event.payload) {
                Ok(payload) => {
                    debug!(
                        event = "bridge.payload_decoded",
                        source_topic = %topic,
                        source_event_type = %event_type,
                        session_id = %payload.session_id,
                        payload_type = "PairingFailurePayload",
                        "decoded websocket payload"
                    );
                    log_bridge_routing(
                        &topic,
                        &event_type,
                        Some(&payload.session_id),
                        None,
                        "PairingFailed",
                    );
                    Some(RealtimeEvent::PairingFailed(PairingFailedEvent {
                        session_id: payload.session_id,
                        reason: payload.reason,
                    }))
                }
                Err(err) => {
                    log_decode_failed(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        "PairingFailurePayload",
                        &err,
                    );
                    None
                }
            }
        }
        ws_event::PEERS_CHANGED => {
            match serde_json::from_value::<PeersChangedFullPayload>(event.payload) {
                Ok(payload) => {
                    debug!(
                        event = "bridge.payload_decoded",
                        source_topic = %topic,
                        source_event_type = %event_type,
                        session_id = session_id.as_deref().unwrap_or(""),
                        payload_type = "PeersChangedFullPayload",
                        peer_count = payload.peers.len(),
                        "decoded websocket payload"
                    );
                    log_bridge_routing(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        None,
                        "PeersChanged",
                    );
                    Some(RealtimeEvent::PeersChanged(PeerChangedEvent {
                        peers: payload
                            .peers
                            .into_iter()
                            .map(|p| RealtimePeerSummary {
                                peer_id: p.peer_id,
                                device_name: p.device_name,
                                connected: p.connected,
                            })
                            .collect(),
                    }))
                }
                Err(err) => {
                    log_decode_failed(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        "PeersChangedFullPayload",
                        &err,
                    );
                    None
                }
            }
        }
        ws_event::PEERS_NAME_UPDATED => {
            match serde_json::from_value::<PeerNameUpdatedPayload>(event.payload) {
                Ok(payload) => {
                    debug!(
                        event = "bridge.payload_decoded",
                        source_topic = %topic,
                        source_event_type = %event_type,
                        session_id = session_id.as_deref().unwrap_or(""),
                        payload_type = "PeerNameUpdatedPayload",
                        peer_id = %payload.peer_id,
                        has_device_name = !payload.device_name.is_empty(),
                        "decoded websocket payload"
                    );
                    log_bridge_routing(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        None,
                        "PeersNameUpdated",
                    );
                    Some(RealtimeEvent::PeersNameUpdated(PeerNameUpdatedEvent {
                        peer_id: payload.peer_id,
                        device_name: payload.device_name,
                    }))
                }
                Err(err) => {
                    log_decode_failed(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        "PeerNameUpdatedPayload",
                        &err,
                    );
                    None
                }
            }
        }
        ws_event::PEERS_CONNECTION_CHANGED => {
            match serde_json::from_value::<PeerConnectionChangedPayload>(event.payload) {
                Ok(payload) => {
                    debug!(
                        event = "bridge.payload_decoded",
                        source_topic = %topic,
                        source_event_type = %event_type,
                        session_id = session_id.as_deref().unwrap_or(""),
                        payload_type = "PeerConnectionChangedPayload",
                        peer_id = %payload.peer_id,
                        connected = payload.connected,
                        has_device_name = payload.device_name.is_some(),
                        "decoded websocket payload"
                    );
                    log_bridge_routing(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        None,
                        "PeersConnectionChanged",
                    );
                    Some(RealtimeEvent::PeersConnectionChanged(
                        PeerConnectionChangedEvent {
                            peer_id: payload.peer_id,
                            connected: payload.connected,
                            device_name: payload.device_name,
                        },
                    ))
                }
                Err(err) => {
                    log_decode_failed(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        "PeerConnectionChangedPayload",
                        &err,
                    );
                    None
                }
            }
        }
        ws_event::PAIRED_DEVICES_CHANGED => {
            match serde_json::from_value::<SpaceMembersChangedPayload>(event.payload) {
                Ok(payload) => {
                    debug!(
                        event = "bridge.payload_decoded",
                        source_topic = %topic,
                        source_event_type = %event_type,
                        session_id = session_id.as_deref().unwrap_or(""),
                        payload_type = "SpaceMembersChangedPayload",
                        peer_id = %payload.peer_id,
                        has_device_name = payload.device_name.is_some(),
                        "decoded websocket payload"
                    );
                    log_bridge_routing(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        None,
                        "SpaceMembersChanged",
                    );
                    Some(RealtimeEvent::SpaceMembersChanged(
                        SpaceMembersChangedEvent {
                            devices: vec![crate::realtime::RealtimeSpaceMemberSummary {
                                device_id: payload.peer_id,
                                device_name: payload.device_name.unwrap_or_default(),
                                last_seen_ts: None,
                            }],
                        },
                    ))
                }
                Err(err) => {
                    log_decode_failed(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        "SpaceMembersChangedPayload",
                        &err,
                    );
                    None
                }
            }
        }
        ws_event::CLIPBOARD_NEW_CONTENT => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct ClipboardPayload {
                entry_id: String,
                preview: String,
                origin: String,
            }
            match serde_json::from_value::<ClipboardPayload>(event.payload) {
                Ok(payload) => {
                    debug!(
                        event = "bridge.payload_decoded",
                        source_topic = %topic,
                        source_event_type = %event_type,
                        session_id = session_id.as_deref().unwrap_or(""),
                        payload_type = "ClipboardPayload",
                        entry_id = %payload.entry_id,
                        origin = %payload.origin,
                        preview_len = payload.preview.len(),
                        "decoded websocket payload"
                    );
                    log_bridge_routing(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        None,
                        "ClipboardNewContent",
                    );
                    Some(RealtimeEvent::ClipboardNewContent(
                        ClipboardNewContentEvent {
                            entry_id: payload.entry_id,
                            preview: payload.preview,
                            origin: payload.origin,
                        },
                    ))
                }
                Err(err) => {
                    log_decode_failed(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        "ClipboardPayload",
                        &err,
                    );
                    None
                }
            }
        }
        ws_event::CLIPBOARD_INCOMING_PENDING => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct IncomingPendingPayload {
                entry_id: String,
                from_device: String,
                #[serde(default)]
                total_bytes: Option<u64>,
                #[serde(default)]
                filenames: Vec<String>,
            }
            match serde_json::from_value::<IncomingPendingPayload>(event.payload) {
                Ok(payload) => {
                    debug!(
                        event = "bridge.payload_decoded",
                        source_topic = %topic,
                        source_event_type = %event_type,
                        session_id = session_id.as_deref().unwrap_or(""),
                        payload_type = "IncomingPendingPayload",
                        entry_id = %payload.entry_id,
                        filename_count = payload.filenames.len(),
                        "decoded websocket payload"
                    );
                    log_bridge_routing(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        None,
                        "ClipboardIncomingPending",
                    );
                    Some(RealtimeEvent::ClipboardIncomingPending(
                        ClipboardIncomingPendingEvent {
                            entry_id: payload.entry_id,
                            from_device: payload.from_device,
                            total_bytes: payload.total_bytes,
                            filenames: payload.filenames,
                        },
                    ))
                }
                Err(err) => {
                    log_decode_failed(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        "IncomingPendingPayload",
                        &err,
                    );
                    None
                }
            }
        }
        ws_event::FILE_TRANSFER_STATUS_CHANGED => {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct StatusChangedPayload {
                transfer_id: String,
                entry_id: String,
                status: String,
                #[serde(default)]
                reason: Option<String>,
            }
            match serde_json::from_value::<StatusChangedPayload>(event.payload) {
                Ok(payload) => {
                    debug!(
                        event = "bridge.payload_decoded",
                        source_topic = %topic,
                        source_event_type = %event_type,
                        session_id = session_id.as_deref().unwrap_or(""),
                        payload_type = "StatusChangedPayload",
                        transfer_id = %payload.transfer_id,
                        entry_id = %payload.entry_id,
                        status = %payload.status,
                        "decoded websocket payload"
                    );
                    log_bridge_routing(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        None,
                        "FileTransferStatusChanged",
                    );
                    Some(RealtimeEvent::FileTransferStatusChanged(
                        FileTransferStatusChangedEvent {
                            transfer_id: payload.transfer_id,
                            entry_id: payload.entry_id,
                            status: payload.status,
                            reason: payload.reason,
                        },
                    ))
                }
                Err(err) => {
                    log_decode_failed(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        "StatusChangedPayload",
                        &err,
                    );
                    None
                }
            }
        }
        ws_event::FILE_TRANSFER_PROGRESS => {
            match serde_json::from_value::<FileTransferProgressPayload>(event.payload) {
                Ok(payload) => {
                    debug!(
                        event = "bridge.payload_decoded",
                        source_topic = %topic,
                        source_event_type = %event_type,
                        session_id = session_id.as_deref().unwrap_or(""),
                        payload_type = "FileTransferProgressPayload",
                        transfer_id = %payload.transfer_id,
                        peer_id = %payload.peer_id,
                        bytes_transferred = payload.bytes_transferred,
                        "decoded websocket payload"
                    );
                    log_bridge_routing(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        None,
                        "FileTransferProgress",
                    );
                    Some(RealtimeEvent::FileTransferProgress(
                        FileTransferProgressEvent {
                            transfer_id: payload.transfer_id,
                            entry_id: payload.entry_id,
                            peer_id: payload.peer_id,
                            direction: payload.direction,
                            bytes_transferred: payload.bytes_transferred,
                            total_bytes: payload.total_bytes,
                        },
                    ))
                }
                Err(err) => {
                    log_decode_failed(
                        &topic,
                        &event_type,
                        session_id.as_deref(),
                        "FileTransferProgressPayload",
                        &err,
                    );
                    None
                }
            }
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
        RealtimeEvent::SpaceMembersChanged(_) => RealtimeTopic::PairedDevices,
        RealtimeEvent::ClipboardNewContent(_) | RealtimeEvent::ClipboardIncomingPending(_) => {
            RealtimeTopic::Clipboard
        }
        RealtimeEvent::FileTransferStatusChanged(_) | RealtimeEvent::FileTransferProgress(_) => {
            RealtimeTopic::FileTransfer
        }
    }
}

fn topic_name(topic: &RealtimeTopic) -> &'static str {
    match topic {
        RealtimeTopic::Pairing => ws_topic::PAIRING,
        RealtimeTopic::Peers => ws_topic::PEERS,
        RealtimeTopic::PairedDevices => ws_topic::PAIRED_DEVICES,
        RealtimeTopic::Setup => ws_topic::SETUP,
        RealtimeTopic::Clipboard => ws_topic::CLIPBOARD,
        RealtimeTopic::FileTransfer => ws_topic::FILE_TRANSFER,
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
