//! File transfer service for chunked file transfers over libp2p streams.
//!
//! Follows PairingStreamService patterns with Arc<Inner>, semaphore-based
//! concurrency control, and async stream handling.

use super::framing::{read_file_frame, write_file_frame, FileMessageType};
use super::protocol::{
    receive_file_chunked, send_file_chunked, FileAcceptance, FileAnnounce, CHUNK_SIZE,
};
use anyhow::{anyhow, Result};
use libp2p::{futures::StreamExt, PeerId, StreamProtocol};
use libp2p_stream as stream;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex as AsyncMutex, OwnedSemaphorePermit, Semaphore};
use tokio::time::Duration;
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
}

struct TransferPermits {
    _global: OwnedSemaphorePermit,
    _peer: OwnedSemaphorePermit,
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
            }),
        }
    }

    /// Spawn the accept loop for incoming file transfers.
    pub fn spawn_accept_loop(&self) {
        let service = self.clone();
        tokio::spawn(async move {
            service.run_accept_loop().await;
        });
    }

    async fn run_accept_loop(&self) {
        let mut incoming = {
            let mut control = self.inner.control.lock().await;
            match control.accept(StreamProtocol::new(ProtocolId::FileTransfer.as_str())) {
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
            let span = info_span!("file_transfer.incoming", peer_id = %span_peer_id);
            tokio::spawn(
                async move {
                    if let Err(err) = service
                        .handle_incoming_transfer(peer_id.clone(), stream)
                        .await
                    {
                        warn!(
                            peer_id = %peer_id,
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
    async fn handle_incoming_transfer<S>(&self, peer_id: String, mut stream: S) -> Result<()>
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

        let announce: FileAnnounce = serde_json::from_slice(&frame.1)
            .map_err(|e| anyhow!("invalid announce message: {}", e))?;

        info!(
            transfer_id = %announce.transfer_id,
            filename = %announce.filename,
            file_size = announce.file_size,
            "incoming file transfer"
        );

        // Emit start event
        let _ = self
            .inner
            .event_tx
            .send(NetworkEvent::FileTransferStarted {
                transfer_id: announce.transfer_id.clone(),
                peer_id: peer_id.clone(),
                filename: announce.filename.clone(),
                file_size: announce.file_size,
            })
            .await;

        // Check disk space (basic check)
        let cache_dir = &self.inner.config.cache_dir;
        if let Err(space_err) = check_disk_space(cache_dir, announce.file_size).await {
            let rejection = FileAcceptance {
                transfer_id: announce.transfer_id.clone(),
                accepted: false,
                reason: Some(space_err.to_string()),
            };
            let rejection_bytes = serde_json::to_vec(&rejection)?;
            write_file_frame(&mut stream, FileMessageType::Reject, &rejection_bytes).await?;

            let _ = self
                .inner
                .event_tx
                .send(NetworkEvent::FileTransferFailed {
                    transfer_id: announce.transfer_id.clone(),
                    peer_id: peer_id.clone(),
                    error: space_err.to_string(),
                })
                .await;
            return Err(space_err);
        }

        // Send acceptance
        let acceptance = FileAcceptance {
            transfer_id: announce.transfer_id.clone(),
            accepted: true,
            reason: None,
        };
        let acceptance_bytes = serde_json::to_vec(&acceptance)?;
        write_file_frame(&mut stream, FileMessageType::Accept, &acceptance_bytes).await?;

        // Receive the file
        let progress_port = self.inner.progress_port.clone();
        let peer_id_clone = peer_id.clone();
        let transfer_id_clone = announce.transfer_id.clone();

        let progress_callback = move |chunks_completed: u32, total_chunks: u32, bytes: u64| {
            let progress = TransferProgress {
                transfer_id: transfer_id_clone.clone(),
                peer_id: peer_id_clone.clone(),
                direction: TransferDirection::Receiving,
                chunks_completed,
                total_chunks,
                bytes_transferred: bytes,
                total_bytes: Some(announce.file_size),
            };
            let port = progress_port.clone();
            tokio::spawn(async move {
                let _ = port.report_progress(progress).await;
            });
        };

        let result =
            receive_file_chunked(&mut stream, &announce, cache_dir, Some(&progress_callback)).await;

        // Hold permits until transfer completes
        drop(permits);

        match result {
            Ok(final_path) => {
                info!(
                    transfer_id = %announce.transfer_id,
                    path = %final_path.display(),
                    "file transfer complete"
                );
                let _ = self
                    .inner
                    .event_tx
                    .send(NetworkEvent::FileTransferCompleted {
                        transfer_id: announce.transfer_id.clone(),
                        peer_id: peer_id.clone(),
                        filename: announce.filename.clone(),
                        file_path: final_path,
                        batch_id: announce.batch_id.clone(),
                        batch_total: announce.batch_total,
                    })
                    .await;
                Ok(())
            }
            Err(e) => {
                let _ = self
                    .inner
                    .event_tx
                    .send(NetworkEvent::FileTransferFailed {
                        transfer_id: announce.transfer_id.clone(),
                        peer_id: peer_id.clone(),
                        error: e.to_string(),
                    })
                    .await;
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
        let stream = {
            let mut control = self.inner.control.lock().await;
            control
                .open_stream(peer, StreamProtocol::new(ProtocolId::FileTransfer.as_str()))
                .await
                .map_err(|err| anyhow!("failed to open file transfer stream: {err}"))?
        };
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
        let progress_callback = move |chunks_completed: u32, total_chunks: u32, bytes: u64| {
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
                let _ = port.report_progress(progress).await;
            });
        };

        // Send the file
        let send_result = send_file_chunked(
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
                        info!(transfer_id = %transfer_id, "file transfer accepted and sent");
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
