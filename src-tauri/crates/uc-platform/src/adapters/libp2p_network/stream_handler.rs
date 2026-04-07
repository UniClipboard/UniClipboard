use anyhow::anyhow;
use libp2p::futures::{AsyncReadExt, StreamExt};
use libp2p::StreamProtocol;
use libp2p_stream as stream;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, warn};
use uc_core::network::protocol::ClipboardPayloadVersion;
use uc_core::network::{ClipboardMessage, NetworkEvent, ProtocolDirection, ProtocolMessage};
use uc_core::ports::{
    ConnectionPolicyResolverPort, EncryptionSessionPort, TransferDirection,
    TransferPayloadDecryptorPort, TransferProgress,
};

use super::peer_cache::PeerCaches;
use super::{
    check_business_allowed, try_send_event, BUSINESS_PAYLOAD_MAX_BYTES, BUSINESS_PROTOCOL_ID,
    BUSINESS_READ_TIMEOUT, MAX_CHUNK_CIPHERTEXT_SIZE, MAX_JSON_HEADER_SIZE,
};

use super::ProcessedMessage;

pub(super) fn spawn_business_stream_handler(
    mut control: stream::Control,
    caches: Arc<RwLock<PeerCaches>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    clipboard_tx: mpsc::Sender<(ClipboardMessage, Option<Vec<u8>>)>,
    policy_resolver: Arc<dyn ConnectionPolicyResolverPort>,
    encryption_session: Arc<dyn EncryptionSessionPort>,
    transfer_decryptor: Arc<dyn TransferPayloadDecryptorPort>,
) {
    let mut incoming = match control.accept(StreamProtocol::new(BUSINESS_PROTOCOL_ID)) {
        Ok(incoming) => incoming,
        Err(err) => {
            warn!("failed to accept business stream: {err}");
            return;
        }
    };

    tokio::spawn(async move {
        while let Some((_peer, stream)) = incoming.next().await {
            let peer_id = _peer.to_string();
            let event_tx = event_tx.clone();
            let clipboard_tx = clipboard_tx.clone();
            let policy_resolver = policy_resolver.clone();
            let caches = caches.clone();
            let encryption_session = encryption_session.clone();
            let transfer_decryptor = transfer_decryptor.clone();
            tokio::spawn(async move {
                // Policy check is deferred until after reading the message type.
                // DeviceAnnounce is allowed from any peer (even unpaired) so that
                // device names are available in JoinPickDeviceStep before pairing.

                // Apply overall size guard on the stream
                let limited = AsyncReadExt::take(stream, BUSINESS_PAYLOAD_MAX_BYTES + 1);

                // Convert libp2p stream (futures::AsyncRead) to tokio AsyncRead
                use tokio_util::compat::FuturesAsyncReadCompatExt;
                let mut reader = limited.compat();

                let result: Result<Result<ProcessedMessage, String>, _> = tokio::time::timeout(BUSINESS_READ_TIMEOUT, async {
                    use tokio::io::AsyncReadExt as TokioAsyncReadExt;

                    // Step 1: Read 4-byte JSON header length (u32 LE)
                    // An immediate EOF here means the peer opened the stream as a
                    // connectivity probe (ensure_business_path) and closed it without
                    // sending data — not an error.
                    let mut len_buf = [0u8; 4];
                    match TokioAsyncReadExt::read_exact(&mut reader, &mut len_buf).await {
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                            return Err("probe".into());
                        }
                        Err(e) => {
                            return Err(format!("failed to read json header length: {e}"));
                        }
                    }
                    let json_len = u32::from_le_bytes(len_buf) as usize;

                    // Guard: cap JSON header size at 64KB
                    if json_len > MAX_JSON_HEADER_SIZE {
                        return Err(format!(
                            "json header too large: {json_len} > {MAX_JSON_HEADER_SIZE}"
                        ));
                    }

                    // Step 2: Read JSON header (exactly json_len bytes)
                    let mut json_buf = vec![0u8; json_len];
                    TokioAsyncReadExt::read_exact(&mut reader, &mut json_buf)
                        .await
                        .map_err(|e| format!("failed to read json header: {e}"))?;

                    let message = ProtocolMessage::from_bytes(&json_buf)
                        .map_err(|e| format!("invalid protocol message: {e}"))?;

                    match message {
                        ProtocolMessage::Clipboard(msg)
                            if msg.payload_version == ClipboardPayloadVersion::V3
                                && msg.encrypted_content.is_empty() =>
                        {
                            // Gate unpaired/pending peers before expensive I/O and crypto.
                            if check_business_allowed(
                                &policy_resolver,
                                &event_tx,
                                &peer_id,
                                ProtocolDirection::Inbound,
                            )
                            .await
                            .is_err()
                            {
                                return Err("denied by policy".into());
                            }

                            // Streaming decode uses a blocking read-to-end, then async decrypt
                            // via injected TransferPayloadDecryptorPort.
                            let master_key = match encryption_session.get_master_key().await {
                                Ok(k) => k,
                                Err(e) => {
                                    return Err(format!(
                                        "inbound: encryption session not ready: {e}"
                                    ));
                                }
                            };

                            // Clone event_tx for progress reporting inside spawn_blocking
                            let progress_event_tx = event_tx.clone();
                            let inbound_peer_id_str = peer_id.clone();
                            let encrypted = tokio::task::spawn_blocking(move || {
                                use std::io::Read;
                                use tokio_util::io::SyncIoBridge;
                                let mut sync_reader = SyncIoBridge::new(reader);

                                // Read V3 header (37 bytes) to extract total_chunks and transfer_id
                                let mut header = [0u8; 37];
                                sync_reader
                                    .read_exact(&mut header)
                                    .map_err(|e| anyhow!("stream read failed (header): {e}"))?;

                                let total_chunks = u32::from_le_bytes(
                                    header[25..29]
                                        .try_into()
                                        .map_err(|_| anyhow!("invalid header: total_chunks"))?,
                                );
                                let transfer_id = header[9..25]
                                    .iter()
                                    .map(|b| format!("{b:02x}"))
                                    .collect::<String>();

                                debug!(
                                    peer_id = %inbound_peer_id_str,
                                    transfer_id = %transfer_id,
                                    total_chunks,
                                    "inbound chunked read started"
                                );

                                // Accumulate: header + per-chunk (4-byte len prefix + ciphertext)
                                let mut buf = Vec::from(&header[..]);
                                let mut bytes_received = 37u64;
                                let mut last_progress = std::time::Instant::now();

                                for chunk_idx in 0..total_chunks {
                                    // Read 4-byte chunk ciphertext length
                                    let mut len_buf = [0u8; 4];
                                    sync_reader.read_exact(&mut len_buf).map_err(|e| {
                                        anyhow!("stream read failed (chunk {} len): {e}", chunk_idx)
                                    })?;
                                    let ct_len = u32::from_le_bytes(len_buf) as usize;
                                    if ct_len > MAX_CHUNK_CIPHERTEXT_SIZE {
                                        return Err(anyhow!(
                                            "chunk {} ciphertext length {} exceeds maximum allowed size {}",
                                            chunk_idx,
                                            ct_len,
                                            MAX_CHUNK_CIPHERTEXT_SIZE
                                        ));
                                    }
                                    buf.extend_from_slice(&len_buf);

                                    // Read chunk ciphertext
                                    let mut ct_buf = vec![0u8; ct_len];
                                    sync_reader.read_exact(&mut ct_buf).map_err(|e| {
                                        anyhow!(
                                            "stream read failed (chunk {} data): {e}",
                                            chunk_idx
                                        )
                                    })?;
                                    buf.extend_from_slice(&ct_buf);
                                    bytes_received += 4 + ct_len as u64;

                                    let chunks_completed = chunk_idx + 1;

                                    debug!(
                                        transfer_id = %transfer_id,
                                        chunk = chunks_completed,
                                        total_chunks,
                                        ct_len,
                                        bytes_received,
                                        "inbound chunk read"
                                    );

                                    // Throttle progress: first, last, and at most every 100ms
                                    if chunks_completed == 1
                                        || chunks_completed == total_chunks
                                        || last_progress.elapsed()
                                            >= std::time::Duration::from_millis(100)
                                    {
                                        let _ = try_send_event(
                                            &progress_event_tx,
                                            NetworkEvent::TransferProgress(TransferProgress {
                                                transfer_id: transfer_id.clone(),
                                                peer_id: inbound_peer_id_str.clone(),
                                                direction: TransferDirection::Receiving,
                                                chunks_completed,
                                                total_chunks,
                                                bytes_transferred: bytes_received,
                                                total_bytes: None, // unknown until fully read
                                            }),
                                            "TransferProgress",
                                        );
                                        last_progress = std::time::Instant::now();
                                    }
                                }

                                debug!(
                                    transfer_id = %transfer_id,
                                    total_chunks,
                                    total_bytes_received = bytes_received,
                                    "inbound chunked read completed"
                                );

                                Ok::<Vec<u8>, anyhow::Error>(buf)
                            })
                            .await
                            .map_err(|e| format!("buffer task panicked: {e}"))?
                            .map_err(|e| format!("inbound: stream read failed: {e}"))?;

                            let plaintext = transfer_decryptor
                                .decrypt(&encrypted, &master_key)
                                .map_err(|e| format!("inbound: chunk decrypt failed: {e}"))?;
                            Ok(ProcessedMessage::StreamingClipboard(msg, plaintext))
                        }
                        other => {
                            // DeviceAnnounce, Heartbeat, Pairing — no trailing payload
                            Ok(ProcessedMessage::Standard(other))
                        }
                    }
                })
                .await;

                // Stream ownership: for streaming clipboard the stream is moved into
                // spawn_blocking via SyncIoBridge; when buffering finishes (or errors),
                // SyncIoBridge is dropped, which drops the underlying tokio reader / compat
                // layer / Take<libp2p::Stream>. The libp2p stream close happens via Drop.
                // For non-clipboard messages, the reader is dropped when the async block completes.

                match result {
                    Ok(Ok(ProcessedMessage::StreamingClipboard(msg, plaintext))) => {
                        // Policy already checked inside the streaming branch before
                        // get_master_key / spawn_blocking / decrypt.
                        handle_v2_clipboard(
                            caches,
                            event_tx,
                            clipboard_tx,
                            peer_id,
                            msg,
                            plaintext,
                        )
                        .await;
                    }
                    Ok(Ok(ProcessedMessage::Standard(ProtocolMessage::DeviceAnnounce(
                        announce,
                    )))) => {
                        // DeviceAnnounce is allowed from any peer (even unpaired)
                        handle_standard_message(
                            caches,
                            event_tx,
                            clipboard_tx,
                            peer_id,
                            ProtocolMessage::DeviceAnnounce(announce),
                        )
                        .await;
                    }
                    Ok(Ok(ProcessedMessage::Standard(message))) => {
                        // All other standard messages require pairing
                        if check_business_allowed(
                            &policy_resolver,
                            &event_tx,
                            &peer_id,
                            ProtocolDirection::Inbound,
                        )
                        .await
                        .is_err()
                        {
                            return;
                        }
                        handle_standard_message(caches, event_tx, clipboard_tx, peer_id, message)
                            .await;
                    }
                    Ok(Err(err)) if err == "probe" => {
                        debug!(peer_id = %peer_id, "business stream probe (ensure_business_path)");
                    }
                    Ok(Err(err)) => {
                        warn!(peer_id = %peer_id, error = %err, "business stream processing failed");
                    }
                    Err(_) => {
                        warn!(peer_id = %peer_id, "business stream read timed out");
                    }
                }
            });
        }
    });
}

/// Handle non-streaming protocol messages (DeviceAnnounce, Heartbeat, Pairing, fallback clipboard).
pub(super) async fn handle_standard_message(
    caches: Arc<RwLock<PeerCaches>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    clipboard_tx: mpsc::Sender<(ClipboardMessage, Option<Vec<u8>>)>,
    peer_id: String,
    message: ProtocolMessage,
) {
    match message {
        ProtocolMessage::DeviceAnnounce(announce) => {
            if announce.peer_id != peer_id {
                warn!(
                    "Device announce peer_id mismatch: peer_id={}, announced_peer_id={}",
                    peer_id, announce.peer_id
                );
            }
            let changed = {
                let mut caches = caches.write().await;
                caches.upsert_device_name(
                    peer_id.as_str(),
                    announce.device_name.clone(),
                    announce.timestamp,
                )
            };
            if changed {
                if let Err(err) = try_send_event(
                    &event_tx,
                    NetworkEvent::PeerNameUpdated {
                        peer_id: peer_id.clone(),
                        device_name: announce.device_name,
                    },
                    "PeerNameUpdated",
                ) {
                    warn!("failed to send PeerNameUpdated event: {err}");
                }
            }
        }
        ProtocolMessage::Clipboard(message) => {
            // Fallback path — send with None for pre-decoded plaintext
            if let Err(err) = clipboard_tx.send((message.clone(), None)).await {
                warn!("Failed to forward clipboard payload: {err}");
            }
            if let Err(err) = try_send_event(
                &event_tx,
                NetworkEvent::ClipboardReceived(message),
                "ClipboardReceived",
            ) {
                warn!("failed to send ClipboardReceived event: {err}");
            }
        }
        ProtocolMessage::Heartbeat(_) => {
            debug!("Received heartbeat payload from peer_id={}", peer_id);
        }
        ProtocolMessage::Pairing(_) => {
            warn!(
                "Unexpected pairing payload on business stream from peer_id={}",
                peer_id
            );
        }
    }
}

/// Handle clipboard message with pre-decoded plaintext from transport-level streaming decode.
pub(super) async fn handle_v2_clipboard(
    _caches: Arc<RwLock<PeerCaches>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    clipboard_tx: mpsc::Sender<(ClipboardMessage, Option<Vec<u8>>)>,
    _peer_id: String,
    message: ClipboardMessage,
    plaintext: Vec<u8>,
) {
    if let Err(err) = clipboard_tx.send((message.clone(), Some(plaintext))).await {
        warn!("Failed to forward clipboard payload: {err}");
    }
    if let Err(err) = try_send_event(
        &event_tx,
        NetworkEvent::ClipboardReceived(message),
        "ClipboardReceived",
    ) {
        warn!("failed to send ClipboardReceived event: {err}");
    }
}

pub(super) async fn emit_protocol_denied(
    event_tx: &mpsc::Sender<NetworkEvent>,
    peer_id: String,
    protocol_id: &str,
    pairing_state: uc_core::network::PairingState,
    direction: ProtocolDirection,
    reason: uc_core::network::ProtocolDenyReason,
) {
    if let Err(err) = event_tx
        .send(NetworkEvent::ProtocolDenied {
            peer_id,
            protocol_id: protocol_id.to_string(),
            pairing_state,
            direction,
            reason,
        })
        .await
    {
        warn!("failed to emit protocol denied event: {err}");
    }
}

pub(super) async fn handle_pairing_open_error(
    policy_resolver: &Arc<dyn ConnectionPolicyResolverPort>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    peer_id: &str,
    error: &anyhow::Error,
) {
    use super::super::pairing_stream::service::PairingStreamError;
    use uc_core::network::{PairingState, ProtocolDenyReason, ProtocolId};

    if let Some(pairing_error) = error.downcast_ref::<PairingStreamError>() {
        if matches!(pairing_error, PairingStreamError::UnsupportedProtocol) {
            let peer = uc_core::PeerId::from(peer_id);
            let pairing_state = match policy_resolver.resolve_for_peer(&peer).await {
                Ok(resolved) => resolved.pairing_state,
                Err(err) => {
                    warn!("policy resolver failed for pairing protocol peer={peer_id}: {err}");
                    PairingState::Pending
                }
            };
            emit_protocol_denied(
                event_tx,
                peer_id.to_string(),
                ProtocolId::Pairing.as_str(),
                pairing_state,
                ProtocolDirection::Outbound,
                ProtocolDenyReason::NotSupported,
            )
            .await;
        }
    }
}
