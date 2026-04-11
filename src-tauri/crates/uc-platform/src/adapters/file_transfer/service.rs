//! File transfer service for chunked file transfers over libp2p streams.
//!
//! Follows PairingStreamService patterns with Arc<Inner>, semaphore-based
//! concurrency control, and async stream handling.

use super::framing::{read_file_frame, write_file_frame, FileMessageType};
use super::protocol::{
    FileAcceptance, FileAnnounce, FileAnnounceV2, LegacyFileTransferProtocol,
    StreamingFileTransferProtocol, CHUNK_SIZE,
};
use anyhow::{anyhow, Result};
use libp2p::{futures::StreamExt, PeerId, Stream, StreamProtocol};
use libp2p_stream as stream;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::{Duration, Instant};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use tracing::{info, info_span, instrument, warn, Instrument};
use uc_core::network::{NetworkEvent, ProtocolId};
use uc_core::ports::transfer_progress::{
    TransferDirection, TransferProgress, TransferProgressPort,
};

/// Maximum concurrent file transfers globally.
pub const MAX_FILE_TRANSFER_CONCURRENCY: usize = 8;

/// Maximum concurrent file transfers per peer.
const PER_PEER_FILE_CONCURRENCY: usize = 2;
const PROGRESS_LOG_STEP_PERCENT: u32 = 10;
const PROGRESS_EMIT_MIN_INTERVAL_MS: u64 = 250;
const PROGRESS_EMIT_MIN_BYTES: u64 = 4 * 1024 * 1024;

/// Configuration for the file transfer service.
#[derive(Debug, Clone)]
pub struct FileTransferConfig {
    pub chunk_size: usize,
    pub transfer_timeout: Duration,
    pub cache_dir: PathBuf,
}

impl FileTransferConfig {
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            chunk_size: CHUNK_SIZE,
            transfer_timeout: Duration::from_secs(300), // 5 minutes
            cache_dir,
        }
    }
}

/// File transfer service managing chunked file transfers over libp2p streams.
#[derive(Clone)]
pub struct FileTransferService {
    inner: Arc<FileTransferServiceInner>,
}

struct FileTransferServiceInner {
    control: AsyncMutex<stream::Control>,
    event_tx: mpsc::Sender<NetworkEvent>,
    progress_port: Arc<dyn TransferProgressPort>,
    peer_semaphores: AsyncMutex<HashMap<String, Arc<Semaphore>>>,
    global_semaphore: Arc<Semaphore>,
    config: FileTransferConfig,
    protocol_coordinator: FileTransferProtocolCoordinator,
}

struct TransferPermits {
    _global: OwnedSemaphorePermit,
    _peer: OwnedSemaphorePermit,
}

struct ProgressEmitGate {
    last_emit_at: Instant,
    last_emitted_bytes: u64,
}

async fn report_progress_with_warning(
    port: Arc<dyn TransferProgressPort>,
    progress: TransferProgress,
    stage: &'static str,
) {
    let progress_for_log = progress.clone();
    if let Err(err) = port.report_progress(progress).await {
        warn!(
            stage,
            transfer_id = %progress_for_log.transfer_id,
            peer_id = %progress_for_log.peer_id,
            direction = ?progress_for_log.direction,
            chunks_completed = progress_for_log.chunks_completed,
            total_chunks = progress_for_log.total_chunks,
            bytes_transferred = progress_for_log.bytes_transferred,
            total_bytes = ?progress_for_log.total_bytes,
            port_trait = %std::any::type_name_of_val(port.as_ref()),
            port_ptr = ?Arc::as_ptr(&port),
            error = %err,
            "failed to report file transfer progress"
        );
    }
}

impl ProgressEmitGate {
    fn new(now: Instant) -> Self {
        Self {
            last_emit_at: now,
            last_emitted_bytes: 0,
        }
    }

    fn should_emit(
        &mut self,
        now: Instant,
        bytes: u64,
        file_size: u64,
        chunks_completed: u32,
        total_chunks: u32,
    ) -> bool {
        if bytes == 0 {
            return false;
        }

        let is_first_chunk = chunks_completed <= 1;
        let is_final_chunk =
            bytes >= file_size || (total_chunks > 0 && chunks_completed >= total_chunks);
        let elapsed_ms = now.duration_since(self.last_emit_at).as_millis() as u64;
        let advanced_bytes = bytes.saturating_sub(self.last_emitted_bytes);
        let should_emit = is_first_chunk
            || is_final_chunk
            || elapsed_ms >= PROGRESS_EMIT_MIN_INTERVAL_MS
            || advanced_bytes >= PROGRESS_EMIT_MIN_BYTES;

        if should_emit {
            self.last_emit_at = now;
            self.last_emitted_bytes = bytes;
        }

        should_emit
    }
}

#[derive(Clone, Copy)]
enum FileTransferProtocolVersion {
    V1,
    V2,
}

#[derive(Clone, Copy, Default)]
struct FileTransferProtocolCoordinator {
    legacy: LegacyFileTransferProtocol,
    streaming: StreamingFileTransferProtocol,
}

impl FileTransferService {
    /// Create a new file transfer service.
    pub fn new(
        control: stream::Control,
        event_tx: mpsc::Sender<NetworkEvent>,
        progress_port: Arc<dyn TransferProgressPort>,
        config: FileTransferConfig,
    ) -> Self {
        Self {
            inner: Arc::new(FileTransferServiceInner {
                control: AsyncMutex::new(control),
                event_tx,
                progress_port,
                peer_semaphores: AsyncMutex::new(HashMap::new()),
                global_semaphore: Arc::new(Semaphore::new(MAX_FILE_TRANSFER_CONCURRENCY)),
                config,
                protocol_coordinator: FileTransferProtocolCoordinator::default(),
            }),
        }
    }

    /// Spawn the accept loop for incoming file transfers.
    pub fn spawn_accept_loop(&self) {
        for version in [
            FileTransferProtocolVersion::V1,
            FileTransferProtocolVersion::V2,
        ] {
            let service = self.clone();
            tokio::spawn(async move {
                service.run_accept_loop(version).await;
            });
        }
    }

    async fn run_accept_loop(&self, version: FileTransferProtocolVersion) {
        let mut incoming = {
            let mut control = self.inner.control.lock().await;
            match control.accept(StreamProtocol::new(protocol_id(version).as_str())) {
                Ok(incoming) => incoming,
                Err(err) => {
                    warn!("failed to accept file transfer stream: {err}");
                    return;
                }
            }
        };

        while let Some((peer, stream)) = incoming.next().await {
            let peer_id = peer.to_string();
            let service = self.clone();
            let stream = stream.compat();
            let span_peer_id = peer_id.clone();
            let span = info_span!(
                "file_transfer.incoming",
                peer_id = %span_peer_id,
                protocol_version = protocol_version_label(version)
            );
            tokio::spawn(
                async move {
                    if let Err(err) = service
                        .handle_incoming_transfer(peer_id.clone(), stream, version)
                        .await
                    {
                        warn!(
                            peer_id = %peer_id,
                            protocol_version = protocol_version_label(version),
                            error = %err,
                            "file transfer failed"
                        );
                    }
                }
                .instrument(span),
            );
        }
    }

    /// Handle an incoming file transfer stream.
    async fn handle_incoming_transfer<S>(
        &self,
        peer_id: String,
        mut stream: S,
        version: FileTransferProtocolVersion,
    ) -> Result<()>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let permits = self.acquire_permits(&peer_id).await?;

        // Read announce frame
        let frame = read_file_frame(&mut stream)
            .await?
            .ok_or_else(|| anyhow!("stream closed before announce"))?;

        if frame.0 != FileMessageType::Announce {
            return Err(anyhow!("expected announce frame, got {:?}", frame.0));
        }

        let incoming = match version {
            FileTransferProtocolVersion::V1 => IncomingTransfer::V1(
                serde_json::from_slice(&frame.1)
                    .map_err(|e| anyhow!("invalid v1 announce message: {}", e))?,
            ),
            FileTransferProtocolVersion::V2 => IncomingTransfer::V2(
                serde_json::from_slice(&frame.1)
                    .map_err(|e| anyhow!("invalid v2 announce message: {}", e))?,
            ),
        };

        info!(
            transfer_id = %incoming.transfer_id(),
            peer_id = %peer_id,
            filename = %incoming.filename(),
            file_size = incoming.file_size(),
            protocol_version = protocol_version_label(version),
            "incoming file transfer"
        );

        // Emit start event
        let _ = self
            .inner
            .event_tx
            .send(NetworkEvent::FileTransferStarted {
                transfer_id: incoming.transfer_id().to_string(),
                peer_id: peer_id.clone(),
                filename: incoming.filename().to_string(),
                file_size: incoming.file_size(),
            })
            .await;

        // Check disk space (basic check)
        let cache_dir = &self.inner.config.cache_dir;
        if let Err(space_err) = check_disk_space(cache_dir, incoming.file_size()).await {
            let rejection = FileAcceptance {
                transfer_id: incoming.transfer_id().to_string(),
                accepted: false,
                reason: Some(space_err.to_string()),
            };
            let rejection_bytes = serde_json::to_vec(&rejection)?;
            write_file_frame(&mut stream, FileMessageType::Reject, &rejection_bytes).await?;

            let _ = self
                .inner
                .event_tx
                .send(NetworkEvent::FileTransferFailed {
                    transfer_id: incoming.transfer_id().to_string(),
                    peer_id: peer_id.clone(),
                    error: space_err.to_string(),
                })
                .await;
            return Err(space_err);
        }

        // Send acceptance
        let acceptance = FileAcceptance {
            transfer_id: incoming.transfer_id().to_string(),
            accepted: true,
            reason: None,
        };
        let acceptance_bytes = serde_json::to_vec(&acceptance)?;
        write_file_frame(&mut stream, FileMessageType::Accept, &acceptance_bytes).await?;

        // Receive the file
        let progress_port = self.inner.progress_port.clone();
        let peer_id_clone = peer_id.clone();
        let transfer_id_clone = incoming.transfer_id().to_string();
        let filename_clone = incoming.filename().to_string();
        let file_size = incoming.file_size();
        let progress_log_state = Arc::new(AtomicU32::new(PROGRESS_LOG_STEP_PERCENT));
        let progress_log_state_clone = progress_log_state.clone();
        let progress_started_at = Instant::now();
        let progress_emit_gate = Arc::new(std::sync::Mutex::new(ProgressEmitGate::new(
            progress_started_at,
        )));
        let progress_emit_gate_clone = progress_emit_gate.clone();

        let progress_callback = move |chunks_completed: u32, total_chunks: u32, bytes: u64| {
            maybe_log_progress(
                &progress_log_state_clone,
                "receiving",
                &transfer_id_clone,
                &peer_id_clone,
                &filename_clone,
                file_size,
                bytes,
                chunks_completed,
                total_chunks,
                progress_started_at,
            );
            let should_emit = {
                let mut gate = progress_emit_gate_clone
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                gate.should_emit(
                    Instant::now(),
                    bytes,
                    file_size,
                    chunks_completed,
                    total_chunks,
                )
            };
            if !should_emit {
                return;
            }
            let progress = TransferProgress {
                transfer_id: transfer_id_clone.clone(),
                peer_id: peer_id_clone.clone(),
                direction: TransferDirection::Receiving,
                chunks_completed,
                total_chunks,
                bytes_transferred: bytes,
                total_bytes: Some(file_size),
            };
            let port = progress_port.clone();
            tokio::spawn(async move {
                report_progress_with_warning(port, progress, "receiving").await;
            });
        };

        let result = self
            .inner
            .protocol_coordinator
            .receive_on_stream(&mut stream, &incoming, cache_dir, Some(&progress_callback))
            .await;

        // Hold permits until transfer completes
        drop(permits);

        match result {
            Ok(final_path) => {
                info!(
                    transfer_id = %incoming.transfer_id(),
                    peer_id = %peer_id,
                    protocol_version = protocol_version_label(version),
                    path = %final_path.display(),
                    "file transfer complete"
                );
                let _ = self
                    .inner
                    .event_tx
                    .send(NetworkEvent::FileTransferCompleted {
                        transfer_id: incoming.transfer_id().to_string(),
                        peer_id: peer_id.clone(),
                        filename: incoming.filename().to_string(),
                        file_path: final_path,
                        batch_id: incoming.batch_id().cloned(),
                        batch_total: incoming.batch_total(),
                    })
                    .await;
                Ok(())
            }
            Err(e) => {
                let _ = self
                    .inner
                    .event_tx
                    .send(NetworkEvent::FileTransferFailed {
                        transfer_id: incoming.transfer_id().to_string(),
                        peer_id: peer_id.clone(),
                        error: e.to_string(),
                    })
                    .await;
                warn!(
                    transfer_id = %incoming.transfer_id(),
                    peer_id = %peer_id,
                    protocol_version = protocol_version_label(version),
                    error = %e,
                    "incoming file transfer failed"
                );
                Err(e)
            }
        }
    }

    /// Send a file to a peer.
    #[instrument(
        name = "file_transfer.send",
        level = "info",
        skip(self, file_path),
        fields(
            peer_id = %peer_id_str,
            transfer_id = %transfer_id,
            file = %file_path.display(),
            batch_id = ?batch_id,
        )
    )]
    pub async fn send_file(
        &self,
        peer_id_str: &str,
        file_path: PathBuf,
        transfer_id: String,
        batch_id: Option<String>,
        batch_total: Option<u32>,
    ) -> Result<()> {
        let permits = self.acquire_permits(peer_id_str).await?;

        let peer = peer_id_str
            .parse::<PeerId>()
            .map_err(|err| anyhow!("invalid peer id: {err}"))?;

        // Open outbound stream
        let stream_open_started_at = Instant::now();
        let (stream, protocol_version) = self.open_outbound_stream(peer).await?;
        let stream_open_elapsed_ms = stream_open_started_at.elapsed().as_millis() as u64;
        let stream = stream.compat();
        let (mut read_half, mut write_half) = tokio::io::split(stream);

        // Emit start event
        let filename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        let file_size = tokio::fs::metadata(&file_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        info!(
            transfer_id = %transfer_id,
            peer_id = %peer_id_str,
            filename = %filename,
            file_size,
            protocol_version = protocol_version_label(protocol_version),
            stream_open_elapsed_ms,
            "outgoing file transfer started"
        );

        let _ = self
            .inner
            .event_tx
            .send(NetworkEvent::FileTransferStarted {
                transfer_id: transfer_id.clone(),
                peer_id: peer_id_str.to_string(),
                filename: filename.clone(),
                file_size,
            })
            .await;

        // Progress reporting
        let progress_port = self.inner.progress_port.clone();
        let peer_id_for_progress = peer_id_str.to_string();
        let transfer_id_for_progress = transfer_id.clone();
        let filename_for_progress = filename.clone();
        let progress_log_state = Arc::new(AtomicU32::new(PROGRESS_LOG_STEP_PERCENT));
        let progress_log_state_clone = progress_log_state.clone();
        let progress_started_at = Instant::now();
        let progress_emit_gate = Arc::new(std::sync::Mutex::new(ProgressEmitGate::new(
            progress_started_at,
        )));
        let progress_emit_gate_clone = progress_emit_gate.clone();
        let progress_callback = move |chunks_completed: u32, total_chunks: u32, bytes: u64| {
            maybe_log_progress(
                &progress_log_state_clone,
                "sending",
                &transfer_id_for_progress,
                &peer_id_for_progress,
                &filename_for_progress,
                file_size,
                bytes,
                chunks_completed,
                total_chunks,
                progress_started_at,
            );
            let should_emit = {
                let mut gate = progress_emit_gate_clone
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                gate.should_emit(
                    Instant::now(),
                    bytes,
                    file_size,
                    chunks_completed,
                    total_chunks,
                )
            };
            if !should_emit {
                return;
            }
            let progress = TransferProgress {
                transfer_id: transfer_id_for_progress.clone(),
                peer_id: peer_id_for_progress.clone(),
                direction: TransferDirection::Sending,
                chunks_completed,
                total_chunks,
                bytes_transferred: bytes,
                total_bytes: Some(file_size),
            };
            let port = progress_port.clone();
            tokio::spawn(async move {
                report_progress_with_warning(port, progress, "sending").await;
            });
        };

        // Send the file
        let send_result = self
            .inner
            .protocol_coordinator
            .send_on_stream(
                protocol_version,
                &mut write_half,
                &file_path,
                &transfer_id,
                batch_id.clone(),
                batch_total,
                self.inner.config.chunk_size,
                Some(&progress_callback),
            )
            .await;

        // Hold permits until done
        drop(permits);

        match send_result {
            Ok(_hash) => {
                // Read acceptance (best effort)
                match read_file_frame(&mut read_half).await {
                    Ok(Some((FileMessageType::Accept, _))) => {
                        info!(
                            transfer_id = %transfer_id,
                            peer_id = %peer_id_str,
                            protocol_version = protocol_version_label(protocol_version),
                            "file transfer accepted and sent"
                        );
                    }
                    Ok(Some((FileMessageType::Reject, payload))) => {
                        let rejection: FileAcceptance =
                            serde_json::from_slice(&payload).unwrap_or(FileAcceptance {
                                transfer_id: transfer_id.clone(),
                                accepted: false,
                                reason: Some("unknown rejection".to_string()),
                            });
                        let reason = rejection.reason.unwrap_or_default();
                        let _ = self
                            .inner
                            .event_tx
                            .send(NetworkEvent::FileTransferFailed {
                                transfer_id: transfer_id.clone(),
                                peer_id: peer_id_str.to_string(),
                                error: format!("rejected: {}", reason),
                            })
                            .await;
                        return Err(anyhow!("file transfer rejected: {}", reason));
                    }
                    _ => {
                        // No response or unexpected; treat as success since chunks were sent
                    }
                }

                let _ = self
                    .inner
                    .event_tx
                    .send(NetworkEvent::FileTransferCompleted {
                        transfer_id: transfer_id.clone(),
                        peer_id: peer_id_str.to_string(),
                        filename,
                        file_path,
                        batch_id,
                        batch_total,
                    })
                    .await;
                Ok(())
            }
            Err(e) => {
                let _ = self
                    .inner
                    .event_tx
                    .send(NetworkEvent::FileTransferFailed {
                        transfer_id: transfer_id.clone(),
                        peer_id: peer_id_str.to_string(),
                        error: e.to_string(),
                    })
                    .await;
                warn!(
                    transfer_id = %transfer_id,
                    peer_id = %peer_id_str,
                    protocol_version = protocol_version_label(protocol_version),
                    error = %e,
                    "outgoing file transfer failed"
                );
                Err(e)
            }
        }
    }

    async fn acquire_permits(&self, peer_id: &str) -> Result<TransferPermits> {
        let global = self
            .inner
            .global_semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow!("file transfer global semaphore closed"))?;

        let peer_semaphore = {
            let mut semaphores = self.inner.peer_semaphores.lock().await;
            semaphores
                .entry(peer_id.to_string())
                .or_insert_with(|| Arc::new(Semaphore::new(PER_PEER_FILE_CONCURRENCY)))
                .clone()
        };

        let peer = peer_semaphore
            .acquire_owned()
            .await
            .map_err(|_| anyhow!("file transfer peer semaphore closed"))?;

        Ok(TransferPermits {
            _global: global,
            _peer: peer,
        })
    }

    async fn open_outbound_stream(
        &self,
        peer: PeerId,
    ) -> Result<(Stream, FileTransferProtocolVersion)> {
        let mut control = self.inner.control.lock().await;
        self.inner
            .protocol_coordinator
            .open_outbound_stream(&mut control, peer)
            .await
    }
}

enum IncomingTransfer {
    V1(FileAnnounce),
    V2(FileAnnounceV2),
}

impl IncomingTransfer {
    fn transfer_id(&self) -> &str {
        match self {
            Self::V1(v) => &v.transfer_id,
            Self::V2(v) => &v.transfer_id,
        }
    }

    fn filename(&self) -> &str {
        match self {
            Self::V1(v) => &v.filename,
            Self::V2(v) => &v.filename,
        }
    }

    fn file_size(&self) -> u64 {
        match self {
            Self::V1(v) => v.file_size,
            Self::V2(v) => v.file_size,
        }
    }

    fn batch_id(&self) -> Option<&String> {
        match self {
            Self::V1(v) => v.batch_id.as_ref(),
            Self::V2(v) => v.batch_id.as_ref(),
        }
    }

    fn batch_total(&self) -> Option<u32> {
        match self {
            Self::V1(v) => v.batch_total,
            Self::V2(v) => v.batch_total,
        }
    }
}

fn protocol_id(version: FileTransferProtocolVersion) -> ProtocolId {
    match version {
        FileTransferProtocolVersion::V1 => ProtocolId::FileTransfer,
        FileTransferProtocolVersion::V2 => ProtocolId::FileTransferV2,
    }
}

impl FileTransferProtocolCoordinator {
    async fn open_outbound_stream(
        &self,
        control: &mut stream::Control,
        peer: PeerId,
    ) -> Result<(Stream, FileTransferProtocolVersion)> {
        match control
            .open_stream(
                peer,
                StreamProtocol::new(ProtocolId::FileTransferV2.as_str()),
            )
            .await
        {
            Ok(stream) => {
                info!(
                    peer_id = %peer,
                    protocol_version = protocol_version_label(FileTransferProtocolVersion::V2),
                    "opened outbound file transfer stream"
                );
                Ok((stream, FileTransferProtocolVersion::V2))
            }
            Err(v2_err) => {
                warn!(
                    peer_id = %peer,
                    error = %v2_err,
                    "file transfer v2 unavailable; falling back to v1"
                );
                let stream = control
                    .open_stream(peer, StreamProtocol::new(ProtocolId::FileTransfer.as_str()))
                    .await
                    .map_err(|err| {
                        anyhow!(
                            "failed to open file transfer stream via v2 ({v2_err}) or v1 ({err})"
                        )
                    })?;
                info!(
                    peer_id = %peer,
                    protocol_version = protocol_version_label(FileTransferProtocolVersion::V1),
                    "opened outbound file transfer stream"
                );
                Ok((stream, FileTransferProtocolVersion::V1))
            }
        }
    }

    async fn send_on_stream<W>(
        &self,
        version: FileTransferProtocolVersion,
        writer: &mut W,
        file_path: &std::path::Path,
        transfer_id: &str,
        batch_id: Option<String>,
        batch_total: Option<u32>,
        chunk_size: usize,
        progress_callback: Option<&(dyn Fn(u32, u32, u64) + Send + Sync)>,
    ) -> Result<String>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        match version {
            FileTransferProtocolVersion::V1 => {
                self.legacy
                    .send_file(
                        writer,
                        file_path,
                        transfer_id,
                        batch_id,
                        batch_total,
                        chunk_size,
                        progress_callback,
                    )
                    .await
            }
            FileTransferProtocolVersion::V2 => {
                self.streaming
                    .send_file(
                        writer,
                        file_path,
                        transfer_id,
                        batch_id,
                        batch_total,
                        chunk_size,
                        progress_callback,
                    )
                    .await
            }
        }
    }

    async fn receive_on_stream<S>(
        &self,
        stream: &mut S,
        incoming: &IncomingTransfer,
        cache_dir: &std::path::Path,
        progress_callback: Option<&(dyn Fn(u32, u32, u64) + Send + Sync)>,
    ) -> Result<PathBuf>
    where
        S: tokio::io::AsyncRead + Unpin,
    {
        match incoming {
            IncomingTransfer::V1(announce) => {
                self.legacy
                    .receive_file(stream, announce, cache_dir, progress_callback)
                    .await
            }
            IncomingTransfer::V2(announce) => {
                self.streaming
                    .receive_file(stream, announce, cache_dir, progress_callback)
                    .await
            }
        }
    }
}

/// Basic disk space check. Returns an error if insufficient space.
async fn check_disk_space(cache_dir: &std::path::Path, required: u64) -> Result<()> {
    // Ensure cache dir exists for the check
    tokio::fs::create_dir_all(cache_dir).await?;

    // Use statvfs on Unix for disk space check
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let path_str = cache_dir
            .to_str()
            .ok_or_else(|| anyhow!("cache_dir is not valid UTF-8"))?;
        let c_path = CString::new(path_str)?;

        let available = unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
                (stat.f_bavail as u64) * (stat.f_bsize as u64)
            } else {
                // If statvfs fails, skip check rather than block transfer
                return Ok(());
            }
        };

        let buffer = 10 * 1024 * 1024; // 10MB buffer
        if available < required + buffer {
            return Err(anyhow!(
                "Insufficient disk space: {} available, {} required (+ 10MB buffer)",
                available,
                required
            ));
        }
    }

    Ok(())
}

fn protocol_version_label(version: FileTransferProtocolVersion) -> &'static str {
    match version {
        FileTransferProtocolVersion::V1 => "v1",
        FileTransferProtocolVersion::V2 => "v2",
    }
}

fn maybe_log_progress(
    next_percent: &AtomicU32,
    direction: &str,
    transfer_id: &str,
    peer_id: &str,
    filename: &str,
    file_size: u64,
    bytes: u64,
    chunks_completed: u32,
    total_chunks: u32,
    started_at: Instant,
) {
    if bytes == 0 || file_size == 0 {
        return;
    }

    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let avg_mbps = average_mbps(bytes, elapsed_ms);
    let progress_pct = ((bytes.saturating_mul(100) / file_size).min(100)) as u32;
    let mut threshold = next_percent.load(Ordering::Relaxed);

    while progress_pct >= threshold && threshold <= 100 {
        let next_threshold = threshold.saturating_add(PROGRESS_LOG_STEP_PERCENT);
        match next_percent.compare_exchange(
            threshold,
            next_threshold,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                info!(
                    transfer_id = %transfer_id,
                    peer_id = %peer_id,
                    filename = %filename,
                    direction,
                    progress_pct,
                    bytes_transferred = bytes,
                    file_size,
                    chunks_completed,
                    total_chunks,
                    elapsed_ms,
                    avg_mbps,
                    "file transfer progress"
                );
                break;
            }
            Err(current) => threshold = current,
        }
    }
}

fn average_mbps(bytes: u64, elapsed_ms: u64) -> f64 {
    if elapsed_ms == 0 {
        return 0.0;
    }

    let bits = (bytes as f64) * 8.0;
    let seconds = (elapsed_ms as f64) / 1000.0;
    bits / seconds / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ports::transfer_progress::NoopTransferProgressPort;

    #[test]
    fn file_transfer_config_new() {
        let config = FileTransferConfig::new(PathBuf::from("/tmp/test-cache"));
        assert_eq!(config.chunk_size, CHUNK_SIZE);
        assert_eq!(config.transfer_timeout, Duration::from_secs(300));
        assert_eq!(config.cache_dir, PathBuf::from("/tmp/test-cache"));
    }

    #[test]
    fn concurrency_limits() {
        assert_eq!(MAX_FILE_TRANSFER_CONCURRENCY, 8);
        assert_eq!(PER_PEER_FILE_CONCURRENCY, 2);
    }

    #[test]
    fn progress_emit_gate_emits_first_and_final_updates() {
        let now = Instant::now();
        let mut gate = ProgressEmitGate::new(now);

        assert!(gate.should_emit(now, 1, 100, 1, 10));
        assert!(!gate.should_emit(now, 2, 100, 2, 10));
        assert!(gate.should_emit(now, 100, 100, 10, 10));
    }

    #[test]
    fn progress_emit_gate_emits_after_interval_or_large_advance() {
        let now = Instant::now();
        let mut gate = ProgressEmitGate::new(now);

        assert!(gate.should_emit(now, 1024, 10 * 1024 * 1024, 1, 20));

        let soon = now + Duration::from_millis(PROGRESS_EMIT_MIN_INTERVAL_MS - 1);
        assert!(!gate.should_emit(soon, 1024 * 1024, 10 * 1024 * 1024, 2, 20));

        let later = now + Duration::from_millis(PROGRESS_EMIT_MIN_INTERVAL_MS + 1);
        assert!(gate.should_emit(later, 1024 * 1024, 10 * 1024 * 1024, 3, 20));

        let immediate_large_advance = later + Duration::from_millis(1);
        assert!(gate.should_emit(
            immediate_large_advance,
            6 * 1024 * 1024,
            10 * 1024 * 1024,
            4,
            20
        ));
    }

    #[tokio::test]
    async fn acquire_permits_respects_limits() {
        let behaviour = stream::Behaviour::new();
        let control = behaviour.new_control();
        let (event_tx, _event_rx) = mpsc::channel(16);
        let progress_port = Arc::new(NoopTransferProgressPort);
        let config = FileTransferConfig::new(PathBuf::from("/tmp/test-file-cache"));

        let service = FileTransferService::new(control, event_tx, progress_port, config);

        // Acquire 2 per-peer permits (the limit)
        let permit1 = service.acquire_permits("peer-1").await;
        assert!(permit1.is_ok());
        let permit2 = service.acquire_permits("peer-1").await;
        assert!(permit2.is_ok());

        // Third should block (test with timeout)
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            service.acquire_permits("peer-1"),
        )
        .await;
        assert!(result.is_err(), "third permit should timeout (blocked)");

        // Different peer should work
        let permit_other = service.acquire_permits("peer-2").await;
        assert!(permit_other.is_ok());
    }
}
