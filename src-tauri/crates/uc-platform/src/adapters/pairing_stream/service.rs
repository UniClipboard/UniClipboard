use super::framing::{read_length_prefixed, write_length_prefixed, MAX_PAIRING_FRAME_BYTES};
use crate::ports::observability::TraceMetadata;
use anyhow::{anyhow, Result};
use libp2p::{futures::StreamExt, PeerId, StreamProtocol};
use libp2p_stream as stream;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, watch, Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::{timeout, Duration};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{debug, info, info_span, warn, Instrument, Span};
use uc_core::network::{NetworkEvent, PairingMessage, ProtocolId};

pub const MAX_PAIRING_CONCURRENCY: usize = 16;
const PER_PEER_CONCURRENCY: usize = 2;

#[derive(Debug, Error)]
pub enum PairingStreamError {
    #[error("pairing stream protocol unsupported")]
    UnsupportedProtocol,
    #[error("pairing session already open: {session_id}")]
    SessionExists { session_id: String },
}

#[derive(Debug)]
enum ShutdownReason {
    ExplicitClose,
    StreamClosedByPeer,
    ChannelClosed,
}

impl std::fmt::Display for ShutdownReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShutdownReason::ExplicitClose => write!(f, "explicit_close"),
            ShutdownReason::StreamClosedByPeer => write!(f, "stream_closed_by_peer"),
            ShutdownReason::ChannelClosed => write!(f, "channel_closed"),
        }
    }
}

fn close_initiator_for_shutdown_reason(reason: &ShutdownReason, app_closed: bool) -> &'static str {
    if app_closed {
        "application"
    } else {
        match reason {
            ShutdownReason::ExplicitClose => "application",
            ShutdownReason::StreamClosedByPeer => "peer",
            ShutdownReason::ChannelClosed => "local",
        }
    }
}

fn close_initiator_for_error(error: &anyhow::Error, completion_source: &str) -> &'static str {
    let error = error.to_string().to_ascii_lowercase();
    if error.contains("pairing stream idle timeout") {
        "local"
    } else if error.contains("broken pipe")
        || error.contains("connection reset")
        || error.contains("unexpected eof")
        || error.contains("stream closed")
    {
        "peer"
    } else if error.contains("pairing encode failed")
        || error.contains("task join failed")
        || error.contains("send failed")
        || error.contains("channel closed")
    {
        "local"
    } else if completion_source == "write_loop" {
        "unknown"
    } else {
        "unknown"
    }
}

fn end_reason_for_error(error: &anyhow::Error, completion_source: &str) -> &'static str {
    let error = error.to_string().to_ascii_lowercase();
    if error.contains("pairing stream idle timeout") {
        "idle_timeout"
    } else if error.contains("broken pipe")
        || error.contains("connection reset")
        || error.contains("unexpected eof")
        || error.contains("stream closed")
    {
        "peer_transport_error"
    } else if error.contains("pairing frame exceeds max")
        || error.contains("invalid pairing message")
    {
        "invalid_frame"
    } else if error.contains("pairing encode failed") || error.contains("task join failed") {
        "internal_error"
    } else if completion_source == "write_loop" {
        "write_error"
    } else {
        "read_error"
    }
}

#[derive(Clone)]
pub struct PairingStreamConfig {
    pub idle_timeout: Duration,
    pub max_frame_bytes: usize,
    pub outbound_queue_depth: usize,
}

impl Default for PairingStreamConfig {
    fn default() -> Self {
        Self {
            idle_timeout: Duration::from_secs(30),
            max_frame_bytes: MAX_PAIRING_FRAME_BYTES,
            outbound_queue_depth: 16,
        }
    }
}

#[derive(Clone)]
pub struct PairingStreamService {
    inner: Arc<PairingStreamServiceInner>,
}

struct PairingStreamServiceInner {
    control: AsyncMutex<stream::Control>,
    event_tx: mpsc::Sender<NetworkEvent>,
    sessions: AsyncMutex<HashMap<String, SessionHandle>>,
    peer_semaphores: AsyncMutex<HashMap<String, Arc<Semaphore>>>,
    global_semaphore: Arc<Semaphore>,
    config: PairingStreamConfig,
}

struct SessionHandle {
    peer_id: String,
    write_tx: mpsc::Sender<PairingMessage>,
    shutdown_tx: watch::Sender<bool>,
    /// Set to `true` when the application explicitly closes the session via
    /// `close_pairing_session`.  The `run_session` task reads this flag to
    /// decide whether a `StreamClosedByPeer` shutdown should be bridged to a
    /// `PairingFailed` event.
    app_closed_tx: watch::Sender<bool>,
    _global_permit: OwnedSemaphorePermit,
    _peer_permit: OwnedSemaphorePermit,
}

impl PairingStreamService {
    pub fn new(
        control: stream::Control,
        event_tx: mpsc::Sender<NetworkEvent>,
        config: PairingStreamConfig,
    ) -> Self {
        Self {
            inner: Arc::new(PairingStreamServiceInner {
                control: AsyncMutex::new(control),
                event_tx,
                sessions: AsyncMutex::new(HashMap::new()),
                peer_semaphores: AsyncMutex::new(HashMap::new()),
                global_semaphore: Arc::new(Semaphore::new(MAX_PAIRING_CONCURRENCY)),
                config,
            }),
        }
    }

    #[cfg(test)]
    pub fn for_tests(event_tx: mpsc::Sender<NetworkEvent>, config: PairingStreamConfig) -> Self {
        let behaviour = stream::Behaviour::new();
        Self::new(behaviour.new_control(), event_tx, config)
    }

    pub fn spawn_accept_loop(&self) {
        let service = self.clone();
        tokio::spawn(async move {
            service.run_accept_loop().await;
        });
    }

    async fn run_accept_loop(&self) {
        let mut incoming = {
            let mut control = self.inner.control.lock().await;
            match control.accept(StreamProtocol::new(ProtocolId::PairingStream.as_str())) {
                Ok(incoming) => incoming,
                Err(err) => {
                    warn!("failed to accept pairing stream: {err}");
                    return;
                }
            }
        };
        while let Some((peer, stream)) = incoming.next().await {
            let peer_id = peer.to_string();
            let service = self.clone();
            let stream = stream.compat();
            tokio::spawn(async move {
                let handle = service.handle_incoming_stream(peer_id, stream);
                let result = handle.await;
                if let Err(err) = result {
                    warn!("pairing stream task join failed: {err}");
                } else if let Ok(Err(err)) = result {
                    warn!("pairing stream session failed: {err}");
                }
            });
        }
    }

    pub fn handle_incoming_stream<S>(
        &self,
        peer_id: String,
        stream: S,
    ) -> tokio::task::JoinHandle<Result<()>>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let service = self.clone();
        tokio::spawn(async move { service.run_incoming_session(peer_id, stream).await })
    }

    pub async fn open_pairing_session(&self, peer_id: String, session_id: String) -> Result<()> {
        {
            let sessions = self.inner.sessions.lock().await;
            if sessions.contains_key(&session_id) {
                return Ok(());
            }
        }
        let peer = peer_id
            .parse::<PeerId>()
            .map_err(|err| anyhow!("invalid peer id for pairing stream: {err}"))?;
        let permits = self.acquire_permits(&peer_id).await?;
        let stream = {
            let mut control = self.inner.control.lock().await;
            control
                .open_stream(
                    peer,
                    StreamProtocol::new(ProtocolId::PairingStream.as_str()),
                )
                .await
                .map_err(|err| match err {
                    stream::OpenStreamError::UnsupportedProtocol(_) => {
                        PairingStreamError::UnsupportedProtocol.into()
                    }
                    stream::OpenStreamError::Io(error) => {
                        anyhow!("pairing stream open failed: {error}")
                    }
                    other => anyhow!("pairing stream open failed: {other}"),
                })?
        };
        let stream = stream.compat();
        match self
            .spawn_session(peer_id, session_id, stream, None, permits)
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => {
                if let Some(PairingStreamError::SessionExists { .. }) =
                    err.downcast_ref::<PairingStreamError>()
                {
                    Ok(())
                } else {
                    Err(err)
                }
            }
        }
    }

    pub async fn has_session(&self, session_id: &str) -> bool {
        let sessions = self.inner.sessions.lock().await;
        sessions.contains_key(session_id)
    }

    #[tracing::instrument(skip(self, message), fields(session_id = %message.session_id()))]
    pub async fn send_pairing_on_session(&self, message: PairingMessage) -> Result<()> {
        let session_id = message.session_id().to_string();
        let message_kind = pairing_message_kind(&message);
        let sender = {
            let sessions = self.inner.sessions.lock().await;
            sessions
                .get(&session_id)
                .map(|handle| handle.write_tx.clone())
                .ok_or_else(|| anyhow!("pairing session not open: {session_id}"))?
        };
        debug!(
            session_id = %session_id,
            message_kind,
            "queueing pairing message onto session writer"
        );
        sender
            .send(message)
            .await
            .map_err(|err| anyhow!("pairing session send failed: {err}"))?;
        info!(
            session_id = %session_id,
            message_kind,
            "pairing message accepted by session writer queue"
        );
        Ok(())
    }

    #[tracing::instrument(skip(self), fields(peer_id = %peer_id))]
    pub async fn close_sessions_for_peer(&self, peer_id: &str) -> Result<()> {
        let sessions_to_close = {
            let sessions = self.inner.sessions.lock().await;
            sessions
                .iter()
                .filter_map(|(session_id, handle)| {
                    if handle.peer_id == peer_id {
                        Some(session_id.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };

        for session_id in sessions_to_close {
            self.close_pairing_session(
                session_id,
                Some("peer unpaired; closing pairing session".to_string()),
            )
            .await?;
        }

        Ok(())
    }

    pub async fn close_pairing_session(
        &self,
        session_id: String,
        reason: Option<String>,
    ) -> Result<()> {
        let handle = {
            let mut sessions = self.inner.sessions.lock().await;
            sessions.remove(&session_id)
        };
        if let Some(handle) = handle {
            // Mark session as application-initiated close BEFORE sending the
            // shutdown signal so that run_session can observe the flag when it
            // decides whether to bridge StreamClosedByPeer into PairingFailed.
            let _ = handle.app_closed_tx.send(true);
            if let Err(err) = handle.shutdown_tx.send(true) {
                warn!("pairing session shutdown send failed: {err}");
            }
            info!(
                event = "pairing_stream.closed",
                session_id = %session_id,
                peer_id = %handle.peer_id,
                close_initiator = "application",
                close_reason = %reason.as_deref().unwrap_or("explicit_close"),
                "pairing session close requested"
            );
        }
        Ok(())
    }

    async fn run_incoming_session<S>(&self, peer_id: String, mut stream: S) -> Result<()>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let permits = self.acquire_permits(&peer_id).await?;
        let first_payload = self.read_frame(&mut stream).await?;
        let first_payload =
            first_payload.ok_or_else(|| anyhow!("stream closed before first message"))?;
        info!(
            peer_id = %peer_id,
            payload_len = first_payload.len(),
            "read first pairing frame from inbound stream"
        );
        let first_message = self.decode_message(&peer_id, &first_payload)?;
        let session_id = first_message.session_id().to_string();
        let message_kind = pairing_message_kind(&first_message);
        info!(
            event = "pairing_stream.incoming_session_accepted",
            peer_id = %peer_id,
            session_id = %session_id,
            message_kind,
            payload_bytes = first_payload.len() as u64,
            "decoded first pairing message from inbound stream"
        );
        self.spawn_session(peer_id, session_id, stream, Some(first_message), permits)
            .await?
            .await?
    }

    async fn spawn_session<S>(
        &self,
        peer_id: String,
        session_id: String,
        stream: S,
        initial_message: Option<PairingMessage>,
        permits: SessionPermits,
    ) -> Result<tokio::task::JoinHandle<Result<()>>>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        self.ensure_session_slot(&session_id).await?;
        let (write_tx, write_rx) = mpsc::channel(self.inner.config.outbound_queue_depth);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (app_closed_tx, app_closed_rx) = watch::channel(false);
        let handle = SessionHandle {
            peer_id: peer_id.clone(),
            write_tx: write_tx.clone(),
            shutdown_tx: shutdown_tx.clone(),
            app_closed_tx,
            _global_permit: permits.global,
            _peer_permit: permits.peer,
        };
        {
            let mut sessions = self.inner.sessions.lock().await;
            sessions.insert(session_id.clone(), handle);
        }

        let inner = self.inner.clone();
        let span = info_span!(
            "pairing.session",
            trace_id = tracing::field::Empty,
            trace_ts = tracing::field::Empty,
            peer_id = %peer_id,
            session_id = %session_id,
        );
        record_trace_fields(&span, &None);
        let task = tokio::spawn(
            async move {
                let result = run_session(
                    inner.clone(),
                    peer_id,
                    session_id.clone(),
                    stream,
                    initial_message,
                    write_rx,
                    shutdown_tx,
                    shutdown_rx,
                    app_closed_rx,
                )
                .await;
                let mut sessions = inner.sessions.lock().await;
                sessions.remove(&session_id);
                result
            }
            .instrument(span),
        );
        Ok(task)
    }

    async fn ensure_session_slot(&self, session_id: &str) -> Result<()> {
        let sessions = self.inner.sessions.lock().await;
        if sessions.contains_key(session_id) {
            return Err(PairingStreamError::SessionExists {
                session_id: session_id.to_string(),
            }
            .into());
        }
        Ok(())
    }

    async fn acquire_permits(&self, peer_id: &str) -> Result<SessionPermits> {
        let global = self
            .inner
            .global_semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow!("pairing global semaphore closed"))?;
        let peer_semaphore = {
            let mut semaphores = self.inner.peer_semaphores.lock().await;
            semaphores
                .entry(peer_id.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(PER_PEER_CONCURRENCY)))
                .clone()
        };
        let peer = peer_semaphore
            .acquire_owned()
            .await
            .map_err(|_| anyhow!("pairing peer semaphore closed"))?;
        Ok(SessionPermits { global, peer })
    }

    async fn read_frame<R>(&self, reader: &mut R) -> Result<Option<Vec<u8>>>
    where
        R: AsyncRead + Unpin,
    {
        self.inner.read_frame(reader).await
    }

    fn decode_message(&self, peer_id: &str, payload: &[u8]) -> Result<PairingMessage> {
        self.inner.decode_message(peer_id, payload)
    }
}

fn pairing_message_kind(message: &PairingMessage) -> &'static str {
    match message {
        PairingMessage::Request(_) => "request",
        PairingMessage::Challenge(_) => "challenge",
        PairingMessage::KeyslotOffer(_) => "keyslot_offer",
        PairingMessage::ChallengeResponse(_) => "challenge_response",
        PairingMessage::Response(_) => "response",
        PairingMessage::Confirm(_) => "confirm",
        PairingMessage::Reject(_) => "reject",
        PairingMessage::Cancel(_) => "cancel",
        PairingMessage::Busy(_) => "busy",
    }
}

struct SessionPermits {
    global: OwnedSemaphorePermit,
    peer: OwnedSemaphorePermit,
}

async fn run_session<S>(
    inner: Arc<PairingStreamServiceInner>,
    peer_id: String,
    session_id: String,
    stream: S,
    initial_message: Option<PairingMessage>,
    write_rx: mpsc::Receiver<PairingMessage>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    app_closed_rx: watch::Receiver<bool>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (reader, writer) = tokio::io::split(stream);
    let session_direction = if initial_message.is_some() {
        "inbound"
    } else {
        "outbound"
    };
    info!(
        event = "pairing_stream.session_started",
        peer_id = %peer_id,
        session_id = %session_id,
        session_direction,
        idle_timeout_ms = inner.config.idle_timeout.as_millis() as u64,
        "pairing session started"
    );
    if let Some(message) = initial_message {
        if let Err(err) = emit_pairing_event(&inner.event_tx, &peer_id, message).await {
            warn!("pairing event emit failed: {err}");
            return Err(err);
        }
    }

    let mut read_task = tokio::spawn(read_loop(
        inner.clone(),
        peer_id.clone(),
        session_id.clone(),
        reader,
        shutdown_rx.clone(),
    ));
    let mut write_task = tokio::spawn(write_loop(
        inner.clone(),
        peer_id.clone(),
        session_id.clone(),
        writer,
        write_rx,
        shutdown_rx.clone(),
    ));

    enum CompletedTask {
        Read,
        Write,
    }

    let (result, completed) = tokio::select! {
        read_result = &mut read_task => (
            read_result.map_err(|err| anyhow!("pairing read task join failed: {err}"))?,
            CompletedTask::Read,
        ),
        write_result = &mut write_task => (
            write_result.map_err(|err| anyhow!("pairing write task join failed: {err}"))?,
            CompletedTask::Write,
        ),
    };

    send_shutdown_signal(&shutdown_tx);
    match completed {
        CompletedTask::Read => {
            write_task.abort();
            let _ = write_task.await;
        }
        CompletedTask::Write => {
            read_task.abort();
            let _ = read_task.await;
        }
    }

    match &result {
        Ok(reason) => {
            let source = match completed {
                CompletedTask::Read => "read_loop",
                CompletedTask::Write => "write_loop",
            };
            // A clean close initiated by the remote peer (EOF / stream dropped)
            // while no explicit application-level shutdown was signalled means
            // the session ended without the pairing protocol completing
            // normally.  Bridge this to a PairingFailed event so that the
            // joiner side does not stall silently in ProcessingJoinSpace when
            // the responder drops the connection after sending (or failing to
            // send) a Reject frame.
            //
            // We only emit the fallback when:
            //   1. The read side reported StreamClosedByPeer (remote-initiated EOF), AND
            //   2. The application did NOT explicitly close the session via
            //      `close_pairing_session` (app_closed flag not set).
            //
            // The `app_closed` flag is set BEFORE the shutdown signal is sent,
            // so even in the race where the read loop sees EOF before the
            // shutdown signal, we can still detect the application intent.
            let is_app_closed = *app_closed_rx.borrow();
            let close_initiator = close_initiator_for_shutdown_reason(&reason, is_app_closed);
            if matches!(
                (reason, &completed),
                (ShutdownReason::StreamClosedByPeer, CompletedTask::Read)
            ) && !is_app_closed
            {
                warn!(
                    event = "pairing_stream.ended",
                    peer_id = %peer_id,
                    session_id = %session_id,
                    completion_source = source,
                    end_reason = %reason,
                    close_initiator,
                    "pairing session closed by peer without explicit protocol termination; bridging to PairingFailed"
                );
                if let Err(e) = inner
                    .event_tx
                    .send(NetworkEvent::PairingFailed {
                        session_id: session_id.clone(),
                        peer_id: peer_id.clone(),
                        error: "stream_closed_by_peer".to_string(),
                    })
                    .await
                {
                    warn!("failed to emit pairing failed event on peer close: {}", e);
                }
            } else {
                info!(
                    event = "pairing_stream.ended",
                    peer_id = %peer_id,
                    session_id = %session_id,
                    completion_source = source,
                    end_reason = %reason,
                    close_initiator,
                    "pairing session ended"
                );
            }
        }
        Err(err) => {
            let source = match completed {
                CompletedTask::Read => "read_loop",
                CompletedTask::Write => "write_loop",
            };
            let close_initiator = close_initiator_for_error(err, source);
            let end_reason = end_reason_for_error(err, source);
            warn!(
                event = "pairing_stream.ended_with_error",
                peer_id = %peer_id,
                session_id = %session_id,
                completion_source = source,
                close_initiator,
                end_reason,
                error = %err,
                "pairing session ended with error"
            );
            if let Err(e) = inner
                .event_tx
                .send(NetworkEvent::PairingFailed {
                    session_id: session_id.clone(),
                    peer_id: peer_id.clone(),
                    error: err.to_string(),
                })
                .await
            {
                warn!("failed to emit pairing failed event: {}", e);
            }
        }
    }

    result.map(|_| ())
}

async fn read_loop<R>(
    inner: Arc<PairingStreamServiceInner>,
    peer_id: String,
    session_id: String,
    mut reader: R,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<ShutdownReason>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                return Ok(ShutdownReason::ExplicitClose);
            }
            payload = inner.read_frame(&mut reader) => {
                let payload = payload.map_err(|err| {
                    warn!("pairing read failed peer={peer_id} session={session_id}: {err}");
                    err
                })?;
                let payload = match payload {
                    Some(p) => p,
                    None => return Ok(ShutdownReason::StreamClosedByPeer),
                };
                let message = inner.decode_message(&peer_id, &payload).map_err(|err| {
                    warn!("pairing decode failed peer={peer_id} session={session_id}: {err}");
                    err
                })?;
                if let Err(err) = emit_pairing_event(&inner.event_tx, &peer_id, message).await {
                    warn!("pairing event emit failed peer={peer_id} session={session_id}: {err}");
                    return Err(err);
                }
            }
        }
    }
}

fn send_shutdown_signal(shutdown_tx: &watch::Sender<bool>) {
    match shutdown_tx.send(true) {
        Ok(()) => {}
        Err(err) => warn!("pairing session shutdown send failed: {err}"),
    }
}

async fn write_message<W>(
    inner: &PairingStreamServiceInner,
    peer_id: &str,
    session_id: &str,
    writer: &mut W,
    message: PairingMessage,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload =
        serde_json::to_vec(&message).map_err(|err| anyhow!("pairing encode failed: {err}"))?;
    if payload.len() > inner.config.max_frame_bytes {
        let err = anyhow!(
            "pairing frame exceeds max: {} > {}",
            payload.len(),
            inner.config.max_frame_bytes
        );
        warn!("pairing write failed peer={peer_id} session={session_id}: {err}");
        return Err(err);
    }
    if let Err(err) = write_length_prefixed(writer, &payload).await {
        warn!("pairing write failed peer={peer_id} session={session_id}: {err}");
        return Err(err);
    }
    Ok(())
}

async fn write_loop<W>(
    inner: Arc<PairingStreamServiceInner>,
    peer_id: String,
    session_id: String,
    mut writer: W,
    mut write_rx: mpsc::Receiver<PairingMessage>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<ShutdownReason>
where
    W: AsyncWrite + Unpin + Send,
{
    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                break;
            }
            message = write_rx.recv() => {
                let message = match message {
                    Some(message) => message,
                    None => return Ok(ShutdownReason::ChannelClosed),
                };
                write_message(&inner, &peer_id, &session_id, &mut writer, message).await?;
            }
        }
    }

    // Drain phase
    let drain_timeout = Duration::from_millis(250);
    let drain_start = tokio::time::Instant::now();

    loop {
        if drain_start.elapsed() > drain_timeout {
            warn!(
                "pairing session drain timed out: peer_id={} session_id={}",
                peer_id, session_id
            );
            break;
        }

        let remaining = drain_timeout.saturating_sub(drain_start.elapsed());
        if remaining.is_zero() {
            break;
        }

        match timeout(remaining, write_rx.recv()).await {
            Ok(Some(message)) => {
                write_message(&inner, &peer_id, &session_id, &mut writer, message).await?;
            }
            Ok(None) => break,
            Err(_) => {
                // Timeout waiting for next message, treat as done
                break;
            }
        }
    }

    Ok(ShutdownReason::ExplicitClose)
}

async fn emit_pairing_event(
    event_tx: &mpsc::Sender<NetworkEvent>,
    peer_id: &str,
    message: PairingMessage,
) -> Result<()> {
    event_tx
        .send(NetworkEvent::PairingMessageReceived {
            peer_id: peer_id.to_string(),
            message,
        })
        .await
        .map_err(|err| anyhow!("failed to emit pairing message: {err}"))
}

fn record_trace_fields(span: &Span, trace: &Option<TraceMetadata>) {
    if let Some(metadata) = trace.as_ref() {
        span.record("trace_id", tracing::field::display(&metadata.trace_id));
        span.record("trace_ts", metadata.timestamp);
    }
}
impl PairingStreamServiceInner {
    async fn read_frame<R>(&self, reader: &mut R) -> Result<Option<Vec<u8>>>
    where
        R: AsyncRead + Unpin,
    {
        let read_future = read_length_prefixed(reader, self.config.max_frame_bytes);
        match timeout(self.config.idle_timeout, read_future).await {
            Ok(result) => result,
            Err(_) => Err(anyhow!("pairing stream idle timeout")),
        }
    }

    fn decode_message(&self, peer_id: &str, payload: &[u8]) -> Result<PairingMessage> {
        serde_json::from_slice(payload)
            .map_err(|err| anyhow!("invalid pairing message from {peer_id}: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::send_shutdown_signal;
    use super::{PairingStreamConfig, PairingStreamService};
    use anyhow::anyhow;
    use libp2p::PeerId;
    use std::sync::{Arc, Mutex};
    use tokio::sync::{mpsc, watch};
    use tokio::time::{timeout, Duration};
    use tracing::field::{Field, Visit};
    use tracing::{Event, Subscriber};
    use tracing_subscriber::layer::{Context, Layer};
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::registry::LookupSpan;

    #[test]
    fn close_initiator_prefers_application_when_app_closed() {
        assert_eq!(
            super::close_initiator_for_shutdown_reason(
                &super::ShutdownReason::StreamClosedByPeer,
                true,
            ),
            "application"
        );
        assert_eq!(
            super::close_initiator_for_shutdown_reason(
                &super::ShutdownReason::ExplicitClose,
                false,
            ),
            "application"
        );
        assert_eq!(
            super::close_initiator_for_shutdown_reason(
                &super::ShutdownReason::StreamClosedByPeer,
                false,
            ),
            "peer"
        );
        assert_eq!(
            super::close_initiator_for_shutdown_reason(
                &super::ShutdownReason::ChannelClosed,
                false,
            ),
            "local"
        );
    }

    #[test]
    fn error_classification_normalizes_idle_timeout_and_peer_errors() {
        let idle_timeout = anyhow!("pairing stream idle timeout");
        assert_eq!(
            super::close_initiator_for_error(&idle_timeout, "read_loop"),
            "local"
        );
        assert_eq!(
            super::end_reason_for_error(&idle_timeout, "read_loop"),
            "idle_timeout"
        );

        let peer_closed = anyhow!("broken pipe");
        assert_eq!(
            super::close_initiator_for_error(&peer_closed, "write_loop"),
            "peer"
        );
        assert_eq!(
            super::end_reason_for_error(&peer_closed, "write_loop"),
            "peer_transport_error"
        );

        let fallback = anyhow!("some other io failure");
        assert_eq!(
            super::close_initiator_for_error(&fallback, "write_loop"),
            "unknown"
        );
        assert_eq!(
            super::end_reason_for_error(&fallback, "write_loop"),
            "write_error"
        );
    }

    #[tokio::test]
    async fn open_pairing_session_is_idempotent_when_session_exists() {
        let (event_tx, _event_rx) = mpsc::channel(1);
        let service = PairingStreamService::for_tests(event_tx, PairingStreamConfig::default());
        let peer_id = PeerId::random().to_string();
        let session_id = "session-1".to_string();
        let permits = service.acquire_permits(&peer_id).await.expect("permits");
        let (write_tx, _write_rx) = mpsc::channel(1);
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let (app_closed_tx, _app_closed_rx) = watch::channel(false);
        let handle = super::SessionHandle {
            peer_id: peer_id.clone(),
            write_tx,
            shutdown_tx,
            app_closed_tx,
            _global_permit: permits.global,
            _peer_permit: permits.peer,
        };
        {
            let mut sessions = service.inner.sessions.lock().await;
            sessions.insert(session_id.clone(), handle);
        }

        let result = timeout(
            Duration::from_millis(200),
            service.open_pairing_session(peer_id, session_id),
        )
        .await
        .expect("idempotent open timeout");
        result.expect("idempotent open");
    }

    #[tokio::test]
    async fn close_sessions_for_peer_only_removes_target_peer_sessions() {
        let (event_tx, _event_rx) = mpsc::channel(1);
        let service = PairingStreamService::for_tests(event_tx, PairingStreamConfig::default());
        let target_peer = PeerId::random().to_string();
        let other_peer = PeerId::random().to_string();

        let target_permits_a = service
            .acquire_permits(&target_peer)
            .await
            .expect("target permits a");
        let target_permits_b = service
            .acquire_permits(&target_peer)
            .await
            .expect("target permits b");
        let other_permits = service
            .acquire_permits(&other_peer)
            .await
            .expect("other permits");

        let (target_write_tx_a, _target_write_rx_a) = mpsc::channel(1);
        let (target_shutdown_tx_a, _target_shutdown_rx_a) = watch::channel(false);
        let (target_app_closed_tx_a, _target_app_closed_rx_a) = watch::channel(false);
        let (target_write_tx_b, _target_write_rx_b) = mpsc::channel(1);
        let (target_shutdown_tx_b, _target_shutdown_rx_b) = watch::channel(false);
        let (target_app_closed_tx_b, _target_app_closed_rx_b) = watch::channel(false);
        let (other_write_tx, _other_write_rx) = mpsc::channel(1);
        let (other_shutdown_tx, _other_shutdown_rx) = watch::channel(false);
        let (other_app_closed_tx, _other_app_closed_rx) = watch::channel(false);

        {
            let mut sessions = service.inner.sessions.lock().await;
            sessions.insert(
                "target-session-a".to_string(),
                super::SessionHandle {
                    peer_id: target_peer.clone(),
                    write_tx: target_write_tx_a,
                    shutdown_tx: target_shutdown_tx_a,
                    app_closed_tx: target_app_closed_tx_a,
                    _global_permit: target_permits_a.global,
                    _peer_permit: target_permits_a.peer,
                },
            );
            sessions.insert(
                "target-session-b".to_string(),
                super::SessionHandle {
                    peer_id: target_peer.clone(),
                    write_tx: target_write_tx_b,
                    shutdown_tx: target_shutdown_tx_b,
                    app_closed_tx: target_app_closed_tx_b,
                    _global_permit: target_permits_b.global,
                    _peer_permit: target_permits_b.peer,
                },
            );
            sessions.insert(
                "other-session".to_string(),
                super::SessionHandle {
                    peer_id: other_peer.clone(),
                    write_tx: other_write_tx,
                    shutdown_tx: other_shutdown_tx,
                    app_closed_tx: other_app_closed_tx,
                    _global_permit: other_permits.global,
                    _peer_permit: other_permits.peer,
                },
            );
        }

        service
            .close_sessions_for_peer(&target_peer)
            .await
            .expect("close sessions for target peer");

        let sessions = service.inner.sessions.lock().await;
        assert!(!sessions.contains_key("target-session-a"));
        assert!(!sessions.contains_key("target-session-b"));
        assert!(sessions.contains_key("other-session"));
        assert_eq!(
            sessions
                .get("other-session")
                .expect("other session")
                .peer_id,
            other_peer
        );
    }

    #[derive(Default)]
    struct MessageVisitor {
        message: Option<String>,
    }

    impl Visit for MessageVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "message" {
                self.message = Some(value.to_string());
            }
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            if field.name() == "message" && self.message.is_none() {
                self.message = Some(format!("{value:?}").trim_matches('"').to_string());
            }
        }
    }

    #[derive(Clone)]
    struct WarningCaptureLayer {
        warnings: Arc<Mutex<Vec<String>>>,
    }

    impl WarningCaptureLayer {
        fn new(warnings: Arc<Mutex<Vec<String>>>) -> Self {
            Self { warnings }
        }
    }

    impl<S> Layer<S> for WarningCaptureLayer
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
            if *event.metadata().level() > tracing::Level::WARN {
                return;
            }

            let mut visitor = MessageVisitor::default();
            event.record(&mut visitor);
            if let Some(message) = visitor.message {
                self.warnings.lock().expect("warnings lock").push(message);
            }
        }
    }

    #[test]
    fn shutdown_signal_logs_warning_when_receiver_dropped() {
        let warnings = Arc::new(Mutex::new(Vec::<String>::new()));
        let subscriber =
            tracing_subscriber::registry().with(WarningCaptureLayer::new(warnings.clone()));
        let _guard = tracing::subscriber::set_default(subscriber);

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        drop(shutdown_rx);

        send_shutdown_signal(&shutdown_tx);

        let warnings = warnings.lock().expect("warnings lock");
        assert!(warnings
            .iter()
            .any(|entry| entry.contains("pairing session shutdown send failed")));
    }

    /// Simulate EOF after an established session: the peer sends the initial
    /// PairingRequest but then closes the connection without sending a Reject
    /// frame.  The session should emit a PairingFailed event so that the
    /// join-space listener can exit ProcessingJoinSpace instead of stalling.
    #[tokio::test]
    async fn pairing_stream_clean_close_without_protocol_termination_emits_pairing_failed() {
        use super::super::framing::write_length_prefixed;
        use uc_core::network::protocol::PairingRequest;
        use uc_core::network::PairingMessage;

        let (event_tx, mut event_rx) = mpsc::channel(16);
        let service = PairingStreamService::for_tests(
            event_tx,
            PairingStreamConfig {
                idle_timeout: Duration::from_secs(5),
                ..Default::default()
            },
        );

        let peer_id = PeerId::random().to_string();
        let session_id = "sess-eof-after-request".to_string();

        let (mut server_tx, client_rx) = tokio::io::duplex(4096);

        // Write an initial PairingRequest frame so the session is established.
        let request = PairingMessage::Request(PairingRequest {
            session_id: session_id.clone(),
            peer_id: peer_id.clone(),
            device_name: "test-device".to_string(),
            device_id: "test-device-id".to_string(),
            identity_pubkey: vec![1; 32],
            nonce: vec![2; 16],
        });
        let payload = serde_json::to_vec(&request).expect("encode request");
        write_length_prefixed(&mut server_tx, &payload)
            .await
            .expect("write initial frame");

        // Drop server_tx immediately after writing — this causes EOF on client_rx
        // after the initial frame is read, simulating the peer closing the
        // connection without sending a Reject.
        drop(server_tx);

        let task = service.handle_incoming_stream(peer_id.clone(), client_rx);

        // Wait for the session to complete.
        let _ = timeout(Duration::from_secs(2), task)
            .await
            .expect("session task timed out");

        // Give the event a moment to propagate.
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Should see a PairingFailed event for the established session.
        let mut found_failed = false;
        while let Ok(event) = event_rx.try_recv() {
            if let uc_core::network::NetworkEvent::PairingFailed {
                session_id: ref sid,
                ..
            } = event
            {
                if sid == &session_id {
                    found_failed = true;
                }
            }
        }
        assert!(
            found_failed,
            "expected PairingFailed event when peer closes connection after sending initial frame \
             but before completing the pairing protocol"
        );
    }

    /// When close_pairing_session is called explicitly (application-initiated
    /// shutdown), the session should NOT emit a spurious PairingFailed event.
    #[tokio::test]
    async fn explicit_session_close_does_not_emit_pairing_failed() {
        use super::super::framing::write_length_prefixed;
        use uc_core::network::protocol::PairingRequest;
        use uc_core::network::PairingMessage;

        let (event_tx, mut event_rx) = mpsc::channel(8);
        let service = PairingStreamService::for_tests(
            event_tx,
            PairingStreamConfig {
                idle_timeout: Duration::from_secs(5),
                ..Default::default()
            },
        );

        let peer_id = PeerId::random().to_string();
        let session_id = "sess-explicit-close".to_string();

        // Set up a duplex stream where we send an initial PairingRequest so the
        // session is registered, then close via close_pairing_session.
        let (mut server_tx, client_rx) = tokio::io::duplex(4096);

        // Write an initial PairingRequest frame.
        let request = PairingMessage::Request(PairingRequest {
            session_id: session_id.clone(),
            peer_id: peer_id.clone(),
            device_name: "test-device".to_string(),
            device_id: "test-device-id".to_string(),
            identity_pubkey: vec![1; 32],
            nonce: vec![2; 16],
        });
        let payload = serde_json::to_vec(&request).expect("encode request");
        write_length_prefixed(&mut server_tx, &payload)
            .await
            .expect("write frame");

        let service_clone = service.clone();
        let session_id_clone = session_id.clone();
        // Give session time to start and register.
        let task = service.handle_incoming_stream(peer_id.clone(), client_rx);

        // Wait a moment for the session to be registered.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Explicitly close the session from the application side.
        service_clone
            .close_pairing_session(session_id_clone, Some("test explicit close".to_string()))
            .await
            .expect("close session");

        // Drop server_tx so read_loop gets EOF after shutdown.
        drop(server_tx);

        timeout(Duration::from_secs(2), task)
            .await
            .expect("session task timed out")
            .ok(); // Task result doesn't matter.

        // Drain events: should NOT contain PairingFailed.
        let mut found_failed = false;
        while let Ok(event) = event_rx.try_recv() {
            if matches!(event, uc_core::network::NetworkEvent::PairingFailed { .. }) {
                found_failed = true;
            }
        }
        assert!(
            !found_failed,
            "should not emit PairingFailed when session is closed explicitly by the application"
        );
    }
}
