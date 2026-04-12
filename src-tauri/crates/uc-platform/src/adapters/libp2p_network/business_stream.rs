//! Single business stream execution — opens a libp2p stream to a peer,
//! writes an optional payload with chunked progress tracking, and updates
//! peer reachability state based on the outcome.

use anyhow::{anyhow, Result};
use chrono::Utc;
use libp2p::futures::AsyncWriteExt;
use libp2p::{PeerId, StreamProtocol};
use libp2p_stream as stream;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, info, info_span, warn, Instrument, Span};
use uc_core::network::{NetworkEvent, ProtocolDirection};
use uc_core::ports::{ConnectionPolicyResolverPort, TransferDirection, TransferProgress};

use super::dial_strategy::{
    chosen_dial_addr_for_log, dial_decision_for_snapshot, infer_address_scope,
    infer_chosen_dial_addr_resolution, preferred_candidate_transport, TRANSPORT_PENALTY,
};
use super::discovery::{apply_peer_not_ready, apply_peer_ready};
use super::peer_cache::{snapshot_peer_addresses, PeerAddressSnapshot, PeerCaches};
use super::{
    check_business_allowed, try_send_event, DialRequest, BUSINESS_PROTOCOL_ID, NETWORK_CHUNK_SIZE,
};

const PRE_DIAL_CONNECTION_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Returns the maximum effective priority within the best candidate's scope.
///
/// The pre-dial check ensures the connection is in the right **scope**
/// (LAN / WAN / Relay), but does not require a specific transport.
/// For example, if the best candidate is LAN-QUIC (priority 10), the
/// threshold returned is 15 (LAN base 10 + TCP penalty 5), so a
/// TCP connection in the same scope also satisfies the check.
fn preferred_candidate_priority(snapshot: &PeerAddressSnapshot) -> Option<u8> {
    snapshot.candidate_addresses.first().map(|address| {
        let scope = infer_address_scope(address);
        scope.base_priority().saturating_add(TRANSPORT_PENALTY)
    })
}

async fn wait_for_preferred_connection(
    caches: &Arc<RwLock<PeerCaches>>,
    peer_id: &str,
    required_priority: u8,
    wait_budget: Duration,
) -> Result<()> {
    let started = tokio::time::Instant::now();
    let deadline = started + wait_budget;
    let mut poll_count: u32 = 0;

    loop {
        poll_count += 1;
        let snapshot = {
            let caches = caches.read().await;
            snapshot_peer_addresses(&caches, peer_id, Utc::now())
        };
        let best_connection_ready = snapshot
            .best_connected_effective_priority
            .is_some_and(|priority| priority <= required_priority);

        if snapshot.peer_marked_reachable && best_connection_ready {
            debug!(
                peer_id = %peer_id,
                poll_count,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "wait_for_preferred_connection succeeded"
            );
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            warn!(
                peer_id = %peer_id,
                poll_count,
                elapsed_ms = started.elapsed().as_millis() as u64,
                peer_marked_reachable = snapshot.peer_marked_reachable,
                best_connected_priority = ?snapshot.best_connected_effective_priority,
                required_priority,
                "wait_for_preferred_connection timed out"
            );
            return Err(anyhow!(
                "timed out waiting for explicit connection with priority <= {required_priority}"
            ));
        }

        tokio::time::sleep(PRE_DIAL_CONNECTION_POLL_INTERVAL).await;
    }
}

async fn ensure_explicit_connection(
    caches: &Arc<RwLock<PeerCaches>>,
    dial_tx: &mpsc::Sender<DialRequest>,
    peer_id_str: &str,
    peer: PeerId,
    denied_operation: &str,
    address_snapshot: &PeerAddressSnapshot,
    dial_decision: &str,
    open_timeout: Duration,
    open_started_at: tokio::time::Instant,
) -> Result<()> {
    if dial_decision == "reuse_existing_connection" {
        return Ok(());
    }

    let required_priority = preferred_candidate_priority(address_snapshot)
        .ok_or_else(|| anyhow!("no explicit dial candidates available for peer {peer_id_str}"))?;
    let tiers = {
        let caches = caches.read().await;
        caches
            .address_registry
            .candidates_by_tier(peer_id_str)
            .into_iter()
            .map(|(scope, recs)| {
                let addrs: Vec<String> = recs.iter().map(|r| r.addr.clone()).collect();
                (scope, addrs)
            })
            .collect::<Vec<_>>()
    };

    let mut dial_initiated = false;
    for (tier_index, (scope, addr_strings)) in tiers.iter().enumerate() {
        let addrs: Vec<libp2p::Multiaddr> = addr_strings
            .iter()
            .filter_map(|address| match address.parse() {
                Ok(addr) => Some(addr),
                Err(err) => {
                    warn!(
                        event = "business_stream.pre_dial",
                        operation = denied_operation,
                        peer_id = %peer_id_str,
                        scope = ?scope,
                        tier_index,
                        address = %address,
                        error = %err,
                        "failed to parse address, skipping"
                    );
                    None
                }
            })
            .collect();

        if addrs.is_empty() {
            continue;
        }

        let remaining = open_timeout
            .checked_sub(open_started_at.elapsed())
            .unwrap_or_default();
        if remaining.is_zero() {
            return Err(anyhow!(
                "business stream open timed out before dial tier {tier_index}"
            ));
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        info!(
            event = "business_stream.pre_dial",
            operation = denied_operation,
            peer_id = %peer_id_str,
            scope = ?scope,
            address_count = addrs.len(),
            addresses = ?addr_strings,
            tier_index,
            dial_decision,
            "sending pre-dial request with tier addresses"
        );

        let send_result = tokio::time::timeout(
            remaining,
            dial_tx.send(DialRequest {
                peer,
                addresses: addrs,
                allow_connected_dial: dial_decision == "upgrade_to_better_connection",
                bypass_address_filter: false,
                result_tx: tx,
            }),
        )
        .await;

        match send_result {
            Err(_elapsed) => {
                warn!(
                    event = "business_stream.pre_dial",
                    operation = denied_operation,
                    peer_id = %peer_id_str,
                    scope = ?scope,
                    tier_index,
                    dial_decision,
                    "dial send timed out, trying next tier"
                );
                continue;
            }
            Ok(Err(err)) => {
                return Err(anyhow!("failed to send dial request: {err}"));
            }
            Ok(Ok(())) => {}
        }

        let remaining_ack = open_timeout
            .checked_sub(open_started_at.elapsed())
            .unwrap_or_default();

        let ack_result = tokio::time::timeout(remaining_ack, rx).await;

        match ack_result {
            Err(_elapsed) => {
                warn!(
                    event = "business_stream.pre_dial",
                    operation = denied_operation,
                    peer_id = %peer_id_str,
                    scope = ?scope,
                    tier_index,
                    dial_decision,
                    "dial ack timed out, trying next tier"
                );
            }
            Ok(Ok(Ok(()))) => {
                debug!(
                    event = "business_stream.pre_dial",
                    operation = denied_operation,
                    peer_id = %peer_id_str,
                    scope = ?scope,
                    tier_index,
                    dial_decision,
                    "dial initiation succeeded"
                );
                dial_initiated = true;
                break;
            }
            Ok(Ok(Err(err))) => {
                let err_msg = err.to_string();
                // Another task is already dialing this peer — that dial will
                // establish the connection we need, so just wait for it.
                if err_msg.contains("dial is in progress") {
                    debug!(
                        event = "business_stream.pre_dial",
                        operation = denied_operation,
                        peer_id = %peer_id_str,
                        scope = ?scope,
                        tier_index,
                        dial_decision,
                        "dial already in progress from another task, waiting for connection"
                    );
                    dial_initiated = true;
                    break;
                }
                warn!(
                    event = "business_stream.pre_dial",
                    operation = denied_operation,
                    peer_id = %peer_id_str,
                    scope = ?scope,
                    tier_index,
                    dial_decision,
                    error = %err,
                    "dial initiation failed, trying next tier"
                );
            }
            Ok(Err(_)) => {
                warn!(
                    event = "business_stream.pre_dial",
                    operation = denied_operation,
                    peer_id = %peer_id_str,
                    scope = ?scope,
                    tier_index,
                    dial_decision,
                    "dial initiation channel dropped, trying next tier"
                );
            }
        }
    }

    if !dial_initiated {
        return Err(anyhow!("explicit pre-dial failed for all candidate tiers"));
    }

    let remaining_wait_budget = open_timeout
        .checked_sub(open_started_at.elapsed())
        .unwrap_or_default();
    if remaining_wait_budget.is_zero() {
        return Err(anyhow!(
            "business stream open timed out before explicit dial completed"
        ));
    }

    wait_for_preferred_connection(
        caches,
        peer_id_str,
        required_priority,
        remaining_wait_budget,
    )
    .await
}

pub(super) async fn execute_business_stream(
    control: &stream::Control,
    caches: &Arc<RwLock<PeerCaches>>,
    policy_resolver: &Arc<dyn ConnectionPolicyResolverPort>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    dial_tx: &mpsc::Sender<DialRequest>,
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
        let open_started_at = tokio::time::Instant::now();

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
        let result = match ensure_explicit_connection(
            caches,
            dial_tx,
            peer_id_str,
            peer,
            denied_operation,
            &address_snapshot,
            dial_decision,
            open_timeout,
            open_started_at,
        )
        .await
        {
            Err(err) => {
                warn!(
                    event = "business_stream.pre_dial_failed",
                    operation = denied_operation,
                    peer_id = %peer_id_str,
                    dial_decision,
                    candidate_address_count = address_snapshot.candidate_addresses.len(),
                    candidate_addresses = ?address_snapshot.candidate_addresses,
                    error = %err,
                    "explicit pre-dial did not produce a usable preferred connection"
                );
                Err(anyhow!("business stream pre-dial failed: {err}"))
            }
            Ok(()) if payload.is_none() => {
                // 方案B: ensure only establishes the connection (dial),
                // no dummy stream open/close needed.
                debug!(
                    event = "business_stream.ensure_dial_only",
                    operation = denied_operation,
                    peer_id = %peer_id_str,
                    dial_decision,
                    "ensure completed via dial-only (no stream probe)"
                );
                Ok(())
            }
            Ok(()) => match tokio::time::timeout(
                open_timeout
                    .checked_sub(open_started_at.elapsed())
                    .unwrap_or_default(),
                control.open_stream(peer, StreamProtocol::new(BUSINESS_PROTOCOL_ID)),
            )
            .await
            {
            Ok(Ok(mut stream)) => {
                // payload is guaranteed Some here — the None case
                // returns early after ensure_explicit_connection above.
                let Some(data) = payload else {
                    return Ok(());
                };
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

                let write_result = tokio::time::timeout(write_timeout, async {
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
                    Ok(Ok(())) => match tokio::time::timeout(close_timeout, stream.close()).await {
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
            }
        }};

        apply_business_stream_result(caches, event_tx, peer_id_str, &result).await;
        result
    }
    .instrument(span)
    .await
}

/// Update peer reachability state in `PeerCaches` and emit a corresponding `NetworkEvent`
/// reflecting whether a business stream completed successfully.
///
/// This function marks the peer as ready when `result` is `Ok(())`. On failure it
/// only emits `PeerNotReady` when there are no active connections left for that
/// peer; otherwise the connection-layer events remain the source of truth for
/// reachability. Address-level success/failure is recorded by connection-layer
/// events (e.g., `ConnectionEstablished` / `OutgoingConnectionError`), not by
/// this function.
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
    let event = {
        let mut caches = caches.write().await;
        if result.is_ok() {
            apply_peer_ready(&mut caches, peer_id, Utc::now())
        } else if caches.has_active_connections(peer_id) {
            None
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
