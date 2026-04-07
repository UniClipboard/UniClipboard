use anyhow::anyhow;
use chrono::Utc;
use libp2p::futures::AsyncWriteExt;
use libp2p::{PeerId, StreamProtocol};
use libp2p_stream as stream;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::time::timeout;
use tracing::{debug, info, info_span, warn, Instrument, Span};
use uc_core::network::{DeviceAnnounceMessage, NetworkEvent, ProtocolDirection, ProtocolMessage};
use uc_core::ports::{ConnectionPolicyResolverPort, TransferDirection, TransferProgress};

use super::dial_strategy::{
    chosen_dial_addr_for_log, dial_decision_for_snapshot, infer_chosen_dial_addr_resolution,
    preferred_candidate_transport,
};
use super::discovery::{apply_peer_not_ready, apply_peer_ready};
use super::peer_cache::{snapshot_peer_addresses, PeerCaches};
use super::{
    check_business_allowed, try_send_event, BusinessCommand, BUSINESS_PROTOCOL_ID,
    BUSINESS_STREAM_CLOSE_TIMEOUT, BUSINESS_STREAM_OPEN_TIMEOUT, BUSINESS_STREAM_WRITE_TIMEOUT,
    NETWORK_CHUNK_SIZE,
};

use anyhow::Result;

pub(super) fn business_command_log_fields(
    command: &BusinessCommand,
) -> (&'static str, Option<&str>) {
    match command {
        BusinessCommand::SendClipboard { peer_id, .. } => ("clipboard", Some(peer_id.as_str())),
        BusinessCommand::EnsureBusinessPath { peer_id, .. } => ("ensure", Some(peer_id.as_str())),
        BusinessCommand::AnnounceDeviceName { .. } => ("announce_device_name", None),
        BusinessCommand::UnpairPeer { peer_id, .. } => ("unpair", Some(peer_id.as_str())),
    }
}

pub(super) fn notify_enqueue_failure(
    command: BusinessCommand,
    message: &str,
    operation: &str,
    peer_id: &str,
) {
    let result_tx = match command {
        BusinessCommand::SendClipboard { result_tx, .. } => result_tx,
        BusinessCommand::EnsureBusinessPath { result_tx, .. } => result_tx,
        BusinessCommand::UnpairPeer { result_tx, .. } => result_tx,
        BusinessCommand::AnnounceDeviceName { .. } => return,
    };

    if let Err(undelivered_result) = result_tx.send(Err(anyhow!(message.to_string()))) {
        warn!(
            op = operation,
            peer_id = %peer_id,
            result_ok = undelivered_result.is_ok(),
            "failed to deliver enqueue failure to caller"
        );
    }
}

pub(super) fn deliver_business_command_result(
    result_tx: oneshot::Sender<Result<()>>,
    result: Result<()>,
    command_id: u64,
    operation: &str,
    peer_id: &str,
) {
    if let Err(undelivered_result) = result_tx.send(result) {
        warn!(
            cmd_id = command_id,
            op = operation,
            peer_id = %peer_id,
            result_ok = undelivered_result.is_ok(),
            "business command result receiver dropped"
        );
    }
}

pub(super) async fn execute_business_command(
    command: BusinessCommand,
    command_id: u64,
    control: stream::Control,
    caches: Arc<RwLock<PeerCaches>>,
    policy_resolver: Arc<dyn ConnectionPolicyResolverPort>,
    event_tx: mpsc::Sender<NetworkEvent>,
    local_peer_id: String,
) {
    match command {
        BusinessCommand::SendClipboard {
            peer_id,
            data,
            result_tx,
        } => {
            let started_at = std::time::Instant::now();
            let peer_id_str = peer_id.as_str().to_string();
            debug!(
                cmd_id = command_id,
                op = "clipboard",
                peer_id = %peer_id_str,
                "business command started"
            );

            let result = match peer_id_str.parse::<PeerId>() {
                Ok(peer) => {
                    execute_business_stream(
                        &control,
                        &caches,
                        &policy_resolver,
                        &event_tx,
                        &peer_id,
                        peer,
                        Some(&*data),
                        BUSINESS_STREAM_OPEN_TIMEOUT,
                        BUSINESS_STREAM_WRITE_TIMEOUT,
                        BUSINESS_STREAM_CLOSE_TIMEOUT,
                        "clipboard",
                    )
                    .await
                }
                Err(err) => Err(anyhow!("invalid peer id for business stream: {err}")),
            };

            let elapsed_ms = started_at.elapsed().as_millis() as u64;
            match &result {
                Ok(()) => {
                    debug!(
                        cmd_id = command_id,
                        op = "clipboard",
                        peer_id = %peer_id_str,
                        elapsed_ms,
                        "business command completed"
                    );
                }
                Err(err) => {
                    warn!(
                        cmd_id = command_id,
                        op = "clipboard",
                        peer_id = %peer_id_str,
                        elapsed_ms,
                        error = %err,
                        "business command failed"
                    );
                }
            }

            deliver_business_command_result(
                result_tx,
                result,
                command_id,
                "clipboard",
                &peer_id_str,
            );
        }
        BusinessCommand::EnsureBusinessPath { peer_id, result_tx } => {
            let started_at = std::time::Instant::now();
            let peer_id_str = peer_id.as_str().to_string();
            debug!(
                cmd_id = command_id,
                op = "ensure",
                peer_id = %peer_id_str,
                "business command started"
            );

            let result = match peer_id_str.parse::<PeerId>() {
                Ok(peer) => {
                    execute_business_stream(
                        &control,
                        &caches,
                        &policy_resolver,
                        &event_tx,
                        &peer_id,
                        peer,
                        None,
                        BUSINESS_STREAM_OPEN_TIMEOUT,
                        BUSINESS_STREAM_WRITE_TIMEOUT,
                        BUSINESS_STREAM_CLOSE_TIMEOUT,
                        "ensure",
                    )
                    .await
                }
                Err(err) => Err(anyhow!("invalid peer id for ensure business path: {err}")),
            };

            let elapsed_ms = started_at.elapsed().as_millis() as u64;
            match &result {
                Ok(()) => {
                    debug!(
                        cmd_id = command_id,
                        op = "ensure",
                        peer_id = %peer_id_str,
                        elapsed_ms,
                        "business command completed"
                    );
                }
                Err(err) => {
                    warn!(
                        cmd_id = command_id,
                        op = "ensure",
                        peer_id = %peer_id_str,
                        elapsed_ms,
                        error = %err,
                        "business command failed"
                    );
                }
            }

            deliver_business_command_result(result_tx, result, command_id, "ensure", &peer_id_str);
        }
        BusinessCommand::AnnounceDeviceName { device_name } => {
            let started_at = std::time::Instant::now();
            debug!(
                cmd_id = command_id,
                op = "announce_device_name",
                "business command started"
            );

            let peer_ids = {
                let caches = caches.read().await;
                caches
                    .discovered_peers
                    .keys()
                    .filter(|peer_id| peer_id.as_str() != local_peer_id.as_str())
                    .cloned()
                    .collect::<Vec<_>>()
            };
            if peer_ids.is_empty() {
                info!(
                    cmd_id = command_id,
                    op = "announce_device_name",
                    local_peer_id = %local_peer_id,
                    "skip device announce because discovered peer list is empty"
                );
                return;
            }
            info!(
                cmd_id = command_id,
                op = "announce_device_name",
                target_peer_count = peer_ids.len(),
                local_peer_id = %local_peer_id,
                "broadcasting device announce to discovered peers"
            );
            let message = ProtocolMessage::DeviceAnnounce(DeviceAnnounceMessage {
                peer_id: local_peer_id.clone(),
                device_name,
                timestamp: Utc::now(),
            });
            let payload = match message.frame_to_bytes(None) {
                Ok(payload) => payload,
                Err(err) => {
                    warn!(
                        cmd_id = command_id,
                        op = "announce_device_name",
                        error = %err,
                        "failed to serialize device announce payload"
                    );
                    return;
                }
            };

            for peer_id in peer_ids {
                let peer_id_str = peer_id.as_str();
                let peer = match peer_id.as_str().parse::<PeerId>() {
                    Ok(peer) => peer,
                    Err(err) => {
                        warn!(
                            cmd_id = command_id,
                            op = "announce_device_name",
                            peer_id = %peer_id_str,
                            error = %err,
                            "invalid peer id for announce stream"
                        );
                        continue;
                    }
                };
                // DeviceAnnounce is allowed for all peers regardless of pairing
                // state so that device names are visible in the JoinPickDeviceStep
                // UI before pairing is initiated.

                let mut announce_control = control.clone();
                match timeout(
                    BUSINESS_STREAM_OPEN_TIMEOUT,
                    announce_control.open_stream(peer, StreamProtocol::new(BUSINESS_PROTOCOL_ID)),
                )
                .await
                {
                    Ok(Ok(mut stream)) => {
                        match timeout(BUSINESS_STREAM_WRITE_TIMEOUT, stream.write_all(&payload))
                            .await
                        {
                            Ok(Ok(())) => {
                                match timeout(BUSINESS_STREAM_CLOSE_TIMEOUT, stream.close()).await {
                                    Ok(Ok(())) => {}
                                    Ok(Err(err)) => {
                                        warn!(
                                            cmd_id = command_id,
                                            op = "announce_device_name",
                                            peer_id = %peer_id_str,
                                            error = %err,
                                            "announce stream close failed"
                                        );
                                    }
                                    Err(_) => {
                                        warn!(
                                            cmd_id = command_id,
                                            op = "announce_device_name",
                                            peer_id = %peer_id_str,
                                            "announce stream close timed out"
                                        );
                                    }
                                }
                            }
                            Ok(Err(err)) => {
                                warn!(
                                    cmd_id = command_id,
                                    op = "announce_device_name",
                                    peer_id = %peer_id_str,
                                    error = %err,
                                    "announce stream write failed"
                                );
                            }
                            Err(_) => {
                                warn!(
                                    cmd_id = command_id,
                                    op = "announce_device_name",
                                    peer_id = %peer_id_str,
                                    "announce stream write timed out"
                                );
                            }
                        }
                    }
                    Ok(Err(err)) => {
                        warn!(
                            cmd_id = command_id,
                            op = "announce_device_name",
                            peer_id = %peer_id_str,
                            error = %err,
                            "announce stream open failed"
                        );
                    }
                    Err(_) => {
                        warn!(
                            cmd_id = command_id,
                            op = "announce_device_name",
                            peer_id = %peer_id_str,
                            "announce stream open timed out"
                        );
                    }
                }
            }

            let elapsed_ms = started_at.elapsed().as_millis() as u64;
            debug!(
                cmd_id = command_id,
                op = "announce_device_name",
                elapsed_ms,
                "business command completed"
            );
        }
        BusinessCommand::UnpairPeer { peer_id, result_tx } => {
            let peer_id_str = peer_id.as_str().to_string();
            deliver_business_command_result(
                result_tx,
                Err(anyhow!("unpair command must be handled by swarm loop")),
                command_id,
                "unpair",
                &peer_id_str,
            );
        }
    }
}

pub(super) async fn execute_business_stream(
    control: &stream::Control,
    caches: &Arc<RwLock<PeerCaches>>,
    policy_resolver: &Arc<dyn ConnectionPolicyResolverPort>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    peer_id: &uc_core::PeerId,
    peer: PeerId,
    payload: Option<&[u8]>,
    open_timeout: Duration,
    write_timeout: Duration,
    close_timeout: Duration,
    denied_operation: &str,
) -> Result<()> {
    let peer_id_str = peer_id.as_str();
    let payload_bytes = payload.map(|data| data.len() as u64).unwrap_or(0);
    let span = info_span!(
        "business_stream.execute",
        operation = denied_operation,
        peer_id = %peer_id_str,
        payload_bytes,
        has_payload = payload.is_some(),
        dial_decision = tracing::field::Empty,
        peer_marked_reachable = tracing::field::Empty,
        candidate_address_count = tracing::field::Empty,
        preferred_candidate_transport = tracing::field::Empty,
    );

    async move {
        let attempt_started_at = Utc::now();

        if check_business_allowed(
            policy_resolver,
            event_tx,
            peer_id_str,
            ProtocolDirection::Outbound,
        )
        .await
        .is_err()
        {
            return Err(anyhow!(
                "business protocol denied for outbound {denied_operation} peer_id={peer_id_str}"
            ));
        }

        let (address_snapshot, registry_total, registry_candidate_count) = {
            let caches = caches.read().await;
            let snapshot = snapshot_peer_addresses(&caches, peer_id_str, attempt_started_at);
            let reg_total = caches.address_registry.all_for(peer_id_str).len();
            let reg_candidates = caches.address_registry.candidates_for(peer_id_str).len();
            (snapshot, reg_total, reg_candidates)
        };
        let dial_decision = dial_decision_for_snapshot(&address_snapshot);

        // Enforce address cooldown: if a new dial is required and the
        // registry has addresses but ALL of them are cooling down,
        // reject immediately instead of attempting a doomed dial.
        if dial_decision == "new_dial_required"
            && registry_total > 0
            && registry_candidate_count == 0
        {
            warn!(
                event = "business_stream.all_addresses_cooling_down",
                operation = denied_operation,
                peer_id = %peer_id_str,
                registry_total,
                "all addresses for peer are in cooldown, skipping dial"
            );
            return Err(anyhow!(
                "all addresses for peer {peer_id_str} are in cooldown"
            ));
        }
        let preferred_candidate_transport = preferred_candidate_transport(&address_snapshot);
        let span = Span::current();
        span.record("dial_decision", &dial_decision);
        span.record(
            "peer_marked_reachable",
            &address_snapshot.peer_marked_reachable,
        );
        span.record(
            "candidate_address_count",
            &(address_snapshot.candidate_addresses.len() as u64),
        );
        span.record(
            "preferred_candidate_transport",
            &preferred_candidate_transport,
        );
        info!(
            event = "business_stream.open_attempt",
            operation = denied_operation,
            peer_id = %peer_id_str,
            payload_bytes,
            dial_decision,
            peer_marked_reachable = address_snapshot.peer_marked_reachable,
            candidate_address_count = address_snapshot.candidate_addresses.len(),
            preferred_candidate_transport,
            connected_age_ms = ?address_snapshot.connected_age_ms,
            discovered_age_ms = ?address_snapshot.discovered_age_ms,
            last_seen_age_ms = ?address_snapshot.last_seen_age_ms,
            "attempting business stream open"
        );

        let mut control = control.clone();
        let result = match timeout(
            open_timeout,
            control.open_stream(peer, StreamProtocol::new(BUSINESS_PROTOCOL_ID)),
        )
        .await
        {
            Ok(Ok(mut stream)) => {
                if let Some(data) = payload {
                    // Write payload in NETWORK_CHUNK_SIZE chunks with progress tracking
                    let total = data.len() as u64;
                    let total_chunks =
                        ((data.len() + NETWORK_CHUNK_SIZE - 1) / NETWORK_CHUNK_SIZE) as u32;
                    let transfer_id = if data.len() >= 25 {
                        // Extract transfer_id from V3 header bytes [9..25] if payload is large enough
                        data[9..25]
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                    } else {
                        format!("outbound-{}", peer_id_str)
                    };

                    debug!(
                        peer_id = %peer_id_str,
                        transfer_id = %transfer_id,
                        total_bytes = total,
                        total_chunks,
                        chunk_size = NETWORK_CHUNK_SIZE,
                        "outbound chunked write started"
                    );

                    let write_result = timeout(write_timeout, async {
                        let mut written = 0u64;
                        let mut chunks_completed = 0u32;
                        let mut last_progress = std::time::Instant::now();

                        for chunk in data.chunks(NETWORK_CHUNK_SIZE) {
                            stream.write_all(chunk).await?;
                            written += chunk.len() as u64;
                            chunks_completed += 1;

                            debug!(
                                transfer_id = %transfer_id,
                                chunk = chunks_completed,
                                total_chunks,
                                chunk_bytes = chunk.len(),
                                bytes_written = written,
                                total_bytes = total,
                                "outbound chunk written"
                            );

                            // Throttle progress events: emit first, last, and at most every 100ms
                            if chunks_completed == 1
                                || chunks_completed == total_chunks
                                || last_progress.elapsed() >= Duration::from_millis(100)
                            {
                                let _ = try_send_event(
                                    &event_tx,
                                    NetworkEvent::TransferProgress(TransferProgress {
                                        transfer_id: transfer_id.clone(),
                                        peer_id: peer_id_str.to_string(),
                                        direction: TransferDirection::Sending,
                                        chunks_completed,
                                        total_chunks,
                                        bytes_transferred: written,
                                        total_bytes: Some(total),
                                    }),
                                    "TransferProgress",
                                );
                                last_progress = std::time::Instant::now();
                            }
                        }
                        stream.flush().await?;
                        debug!(
                            transfer_id = %transfer_id,
                            total_bytes = total,
                            total_chunks,
                            "outbound chunked write completed"
                        );
                        Ok::<(), std::io::Error>(())
                    })
                    .await;

                    match write_result {
                        Ok(Ok(())) => match timeout(close_timeout, stream.close()).await {
                            Ok(Ok(())) => Ok(()),
                            Ok(Err(err)) => {
                                warn!("business stream close failed: {err}");
                                Err(anyhow!("business stream close failed: {err}"))
                            }
                            Err(_) => {
                                warn!(peer_id = %peer_id_str, "business stream close timed out");
                                Err(anyhow!("business stream close timed out"))
                            }
                        },
                        Ok(Err(err)) => {
                            warn!("business stream write failed: {err}");
                            Err(anyhow!("business stream write failed: {err}"))
                        }
                        Err(_) => {
                            warn!(peer_id = %peer_id_str, "business stream write timed out");
                            Err(anyhow!("business stream write timed out"))
                        }
                    }
                } else {
                    match timeout(close_timeout, stream.close()).await {
                        Ok(Ok(())) => Ok(()),
                        Ok(Err(err)) => Err(anyhow!("ensure business stream close failed: {err}")),
                        Err(_) => {
                            warn!(peer_id = %peer_id_str, "ensure business stream close timed out");
                            Err(anyhow!("ensure business stream close timed out"))
                        }
                    }
                }
            }
            Ok(Err(err)) => {
                let failure_snapshot = {
                    let caches = caches.read().await;
                    snapshot_peer_addresses(&caches, peer_id_str, Utc::now())
                };
                let chosen_dial_addr =
                    chosen_dial_addr_for_log(&failure_snapshot, dial_decision, attempt_started_at);
                let chosen_dial_addr_resolution = infer_chosen_dial_addr_resolution(
                    &failure_snapshot,
                    dial_decision,
                    attempt_started_at,
                );
                if payload.is_some() {
                    warn!(
                        event = "business_stream.open_failed",
                        operation = denied_operation,
                        peer_id = %peer_id_str,
                        dial_decision,
                        candidate_address_count = failure_snapshot.candidate_addresses.len(),
                        candidate_addresses = ?failure_snapshot.candidate_addresses,
                        chosen_dial_addr = %chosen_dial_addr.unwrap_or("-"),
                        chosen_dial_addr_resolution,
                        dial_attempt_address_count = failure_snapshot.dial_attempt_address_count,
                        dial_attempt_addresses = ?failure_snapshot.dial_attempt_addresses,
                        last_dial_outcome = failure_snapshot.last_dial_outcome.unwrap_or("unknown"),
                        last_dial_age_ms = ?failure_snapshot.last_dial_age_ms,
                        error = %err,
                        "business stream open failed"
                    );
                    Err(anyhow!("business stream open failed: {err}"))
                } else {
                    warn!(
                        event = "business_stream.ensure_open_failed",
                        operation = denied_operation,
                        peer_id = %peer_id_str,
                        dial_decision,
                        candidate_address_count = failure_snapshot.candidate_addresses.len(),
                        candidate_addresses = ?failure_snapshot.candidate_addresses,
                        chosen_dial_addr = %chosen_dial_addr.unwrap_or("-"),
                        chosen_dial_addr_resolution,
                        dial_attempt_address_count = failure_snapshot.dial_attempt_address_count,
                        dial_attempt_addresses = ?failure_snapshot.dial_attempt_addresses,
                        last_dial_outcome = failure_snapshot.last_dial_outcome.unwrap_or("unknown"),
                        last_dial_age_ms = ?failure_snapshot.last_dial_age_ms,
                        error = %err,
                        "ensure business stream open failed"
                    );
                    Err(anyhow!("ensure business stream open failed: {err}"))
                }
            }
            Err(_) => {
                let timeout_snapshot = {
                    let caches = caches.read().await;
                    snapshot_peer_addresses(&caches, peer_id_str, Utc::now())
                };
                let chosen_dial_addr =
                    chosen_dial_addr_for_log(&timeout_snapshot, dial_decision, attempt_started_at);
                let chosen_dial_addr_resolution = infer_chosen_dial_addr_resolution(
                    &timeout_snapshot,
                    dial_decision,
                    attempt_started_at,
                );
                if payload.is_some() {
                    warn!(
                        event = "business_stream.open_timeout",
                        operation = denied_operation,
                        peer_id = %peer_id_str,
                        dial_decision,
                        candidate_address_count = timeout_snapshot.candidate_addresses.len(),
                        candidate_addresses = ?timeout_snapshot.candidate_addresses,
                        chosen_dial_addr = %chosen_dial_addr.unwrap_or("-"),
                        chosen_dial_addr_resolution,
                        dial_attempt_address_count = timeout_snapshot.dial_attempt_address_count,
                        dial_attempt_addresses = ?timeout_snapshot.dial_attempt_addresses,
                        last_dial_outcome = timeout_snapshot.last_dial_outcome.unwrap_or("unknown"),
                        last_dial_age_ms = ?timeout_snapshot.last_dial_age_ms,
                        timeout_ms = open_timeout.as_millis() as u64,
                        "business stream open timed out"
                    );
                    Err(anyhow!("business stream open timed out"))
                } else {
                    warn!(
                        event = "business_stream.ensure_open_timeout",
                        operation = denied_operation,
                        peer_id = %peer_id_str,
                        dial_decision,
                        candidate_address_count = timeout_snapshot.candidate_addresses.len(),
                        candidate_addresses = ?timeout_snapshot.candidate_addresses,
                        chosen_dial_addr = %chosen_dial_addr.unwrap_or("-"),
                        chosen_dial_addr_resolution,
                        dial_attempt_address_count = timeout_snapshot.dial_attempt_address_count,
                        dial_attempt_addresses = ?timeout_snapshot.dial_attempt_addresses,
                        last_dial_outcome = timeout_snapshot.last_dial_outcome.unwrap_or("unknown"),
                        last_dial_age_ms = ?timeout_snapshot.last_dial_age_ms,
                        timeout_ms = open_timeout.as_millis() as u64,
                        "ensure business stream open timed out"
                    );
                    Err(anyhow!("ensure business stream open timed out"))
                }
            }
        };

        apply_business_stream_result(caches, event_tx, peer_id_str, &result).await;
        result
    }
    .instrument(span)
    .await
}

/// Update peer reachability state in `PeerCaches` and emit a corresponding `NetworkEvent`
/// reflecting whether a business stream completed successfully.
///
/// This function marks the peer as ready when `result` is `Ok(())` or not-ready when
/// `result` is `Err(..)`, then attempts to send the resulting `NetworkEvent` on
/// `event_tx`. Address-level success/failure is recorded by connection-layer events
/// (e.g., `ConnectionEstablished` / `OutgoingConnectionError`), not by this function.
///
/// # Examples
///
/// ```ignore
/// apply_business_stream_result(&caches, &tx, "peer-id", &Ok(())).await;
/// ```
pub(super) async fn apply_business_stream_result(
    caches: &Arc<RwLock<PeerCaches>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    peer_id: &str,
    result: &Result<()>,
) {
    // Note: address-level success/failure is recorded at the connection layer
    // (ConnectionEstablished / OutgoingConnectionError), not here, because
    // business stream results don't carry the specific dialled address.
    let event = {
        let mut caches = caches.write().await;
        if result.is_ok() {
            apply_peer_ready(&mut caches, peer_id, Utc::now())
        } else {
            apply_peer_not_ready(&mut caches, peer_id)
        }
    };
    if let Some(event) = event {
        let label = if result.is_ok() {
            "PeerReady"
        } else {
            "PeerNotReady"
        };
        let _ = try_send_event(event_tx, event, label);
    }
}
