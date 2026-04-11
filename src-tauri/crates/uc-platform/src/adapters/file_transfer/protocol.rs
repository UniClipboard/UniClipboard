//! Chunked file transfer protocol.
//!
//! Handles the announce/accept/chunk/complete message flow for file transfers
//! with incremental Blake3 hash computation and verification.

use super::framing::{read_file_frame, write_file_frame, FileMessageType};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time::Instant;
use tracing::{debug, info, info_span, instrument, warn, Instrument};

/// Default chunk size: 1MB.
pub const CHUNK_SIZE: usize = 1024 * 1024;
const STREAMING_CHUNK_HEADER_BYTES: usize = 8;

/// File transfer announcement sent by the sender.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAnnounce {
    pub transfer_id: String,
    pub filename: String,
    pub file_size: u64,
    pub chunk_size: u32,
    pub blake3_hash: String,
    pub batch_id: Option<String>,
    pub batch_total: Option<u32>,
}

/// Streaming file transfer announcement for protocol v2.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAnnounceV2 {
    pub transfer_id: String,
    pub filename: String,
    pub file_size: u64,
    pub chunk_size: u32,
    pub batch_id: Option<String>,
    pub batch_total: Option<u32>,
}

/// Acceptance or rejection response from the receiver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAcceptance {
    pub transfer_id: String,
    pub accepted: bool,
    pub reason: Option<String>,
}

/// Header prepended to each data chunk (serialized as JSON within a Chunk frame).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunkHeader {
    pub chunk_index: u32,
    pub chunk_size: u32,
}

/// Completion message with hash for verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileComplete {
    pub transfer_id: String,
    pub blake3_hash: String,
    pub total_chunks: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LegacyFileTransferProtocol;

#[derive(Debug, Clone, Copy, Default)]
pub struct StreamingFileTransferProtocol;

#[derive(Debug, Clone, Copy, Default)]
struct ChunkTransferEngine;

#[derive(Debug, Clone, Copy)]
enum ChunkEncoding {
    LegacyJsonHeader,
    StreamingBinaryHeader,
}

struct ReceivedHash {
    computed_hash: String,
    completed_hash: String,
}

/// Compute Blake3 hash of a file.
async fn compute_blake3_hash(file_path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(file_path).await?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0u8; CHUNK_SIZE];

    loop {
        let bytes_read = file.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

impl LegacyFileTransferProtocol {
    pub async fn send_file<W>(
        &self,
        writer: &mut W,
        file_path: &Path,
        transfer_id: &str,
        batch_id: Option<String>,
        batch_total: Option<u32>,
        chunk_size: usize,
        progress_callback: Option<&(dyn Fn(u32, u32, u64) + Send + Sync)>,
    ) -> Result<String>
    where
        W: AsyncWrite + Unpin,
    {
        let hash = compute_blake3_hash(file_path).await?;
        let (mut file, file_size, filename) = open_file_transfer_source(file_path).await?;

        let announce = FileAnnounce {
            transfer_id: transfer_id.to_string(),
            filename,
            file_size,
            chunk_size: chunk_size as u32,
            blake3_hash: hash.clone(),
            batch_id,
            batch_total,
        };
        let announce_bytes = serde_json::to_vec(&announce)?;
        write_file_frame(writer, FileMessageType::Announce, &announce_bytes).await?;

        let streamed_hash = ChunkTransferEngine::send_stream(
            writer,
            &mut file,
            &announce.transfer_id,
            announce.file_size,
            chunk_size,
            ChunkEncoding::LegacyJsonHeader,
            progress_callback,
        )
        .await?;

        debug_assert_eq!(streamed_hash, hash);
        Ok(streamed_hash)
    }

    pub async fn receive_file<R>(
        &self,
        reader: &mut R,
        announce: &FileAnnounce,
        cache_dir: &Path,
        progress_callback: Option<&(dyn Fn(u32, u32, u64) + Send + Sync)>,
    ) -> Result<PathBuf>
    where
        R: AsyncRead + Unpin,
    {
        ChunkTransferEngine::receive_file(
            reader,
            &announce.transfer_id,
            &announce.filename,
            announce.file_size,
            announce.chunk_size,
            Some(&announce.blake3_hash),
            cache_dir,
            ChunkEncoding::LegacyJsonHeader,
            progress_callback,
        )
        .await
    }
}

impl StreamingFileTransferProtocol {
    pub async fn send_file<W>(
        &self,
        writer: &mut W,
        file_path: &Path,
        transfer_id: &str,
        batch_id: Option<String>,
        batch_total: Option<u32>,
        chunk_size: usize,
        progress_callback: Option<&(dyn Fn(u32, u32, u64) + Send + Sync)>,
    ) -> Result<String>
    where
        W: AsyncWrite + Unpin,
    {
        let (mut file, file_size, filename) = open_file_transfer_source(file_path).await?;

        let announce = FileAnnounceV2 {
            transfer_id: transfer_id.to_string(),
            filename,
            file_size,
            chunk_size: chunk_size as u32,
            batch_id,
            batch_total,
        };
        let announce_bytes = serde_json::to_vec(&announce)?;
        write_file_frame(writer, FileMessageType::Announce, &announce_bytes).await?;

        ChunkTransferEngine::send_stream(
            writer,
            &mut file,
            &announce.transfer_id,
            announce.file_size,
            chunk_size,
            ChunkEncoding::StreamingBinaryHeader,
            progress_callback,
        )
        .await
    }

    pub async fn receive_file<R>(
        &self,
        reader: &mut R,
        announce: &FileAnnounceV2,
        cache_dir: &Path,
        progress_callback: Option<&(dyn Fn(u32, u32, u64) + Send + Sync)>,
    ) -> Result<PathBuf>
    where
        R: AsyncRead + Unpin,
    {
        ChunkTransferEngine::receive_file(
            reader,
            &announce.transfer_id,
            &announce.filename,
            announce.file_size,
            announce.chunk_size,
            None,
            cache_dir,
            ChunkEncoding::StreamingBinaryHeader,
            progress_callback,
        )
        .await
    }
}

impl ChunkTransferEngine {
    #[instrument(
        name = "file_transfer.send_stream",
        level = "info",
        skip(writer, reader, progress_callback),
        fields(
            transfer_id = %transfer_id,
            file_size,
            chunk_size
        )
    )]
    async fn send_stream<W, R>(
        writer: &mut W,
        reader: &mut R,
        transfer_id: &str,
        file_size: u64,
        chunk_size: usize,
        chunk_encoding: ChunkEncoding,
        progress_callback: Option<&(dyn Fn(u32, u32, u64) + Send + Sync)>,
    ) -> Result<String>
    where
        W: AsyncWrite + Unpin,
        R: AsyncRead + Unpin,
    {
        let total_chunks = if file_size == 0 {
            0
        } else {
            file_size.div_ceil(chunk_size as u64) as u32
        };
        let mut bytes_sent: u64 = 0;
        let mut chunk_index: u32 = 0;
        let mut hasher = blake3::Hasher::new();
        let mut buffer = vec![0u8; chunk_size];
        let started_at = Instant::now();
        let mut file_read_elapsed_ns: u128 = 0;
        let mut network_write_elapsed_ns: u128 = 0;
        let mut complete_frame_elapsed_ns: u128 = 0;
        let mut flush_elapsed_ns: u128 = 0;

        loop {
            let read_started_at = Instant::now();
            let bytes_read = reader.read(&mut buffer).await?;
            file_read_elapsed_ns += read_started_at.elapsed().as_nanos();
            if bytes_read == 0 {
                break;
            }

            let chunk_data = &buffer[..bytes_read];
            hasher.update(chunk_data);
            let write_started_at = Instant::now();
            Self::write_chunk_frame(
                writer,
                chunk_encoding,
                chunk_index,
                bytes_read as u32,
                chunk_data,
            )
            .await?;
            network_write_elapsed_ns += write_started_at.elapsed().as_nanos();

            bytes_sent += bytes_read as u64;
            chunk_index += 1;
            if let Some(cb) = progress_callback {
                cb(chunk_index, total_chunks.max(chunk_index), bytes_sent);
            }
        }

        let hash = hasher.finalize().to_hex().to_string();
        let complete = FileComplete {
            transfer_id: transfer_id.to_string(),
            blake3_hash: hash.clone(),
            total_chunks: chunk_index,
        };
        let complete_bytes = serde_json::to_vec(&complete)?;
        let complete_started_at = Instant::now();
        write_file_frame(writer, FileMessageType::Complete, &complete_bytes).await?;
        complete_frame_elapsed_ns += complete_started_at.elapsed().as_nanos();
        let flush_started_at = Instant::now();
        writer.flush().await?;
        flush_elapsed_ns += flush_started_at.elapsed().as_nanos();

        let elapsed_ms = started_at.elapsed().as_millis() as u64;
        info!(
            transfer_id = %transfer_id,
            file_size,
            total_chunks = chunk_index,
            chunk_size,
            elapsed_ms,
            file_read_elapsed_ms = nanos_to_ms(file_read_elapsed_ns),
            network_write_elapsed_ms = nanos_to_ms(network_write_elapsed_ns),
            complete_frame_elapsed_ms = nanos_to_ms(complete_frame_elapsed_ns),
            flush_elapsed_ms = nanos_to_ms(flush_elapsed_ns),
            avg_mbps = average_mbps(bytes_sent, elapsed_ms),
            "file transfer stream sent"
        );

        Ok(hash)
    }

    async fn receive_file<R>(
        reader: &mut R,
        transfer_id: &str,
        filename: &str,
        file_size: u64,
        announced_chunk_size: u32,
        announce_hash: Option<&str>,
        cache_dir: &Path,
        chunk_encoding: ChunkEncoding,
        progress_callback: Option<&(dyn Fn(u32, u32, u64) + Send + Sync)>,
    ) -> Result<PathBuf>
    where
        R: AsyncRead + Unpin,
    {
        let transfer_dir = cache_dir.join(transfer_id);
        tokio::fs::create_dir_all(&transfer_dir).await?;
        let tmp_path = transfer_dir.join(format!("{}.tmp", transfer_id));

        let result = Self::receive_chunks_to_file(
            reader,
            &tmp_path,
            transfer_id,
            file_size,
            announced_chunk_size,
            chunk_encoding,
            progress_callback,
        )
        .instrument(info_span!("receive_chunks", transfer_id = %transfer_id))
        .await;

        match result {
            Ok(received_hash) => {
                if let Some(expected_hash) = announce_hash {
                    if received_hash.completed_hash != expected_hash {
                        let _ = tokio::fs::remove_file(&tmp_path).await;
                        return Err(anyhow!(
                            "blake3 hash mismatch: expected {}, got {}",
                            expected_hash,
                            received_hash.completed_hash
                        ));
                    }
                }

                if received_hash.computed_hash != received_hash.completed_hash {
                    let _ = tokio::fs::remove_file(&tmp_path).await;
                    return Err(anyhow!(
                        "blake3 hash mismatch: expected {}, got {}",
                        received_hash.completed_hash,
                        received_hash.computed_hash
                    ));
                }

                let safe_filename = sanitize_filename(filename);
                let final_path = transfer_dir.join(&safe_filename);
                tokio::fs::rename(&tmp_path, &final_path).await?;
                Ok(final_path)
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                Err(e)
            }
        }
    }

    #[instrument(
        name = "file_transfer.receive_chunks",
        level = "info",
        skip(reader, progress_callback),
        fields(
            transfer_id = %transfer_id,
            file_size,
            tmp_path = %tmp_path.display()
        )
    )]
    async fn receive_chunks_to_file<R>(
        reader: &mut R,
        tmp_path: &Path,
        transfer_id: &str,
        file_size: u64,
        announced_chunk_size: u32,
        chunk_encoding: ChunkEncoding,
        progress_callback: Option<&(dyn Fn(u32, u32, u64) + Send + Sync)>,
    ) -> Result<ReceivedHash>
    where
        R: AsyncRead + Unpin,
    {
        let mut hasher = blake3::Hasher::new();
        let mut file = tokio::fs::File::create(tmp_path).await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            tokio::fs::set_permissions(tmp_path, perms).await?;
        }

        let mut bytes_received: u64 = 0;
        let mut chunks_received: u32 = 0;
        let started_at = Instant::now();
        let mut frame_read_elapsed_ns: u128 = 0;
        let mut file_write_elapsed_ns: u128 = 0;
        let mut finalize_sync_elapsed_ns: u128 = 0;

        loop {
            let read_started_at = Instant::now();
            let frame = read_file_frame(reader).await?;
            frame_read_elapsed_ns += read_started_at.elapsed().as_nanos();
            let (msg_type, payload) = match frame {
                Some(f) => f,
                None => return Err(anyhow!("stream closed before transfer complete")),
            };

            match msg_type {
                FileMessageType::Chunk => {
                    let (_chunk_index, chunk_data) =
                        Self::decode_chunk_payload(chunk_encoding, &payload)?;

                    hasher.update(chunk_data);
                    let write_started_at = Instant::now();
                    file.write_all(chunk_data).await?;
                    file_write_elapsed_ns += write_started_at.elapsed().as_nanos();
                    bytes_received += chunk_data.len() as u64;
                    chunks_received += 1;

                    let estimated_total = if file_size > 0 && announced_chunk_size > 0 {
                        file_size.div_ceil(announced_chunk_size as u64) as u32
                    } else {
                        chunks_received
                    };

                    if let Some(cb) = progress_callback {
                        cb(chunks_received, estimated_total, bytes_received);
                    }
                }
                FileMessageType::Complete => {
                    let complete: FileComplete = serde_json::from_slice(&payload)?;
                    let finalize_started_at = Instant::now();
                    file.flush().await?;
                    file.sync_data().await?;
                    finalize_sync_elapsed_ns += finalize_started_at.elapsed().as_nanos();

                    let elapsed_ms = started_at.elapsed().as_millis() as u64;
                    let computed_hash = hasher.finalize().to_hex().to_string();
                    info!(
                        transfer_id = %transfer_id,
                        file_size,
                        bytes_received,
                        total_chunks = complete.total_chunks,
                        chunks_received,
                        elapsed_ms,
                        frame_read_elapsed_ms = nanos_to_ms(frame_read_elapsed_ns),
                        file_write_elapsed_ms = nanos_to_ms(file_write_elapsed_ns),
                        finalize_sync_elapsed_ms = nanos_to_ms(finalize_sync_elapsed_ns),
                        avg_mbps = average_mbps(bytes_received, elapsed_ms),
                        "file transfer stream received"
                    );
                    debug!(
                        transfer_id = %transfer_id,
                        total_chunks = complete.total_chunks,
                        bytes = bytes_received,
                        "file receive complete"
                    );
                    return Ok(ReceivedHash {
                        computed_hash,
                        completed_hash: complete.blake3_hash,
                    });
                }
                other => {
                    warn!("unexpected message type during transfer: {:?}", other);
                    return Err(anyhow!("unexpected message type: {:?}", other));
                }
            }
        }
    }

    async fn write_chunk_frame<W>(
        writer: &mut W,
        chunk_encoding: ChunkEncoding,
        chunk_index: u32,
        chunk_size: u32,
        chunk_data: &[u8],
    ) -> Result<()>
    where
        W: AsyncWrite + Unpin,
    {
        match chunk_encoding {
            ChunkEncoding::LegacyJsonHeader => {
                let header = FileChunkHeader {
                    chunk_index,
                    chunk_size,
                };
                let header_bytes = serde_json::to_vec(&header)?;
                let mut payload = Vec::with_capacity(4 + header_bytes.len() + chunk_data.len());
                payload.extend_from_slice(&(header_bytes.len() as u32).to_be_bytes());
                payload.extend_from_slice(&header_bytes);
                payload.extend_from_slice(chunk_data);
                write_file_frame(writer, FileMessageType::Chunk, &payload).await
            }
            ChunkEncoding::StreamingBinaryHeader => {
                let payload_len = STREAMING_CHUNK_HEADER_BYTES + chunk_data.len();
                if payload_len > super::framing::MAX_FILE_FRAME_BYTES {
                    return Err(anyhow!(
                        "file frame payload too large: {} > {}",
                        payload_len,
                        super::framing::MAX_FILE_FRAME_BYTES
                    ));
                }

                writer.write_all(&[FileMessageType::Chunk as u8]).await?;
                writer
                    .write_all(&(payload_len as u32).to_be_bytes())
                    .await?;
                writer.write_all(&chunk_index.to_be_bytes()).await?;
                writer.write_all(&chunk_size.to_be_bytes()).await?;
                writer.write_all(chunk_data).await?;
                Ok(())
            }
        }
    }

    fn decode_chunk_payload<'a>(
        chunk_encoding: ChunkEncoding,
        payload: &'a [u8],
    ) -> Result<(u32, &'a [u8])> {
        match chunk_encoding {
            ChunkEncoding::LegacyJsonHeader => {
                if payload.len() < 4 {
                    return Err(anyhow!("chunk payload too small"));
                }
                let header_len =
                    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
                if payload.len() < 4 + header_len {
                    return Err(anyhow!("chunk payload missing header data"));
                }
                let header: FileChunkHeader = serde_json::from_slice(&payload[4..4 + header_len])?;
                let chunk_data = &payload[4 + header_len..];
                if chunk_data.len() != header.chunk_size as usize {
                    return Err(anyhow!(
                        "chunk payload size mismatch: expected {}, got {}",
                        header.chunk_size,
                        chunk_data.len()
                    ));
                }
                Ok((header.chunk_index, chunk_data))
            }
            ChunkEncoding::StreamingBinaryHeader => {
                if payload.len() < STREAMING_CHUNK_HEADER_BYTES {
                    return Err(anyhow!("streaming chunk payload too small"));
                }
                let chunk_index =
                    u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let chunk_size =
                    u32::from_be_bytes([payload[4], payload[5], payload[6], payload[7]]) as usize;
                let chunk_data = &payload[STREAMING_CHUNK_HEADER_BYTES..];
                if chunk_data.len() != chunk_size {
                    return Err(anyhow!(
                        "streaming chunk payload size mismatch: expected {}, got {}",
                        chunk_size,
                        chunk_data.len()
                    ));
                }
                Ok((chunk_index, chunk_data))
            }
        }
    }
}

async fn open_file_transfer_source(file_path: &Path) -> Result<(tokio::fs::File, u64, String)> {
    let file = tokio::fs::File::open(file_path)
        .await
        .map_err(|e| anyhow!("failed to open file for transfer: {}", e))?;
    let file_size = file
        .metadata()
        .await
        .map_err(|e| anyhow!("failed to read file metadata for transfer: {}", e))?
        .len();
    let filename = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();
    Ok((file, file_size, filename))
}

/// Sanitize a filename to prevent path traversal.
fn sanitize_filename(name: &str) -> String {
    name.replace("..", "_")
        .replace('/', "_")
        .replace('\\', "_")
        .replace('\0', "_")
}

fn average_mbps(bytes: u64, elapsed_ms: u64) -> f64 {
    if elapsed_ms == 0 {
        return 0.0;
    }

    let bits = (bytes as f64) * 8.0;
    let seconds = (elapsed_ms as f64) / 1000.0;
    bits / seconds / 1_000_000.0
}

fn nanos_to_ms(elapsed_ns: u128) -> u64 {
    (elapsed_ns / 1_000_000) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::task::{Context, Poll};
    use tokio::io::AsyncWrite;

    const LEGACY_PROTOCOL: LegacyFileTransferProtocol = LegacyFileTransferProtocol;
    const STREAMING_PROTOCOL: StreamingFileTransferProtocol = StreamingFileTransferProtocol;

    #[derive(Default)]
    struct CountingWriter {
        inner: Vec<u8>,
        flush_count: Arc<AtomicUsize>,
    }

    fn legacy_chunk_payload(chunk_index: u32, chunk_data: &[u8]) -> Vec<u8> {
        let header = FileChunkHeader {
            chunk_index,
            chunk_size: chunk_data.len() as u32,
        };
        let header_bytes = serde_json::to_vec(&header).unwrap();
        let mut payload = Vec::new();
        payload.extend_from_slice(&(header_bytes.len() as u32).to_be_bytes());
        payload.extend_from_slice(&header_bytes);
        payload.extend_from_slice(chunk_data);
        payload
    }

    fn streaming_chunk_payload(chunk_index: u32, chunk_data: &[u8]) -> Vec<u8> {
        let mut payload = Vec::with_capacity(STREAMING_CHUNK_HEADER_BYTES + chunk_data.len());
        payload.extend_from_slice(&chunk_index.to_be_bytes());
        payload.extend_from_slice(&(chunk_data.len() as u32).to_be_bytes());
        payload.extend_from_slice(chunk_data);
        payload
    }

    impl CountingWriter {
        fn new() -> (Self, Arc<AtomicUsize>) {
            let flush_count = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    inner: Vec::new(),
                    flush_count: flush_count.clone(),
                },
                flush_count,
            )
        }
    }

    impl AsyncWrite for CountingWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.inner.extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            self.flush_count.fetch_add(1, Ordering::SeqCst);
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[test]
    fn file_announce_roundtrip() {
        let announce = FileAnnounce {
            transfer_id: "xfer-1".to_string(),
            filename: "test.txt".to_string(),
            file_size: 1024,
            chunk_size: 256,
            blake3_hash: "abc123".to_string(),
            batch_id: Some("batch-1".to_string()),
            batch_total: Some(3),
        };
        let json = serde_json::to_string(&announce).unwrap();
        let restored: FileAnnounce = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.transfer_id, "xfer-1");
        assert_eq!(restored.filename, "test.txt");
        assert_eq!(restored.file_size, 1024);
        assert_eq!(restored.chunk_size, 256);
        assert_eq!(restored.batch_id, Some("batch-1".to_string()));
    }

    #[test]
    fn file_announce_v2_roundtrip() {
        let announce = FileAnnounceV2 {
            transfer_id: "xfer-2".to_string(),
            filename: "stream.txt".to_string(),
            file_size: 2048,
            chunk_size: 512,
            batch_id: Some("batch-2".to_string()),
            batch_total: Some(4),
        };
        let json = serde_json::to_string(&announce).unwrap();
        let restored: FileAnnounceV2 = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.transfer_id, "xfer-2");
        assert_eq!(restored.filename, "stream.txt");
        assert_eq!(restored.file_size, 2048);
        assert_eq!(restored.chunk_size, 512);
    }

    #[test]
    fn file_chunk_header_roundtrip() {
        let header = FileChunkHeader {
            chunk_index: 5,
            chunk_size: 262144,
        };
        let json = serde_json::to_string(&header).unwrap();
        let restored: FileChunkHeader = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.chunk_index, 5);
        assert_eq!(restored.chunk_size, 262144);
    }

    #[test]
    fn file_acceptance_roundtrip() {
        let acceptance = FileAcceptance {
            transfer_id: "xfer-1".to_string(),
            accepted: true,
            reason: None,
        };
        let json = serde_json::to_string(&acceptance).unwrap();
        let restored: FileAcceptance = serde_json::from_str(&json).unwrap();
        assert!(restored.accepted);
        assert!(restored.reason.is_none());

        let rejection = FileAcceptance {
            transfer_id: "xfer-2".to_string(),
            accepted: false,
            reason: Some("Insufficient disk space".to_string()),
        };
        let json = serde_json::to_string(&rejection).unwrap();
        let restored: FileAcceptance = serde_json::from_str(&json).unwrap();
        assert!(!restored.accepted);
        assert_eq!(restored.reason.unwrap(), "Insufficient disk space");
    }

    #[test]
    fn blake3_hash_deterministic() {
        let data = b"hello world";
        let hash1 = blake3::hash(data).to_hex().to_string();
        let hash2 = blake3::hash(data).to_hex().to_string();
        assert_eq!(hash1, hash2);
        assert!(!hash1.is_empty());
    }

    #[test]
    fn sanitize_filename_removes_traversal() {
        assert_eq!(sanitize_filename("../etc/passwd"), "__etc_passwd");
        assert_eq!(sanitize_filename("file..name.txt"), "file_name.txt");
        assert_eq!(sanitize_filename("normal.txt"), "normal.txt");
        assert_eq!(sanitize_filename("path/to\\file"), "path_to_file");
    }

    #[tokio::test]
    async fn chunked_send_receive_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_path = temp_dir.path().join("source.txt");
        let cache_dir = temp_dir.path().join("cache");
        let test_data = b"Hello, this is test data for chunked transfer!";
        tokio::fs::write(&source_path, test_data).await.unwrap();

        let (client, server) = tokio::io::duplex(64 * 1024);
        let (_client_read, mut client_write) = tokio::io::split(client);
        let (mut server_read, _server_write) = tokio::io::split(server);

        let source_path_clone = source_path.clone();
        let send_handle = tokio::spawn(async move {
            LEGACY_PROTOCOL
                .send_file(
                    &mut client_write,
                    &source_path_clone,
                    "test-xfer",
                    None,
                    None,
                    16,
                    None,
                )
                .await
        });

        let cache_dir_clone = cache_dir.clone();
        let recv_handle = tokio::spawn(async move {
            let frame = read_file_frame(&mut server_read).await.unwrap().unwrap();
            assert_eq!(frame.0, FileMessageType::Announce);
            let announce: FileAnnounce = serde_json::from_slice(&frame.1).unwrap();
            assert_eq!(announce.transfer_id, "test-xfer");
            assert_eq!(announce.filename, "source.txt");
            assert_eq!(announce.chunk_size, 16);
            LEGACY_PROTOCOL
                .receive_file(&mut server_read, &announce, &cache_dir_clone, None)
                .await
        });

        let send_hash = send_handle.await.unwrap().unwrap();
        let final_path = recv_handle.await.unwrap().unwrap();
        let received_data = tokio::fs::read(&final_path).await.unwrap();
        assert_eq!(received_data, test_data);
        assert_eq!(
            final_path.file_name().unwrap().to_str().unwrap(),
            "source.txt"
        );
        assert_eq!(
            final_path
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            "test-xfer"
        );
        assert!(!send_hash.is_empty());
    }

    #[tokio::test]
    async fn sender_starts_streaming_before_input_reader_finishes() {
        let announce = FileAnnounceV2 {
            transfer_id: "streaming-xfer".to_string(),
            filename: "stream.bin".to_string(),
            file_size: 8,
            chunk_size: 4,
            batch_id: None,
            batch_total: None,
        };

        let (mut input_writer, mut input_reader) = tokio::io::duplex(64);
        let (mut writer, mut receiver) = tokio::io::duplex(64 * 1024);
        let send_task = tokio::spawn(async move {
            let announce_bytes = serde_json::to_vec(&announce).unwrap();
            write_file_frame(&mut writer, FileMessageType::Announce, &announce_bytes)
                .await
                .unwrap();
            ChunkTransferEngine::send_stream(
                &mut writer,
                &mut input_reader,
                &announce.transfer_id,
                announce.file_size,
                4,
                ChunkEncoding::StreamingBinaryHeader,
                None,
            )
            .await
        });

        input_writer.write_all(b"abcd").await.unwrap();
        input_writer.flush().await.unwrap();

        let announce_frame = read_file_frame(&mut receiver).await.unwrap().unwrap();
        assert_eq!(announce_frame.0, FileMessageType::Announce);

        let first_chunk_frame = read_file_frame(&mut receiver).await.unwrap().unwrap();
        assert_eq!(first_chunk_frame.0, FileMessageType::Chunk);
        input_writer.write_all(b"efgh").await.unwrap();
        input_writer.shutdown().await.unwrap();

        let second_chunk_frame = read_file_frame(&mut receiver).await.unwrap().unwrap();
        assert_eq!(second_chunk_frame.0, FileMessageType::Chunk);
        let done_frame = read_file_frame(&mut receiver).await.unwrap().unwrap();
        assert_eq!(done_frame.0, FileMessageType::Complete);

        let hash = send_task.await.unwrap().unwrap();
        assert_eq!(hash, blake3::hash(b"abcdefgh").to_hex().to_string());
    }

    #[tokio::test]
    async fn send_stream_flushes_once_after_all_chunks() {
        let (mut writer, flush_count) = CountingWriter::new();
        let mut reader = std::io::Cursor::new(vec![1u8; CHUNK_SIZE * 2 + 17]);

        let hash = ChunkTransferEngine::send_stream(
            &mut writer,
            &mut reader,
            "flush-once-xfer",
            (CHUNK_SIZE * 2 + 17) as u64,
            CHUNK_SIZE,
            ChunkEncoding::LegacyJsonHeader,
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            flush_count.load(Ordering::SeqCst),
            1,
            "chunk streaming should not flush per chunk"
        );
        assert_eq!(
            hash,
            blake3::hash(&vec![1u8; CHUNK_SIZE * 2 + 17])
                .to_hex()
                .to_string()
        );
    }

    #[tokio::test]
    async fn receiver_progress_uses_announced_chunk_size() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        tokio::fs::create_dir_all(&cache_dir).await.unwrap();

        let test_data = b"abcdefghij";
        let announce = FileAnnounce {
            transfer_id: "progress-xfer".to_string(),
            filename: "progress.bin".to_string(),
            file_size: test_data.len() as u64,
            chunk_size: 4,
            blake3_hash: blake3::hash(test_data).to_hex().to_string(),
            batch_id: None,
            batch_total: None,
        };

        let mut stream_data = Vec::new();
        for (index, chunk) in test_data.chunks(announce.chunk_size as usize).enumerate() {
            let chunk_payload = legacy_chunk_payload(index as u32, chunk);
            write_file_frame(&mut stream_data, FileMessageType::Chunk, &chunk_payload)
                .await
                .unwrap();
        }

        let complete = FileComplete {
            transfer_id: announce.transfer_id.clone(),
            blake3_hash: announce.blake3_hash.clone(),
            total_chunks: 3,
        };
        let complete_bytes = serde_json::to_vec(&complete).unwrap();
        write_file_frame(&mut stream_data, FileMessageType::Complete, &complete_bytes)
            .await
            .unwrap();

        let progress_events = std::sync::Mutex::new(Vec::new());
        let progress_callback = |completed: u32, total: u32, bytes: u64| {
            progress_events
                .lock()
                .unwrap()
                .push((completed, total, bytes));
        };

        let mut cursor = &stream_data[..];
        let final_path = LEGACY_PROTOCOL
            .receive_file(&mut cursor, &announce, &cache_dir, Some(&progress_callback))
            .await
            .unwrap();

        let captured = progress_events.lock().unwrap();
        assert_eq!(captured.first().copied(), Some((1, 3, 4)));
        assert_eq!(captured.last().copied(), Some((3, 3, 10)));
        assert_eq!(tokio::fs::read(&final_path).await.unwrap(), test_data);
    }

    #[tokio::test]
    async fn hash_mismatch_deletes_temp_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        tokio::fs::create_dir_all(&cache_dir).await.unwrap();

        let announce = FileAnnounce {
            transfer_id: "bad-hash-xfer".to_string(),
            filename: "test.txt".to_string(),
            file_size: 5,
            chunk_size: 5,
            blake3_hash: "definitely_wrong_hash".to_string(),
            batch_id: None,
            batch_total: None,
        };

        let mut stream_data = Vec::new();
        let chunk_data = b"hello";
        let chunk_payload = legacy_chunk_payload(0, chunk_data);
        write_file_frame(&mut stream_data, FileMessageType::Chunk, &chunk_payload)
            .await
            .unwrap();

        let complete = FileComplete {
            transfer_id: "bad-hash-xfer".to_string(),
            blake3_hash: "definitely_wrong_hash".to_string(),
            total_chunks: 1,
        };
        let complete_bytes = serde_json::to_vec(&complete).unwrap();
        write_file_frame(&mut stream_data, FileMessageType::Complete, &complete_bytes)
            .await
            .unwrap();

        let mut cursor = &stream_data[..];
        let result = LEGACY_PROTOCOL
            .receive_file(&mut cursor, &announce, &cache_dir, None)
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("hash mismatch"));
        let tmp_path = cache_dir.join("bad-hash-xfer").join("bad-hash-xfer.tmp");
        assert!(!tmp_path.exists());
    }

    #[tokio::test]
    async fn atomic_rename_on_success() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        let test_data = b"success data";
        let hash = blake3::hash(test_data).to_hex().to_string();

        let announce = FileAnnounce {
            transfer_id: "rename-xfer".to_string(),
            filename: "result.dat".to_string(),
            file_size: test_data.len() as u64,
            chunk_size: test_data.len() as u32,
            blake3_hash: hash.clone(),
            batch_id: None,
            batch_total: None,
        };

        let mut stream_data = Vec::new();
        let chunk_payload = legacy_chunk_payload(0, test_data);
        write_file_frame(&mut stream_data, FileMessageType::Chunk, &chunk_payload)
            .await
            .unwrap();

        let complete = FileComplete {
            transfer_id: "rename-xfer".to_string(),
            blake3_hash: hash,
            total_chunks: 1,
        };
        let complete_bytes = serde_json::to_vec(&complete).unwrap();
        write_file_frame(&mut stream_data, FileMessageType::Complete, &complete_bytes)
            .await
            .unwrap();

        let mut cursor = &stream_data[..];
        let final_path = LEGACY_PROTOCOL
            .receive_file(&mut cursor, &announce, &cache_dir, None)
            .await
            .unwrap();

        let tmp_path = cache_dir.join("rename-xfer").join("rename-xfer.tmp");
        assert!(!tmp_path.exists());
        assert!(final_path.exists());
        assert_eq!(
            final_path.file_name().unwrap().to_str().unwrap(),
            "result.dat"
        );
        assert_eq!(
            final_path
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_str()
                .unwrap(),
            "rename-xfer"
        );
        let contents = tokio::fs::read(&final_path).await.unwrap();
        assert_eq!(contents, test_data);
    }

    #[tokio::test]
    async fn receiver_writes_chunks_to_temp_file_before_complete() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        let announce = FileAnnounceV2 {
            transfer_id: "streaming-recv".to_string(),
            filename: "video.bin".to_string(),
            file_size: 8,
            chunk_size: 4,
            batch_id: None,
            batch_total: None,
        };

        let (mut writer, mut reader) = tokio::io::duplex(64 * 1024);
        let cache_dir_clone = cache_dir.clone();
        let announce_clone = announce.clone();
        let receive_task = tokio::spawn(async move {
            STREAMING_PROTOCOL
                .receive_file(&mut reader, &announce_clone, &cache_dir_clone, None)
                .await
        });

        let first_chunk = b"abcd";
        let first_payload = streaming_chunk_payload(0, first_chunk);
        write_file_frame(&mut writer, FileMessageType::Chunk, &first_payload)
            .await
            .unwrap();

        let tmp_path = cache_dir.join("streaming-recv").join("streaming-recv.tmp");
        for _ in 0..10 {
            if let Ok(metadata) = tokio::fs::metadata(&tmp_path).await {
                if metadata.len() == first_chunk.len() as u64 {
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        let tmp_metadata = tokio::fs::metadata(&tmp_path)
            .await
            .expect("temp file should be created before complete");
        assert_eq!(tmp_metadata.len(), first_chunk.len() as u64);

        let second_chunk = b"efgh";
        let second_payload = streaming_chunk_payload(1, second_chunk);
        write_file_frame(&mut writer, FileMessageType::Chunk, &second_payload)
            .await
            .unwrap();

        let complete = FileComplete {
            transfer_id: "streaming-recv".to_string(),
            blake3_hash: blake3::hash(b"abcdefgh").to_hex().to_string(),
            total_chunks: 2,
        };
        let complete_bytes = serde_json::to_vec(&complete).unwrap();
        write_file_frame(&mut writer, FileMessageType::Complete, &complete_bytes)
            .await
            .unwrap();

        let final_path = receive_task.await.unwrap().unwrap();
        let final_bytes = tokio::fs::read(final_path).await.unwrap();
        assert_eq!(final_bytes, b"abcdefgh");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_permissions_on_received_file() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().unwrap();
        let cache_dir = temp_dir.path().join("cache");
        let test_data = b"permission test data";
        let hash = blake3::hash(test_data).to_hex().to_string();

        let announce = FileAnnounce {
            transfer_id: "perm-xfer".to_string(),
            filename: "secret.dat".to_string(),
            file_size: test_data.len() as u64,
            chunk_size: test_data.len() as u32,
            blake3_hash: hash.clone(),
            batch_id: None,
            batch_total: None,
        };

        let mut stream_data = Vec::new();
        let chunk_payload = legacy_chunk_payload(0, test_data);
        write_file_frame(&mut stream_data, FileMessageType::Chunk, &chunk_payload)
            .await
            .unwrap();

        let complete = FileComplete {
            transfer_id: "perm-xfer".to_string(),
            blake3_hash: hash,
            total_chunks: 1,
        };
        let complete_bytes = serde_json::to_vec(&complete).unwrap();
        write_file_frame(&mut stream_data, FileMessageType::Complete, &complete_bytes)
            .await
            .unwrap();

        let mut cursor = &stream_data[..];
        let final_path = LEGACY_PROTOCOL
            .receive_file(&mut cursor, &announce, &cache_dir, None)
            .await
            .unwrap();

        let metadata = tokio::fs::metadata(&final_path).await.unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "file should have 0600 permissions");
    }

    #[tokio::test]
    async fn test_large_file_multi_chunk_roundtrip() {
        let temp_dir = tempfile::tempdir().unwrap();
        let source_path = temp_dir.path().join("large_source.bin");
        let cache_dir = temp_dir.path().join("cache");
        let file_size: usize = 1_048_576;
        let test_data: Vec<u8> = (0..file_size).map(|i| (i % 256) as u8).collect();
        tokio::fs::write(&source_path, &test_data).await.unwrap();

        let expected_hash = blake3::hash(&test_data).to_hex().to_string();
        let (client, server) = tokio::io::duplex(CHUNK_SIZE + 4096);
        let (_client_read, mut client_write) = tokio::io::split(client);
        let (mut server_read, _server_write) = tokio::io::split(server);

        let source_path_clone = source_path.clone();
        let send_handle = tokio::spawn(async move {
            STREAMING_PROTOCOL
                .send_file(
                    &mut client_write,
                    &source_path_clone,
                    "large-xfer",
                    Some("batch-large".to_string()),
                    Some(1),
                    CHUNK_SIZE,
                    None,
                )
                .await
        });

        let cache_dir_clone = cache_dir.clone();
        let recv_handle = tokio::spawn(async move {
            let frame = read_file_frame(&mut server_read).await.unwrap().unwrap();
            assert_eq!(frame.0, FileMessageType::Announce);
            let announce: FileAnnounceV2 = serde_json::from_slice(&frame.1).unwrap();
            assert_eq!(announce.transfer_id, "large-xfer");
            assert_eq!(announce.file_size, 1_048_576);
            assert_eq!(announce.filename, "large_source.bin");
            assert_eq!(announce.chunk_size, CHUNK_SIZE as u32);
            STREAMING_PROTOCOL
                .receive_file(&mut server_read, &announce, &cache_dir_clone, None)
                .await
        });

        let send_hash = send_handle.await.unwrap().unwrap();
        let final_path = recv_handle.await.unwrap().unwrap();
        let received_data = tokio::fs::read(&final_path).await.unwrap();
        assert_eq!(received_data.len(), file_size);
        assert_eq!(received_data, test_data);
        let received_hash = blake3::hash(&received_data).to_hex().to_string();
        assert_eq!(send_hash, expected_hash);
        assert_eq!(received_hash, expected_hash);
        assert!(!send_hash.is_empty());
    }
}
