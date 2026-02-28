use super::framing::{read_length_prefixed, write_length_prefixed, MAX_PAIRING_FRAME_BYTES};
use anyhow::{anyhow, Result};
use libp2p::{futures::StreamExt, PeerId, StreamProtocol};
use libp2p_stream as stream;
use log::{info, warn};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, watch, Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::{timeout, Duration};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{info_span, Instrument, Span};
use uc_core::network::{NetworkEvent, PairingMessage, ProtocolId};
use uc_core::ports::observability::TraceMetadata;

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

    pub async fn send_pairing_on_session(&self, message: PairingMessage) -> Result<()> {
        let session_id = message.session_id().to_string();
        let sender = {
            let sessions = self.inner.sessions.lock().await;
            sessions
                .get(&session_id)
                .map(|handle| handle.write_tx.clone())
                .ok_or_else(|| anyhow!("pairing session not open: {session_id}"))?
        };
        sender
            .send(message)
            .await
            .map_err(|err| anyhow!("pairing session send failed: {err}"))
    }

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
            if let Err(err) = handle.shutdown_tx.send(true) {
                warn!("pairing session shutdown send failed: {err}");
            }
            if let Some(reason) = reason.as_ref() {
                info!(
                    "pairing session closed: session_id={} peer_id={} reason={}",
                    session_id, handle.peer_id, reason
                );
            }
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
        let first_message = self.decode_message(&peer_id, &first_payload)?;
        let session_id = first_message.session_id().to_string();
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
        let handle = SessionHandle {
            peer_id: peer_id.clone(),
            write_tx: write_tx.clone(),
            shutdown_tx: shutdown_tx.clone(),
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
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (reader, writer) = tokio::io::split(stream);
    info!(
        "pairing session started: peer_id={} session_id={}",
        peer_id, session_id
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
            info!(
                "pairing session ended cleanly: peer_id={} session_id={} source={} reason={}",
                peer_id, session_id, source, reason
            );
        }
        Err(err) => {
            let source = match completed {
                CompletedTask::Read => "read_loop",
                CompletedTask::Write => "write_loop",
            };
            warn!(
                "pairing session ended with error: peer_id={} session_id={} source={} error={}",
                peer_id, session_id, source, err
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
    use libp2p::PeerId;
    use log::{Level, LevelFilter, Metadata, Record};
    use std::sync::{Mutex, Once};
    use tokio::sync::{mpsc, watch};
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn open_pairing_session_is_idempotent_when_session_exists() {
        let (event_tx, _event_rx) = mpsc::channel(1);
        let service = PairingStreamService::for_tests(event_tx, PairingStreamConfig::default());
        let peer_id = PeerId::random().to_string();
        let session_id = "session-1".to_string();
        let permits = service.acquire_permits(&peer_id).await.expect("permits");
        let (write_tx, _write_rx) = mpsc::channel(1);
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let handle = super::SessionHandle {
            peer_id: peer_id.clone(),
            write_tx,
            shutdown_tx,
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

    struct TestLogger {
        logs: Mutex<Vec<String>>,
    }

    impl log::Log for TestLogger {
        fn enabled(&self, metadata: &Metadata) -> bool {
            metadata.level() <= Level::Warn
        }

        fn log(&self, record: &Record) {
            if self.enabled(record.metadata()) {
                let mut logs = self.logs.lock().expect("logs lock");
                logs.push(format!("{}", record.args()));
            }
        }

        fn flush(&self) {}
    }

    static LOGGER: TestLogger = TestLogger {
        logs: Mutex::new(Vec::new()),
    };
    static LOGGER_INIT: Once = Once::new();

    fn init_logger() {
        LOGGER_INIT.call_once(|| {
            let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Warn));
        });
    }

    #[test]
    fn shutdown_signal_logs_warning_when_receiver_dropped() {
        init_logger();
        {
            let mut logs = LOGGER.logs.lock().expect("logs lock");
            logs.clear();
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        drop(shutdown_rx);

        send_shutdown_signal(&shutdown_tx);

        let logs = LOGGER.logs.lock().expect("logs lock");
        assert!(logs
            .iter()
            .any(|entry| entry.contains("pairing session shutdown send failed")));
    }
}
