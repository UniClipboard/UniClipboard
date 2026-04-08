use anyhow::anyhow;
use chrono::Utc;
use libp2p::core::ConnectedPoint;
use libp2p::futures::StreamExt;
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::{mdns, Multiaddr, PeerId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock, Semaphore};

use tracing::{debug, error, info, instrument, warn};
use uc_core::network::address_registry;
use uc_core::network::NetworkEvent;
use uc_core::ports::ConnectionPolicyResolverPort;

use super::behaviour::{Libp2pBehaviour, Libp2pBehaviourEvent};
use super::business_command::{
    business_command_log_fields, deliver_business_command_result, execute_business_command,
};
use super::dial_strategy::{
    dial_observation_from_error, successful_dial_observation, transport_label, transport_label_str,
};
use super::discovery::{
    apply_mdns_discovered, apply_mdns_expired, apply_peer_ready_from_connection,
    collect_mdns_discovered, collect_mdns_expired,
};
use super::peer_cache::{snapshot_peer_addresses, PeerCaches};
use super::{try_send_event, BusinessCommand, DialRequest, MAX_IN_FLIGHT_BUSINESS_COMMANDS};

/// Drives the libp2p Swarm event loop, processing network events, managing peer caches,
/// handling business commands, and emitting network events to the application.
///
/// This task runs until the swarm is terminated or a fatal internal error occurs. It:
/// - consumes and reacts to libp2p swarm events (mDNS discovery/expiry, connections, errors, listen addresses),
/// - maintains `PeerCaches` state (discoveries, reachability, dial observations and address registry GC),
/// - sequences outgoing business commands with a bounded concurrency semaphore and per-command lifecycle handling,
/// - records per-address dial successes/failures and emits high-level `NetworkEvent`s via `event_tx`,
/// - periodically triggers address-registry garbage collection.
///
/// # Examples
///
/// ```
/// # use tokio::runtime::Runtime;
/// # async fn example() {
/// // Typical usage: spawn the swarm loop onto a Tokio task after constructing the Swarm,
/// // caches, channels, and policy resolver. Types and construction are omitted here for brevity.
/// // tokio::spawn(run_swarm(swarm, caches, event_tx, policy_resolver, business_rx, local_peer_id));
/// # }
/// ```
#[instrument(name = "run_swarm", skip_all, fields(local_peer_id = %local_peer_id))]
pub(super) async fn run_swarm(
    mut swarm: Swarm<Libp2pBehaviour>,
    caches: Arc<RwLock<PeerCaches>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    policy_resolver: Arc<dyn ConnectionPolicyResolverPort>,
    mut business_rx: mpsc::Receiver<BusinessCommand>,
    mut dial_rx: mpsc::Receiver<DialRequest>,
    dial_tx: mpsc::Sender<DialRequest>,
    local_peer_id: Arc<str>,
) {
    info!("libp2p mDNS swarm started");
    let mut next_business_command_id: u64 = 1;
    let business_command_semaphore = Arc::new(Semaphore::new(MAX_IN_FLIGHT_BUSINESS_COMMANDS));
    let mut pending_business_command: Option<(u64, BusinessCommand)> = None;
    let mut gc_interval =
        tokio::time::interval(Duration::from_secs(address_registry::GC_INTERVAL_SECS));

    loop {
        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(Libp2pBehaviourEvent::Mdns(mdns_event)) => match mdns_event {
                        mdns::Event::Discovered(peers) => {
                            handle_mdns_discovered(peers, &mut swarm, &caches, &event_tx, &local_peer_id).await;
                        }
                        mdns::Event::Expired(peers) => {
                            handle_mdns_expired(peers, &caches, &event_tx, &local_peer_id).await;
                        }
                    },
                    SwarmEvent::Behaviour(Libp2pBehaviourEvent::Stream) => {}
                    SwarmEvent::ConnectionEstablished {
                        peer_id,
                        connection_id,
                        endpoint,
                        ..
                    } => {
                        handle_connection_established(
                            peer_id, connection_id, endpoint, &mut swarm, &caches, &event_tx, &local_peer_id,
                        ).await;
                    }
                    SwarmEvent::ConnectionClosed {
                        peer_id,
                        connection_id,
                        endpoint,
                        ..
                    } => {
                        handle_connection_closed(
                            peer_id, connection_id, endpoint, &caches, &event_tx, &local_peer_id,
                        ).await;
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                        handle_outgoing_connection_error(
                            peer_id, error, &caches, &event_tx, &local_peer_id,
                        ).await;
                    }
                    SwarmEvent::IncomingConnectionError {
                        send_back_addr,
                        error,
                        ..
                    } => {
                        handle_incoming_connection_error(
                            send_back_addr, error, &event_tx,
                        ).await;
                    }
                    SwarmEvent::NewListenAddr { address, .. } => {
                        info!(
                            event = "network.new_listen_addr",
                            listen_addr = %address,
                            transport = transport_label(&address),
                            "libp2p listening on discovered address"
                        );
                    }
                    _ => {}
                }
            }
            Some(command) = business_rx.recv(), if pending_business_command.is_none() => {
                let command_id = next_business_command_id;
                next_business_command_id = next_business_command_id.wrapping_add(1);
                let (operation, peer_id) = business_command_log_fields(&command);
                debug!(
                    cmd_id = command_id,
                    op = operation,
                    peer_id = %peer_id.unwrap_or("-"),
                    "business command queued"
                );
                pending_business_command = Some((command_id, command));
            }
            permit_result = business_command_semaphore.clone().acquire_owned(), if pending_business_command.is_some() => {
                let command_permit = match permit_result {
                    Ok(permit) => permit,
                    Err(err) => {
                        error!(error = %err, "business command semaphore closed");
                        break;
                    }
                };
                let Some((command_id, command)) = pending_business_command.take() else {
                    continue;
                };
                let (operation, peer_id) = business_command_log_fields(&command);
                debug!(
                    cmd_id = command_id,
                    op = operation,
                    peer_id = %peer_id.unwrap_or("-"),
                    "business command dispatched"
                );

                if let BusinessCommand::UnpairPeer { peer_id, result_tx } = command {
                    let _command_permit = command_permit;
                    let peer_id_str = peer_id.as_str().to_string();
                    let result = match peer_id_str.parse::<PeerId>() {
                        Ok(peer) => {
                            if swarm.is_connected(&peer) {
                                swarm
                                    .disconnect_peer_id(peer)
                                    .map_err(|_| anyhow!("failed to disconnect peer during unpair"))
                            } else {
                                Ok(())
                            }
                        }
                        Err(err) => Err(anyhow!("invalid peer id for unpair: {err}")),
                    };
                    deliver_business_command_result(result_tx, result, command_id, "unpair", &peer_id_str);
                    continue;
                }

                let command_control = swarm.behaviour().stream.new_control();
                let command_caches = caches.clone();
                let command_policy_resolver = policy_resolver.clone();
                let command_event_tx = event_tx.clone();
                let command_local_peer_id = local_peer_id.clone();
                let command_dial_tx = dial_tx.clone();
                tokio::spawn(async move {
                    let _command_permit = command_permit;
                    execute_business_command(
                        command,
                        command_id,
                        command_control,
                        command_caches,
                        command_policy_resolver,
                        command_event_tx,
                        command_local_peer_id,
                        command_dial_tx,
                    )
                    .await;
                });
            }

            Some(dial_req) = dial_rx.recv() => {
                handle_dial_request(dial_req, &mut swarm, &caches).await;
            }

            _ = gc_interval.tick() => {
                let removed = {
                    let mut caches = caches.write().await;
                    caches.gc_address_registry()
                };
                if removed > 0 {
                    debug!(
                        removed_count = removed,
                        "address registry GC completed"
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Extracted event handlers
// ---------------------------------------------------------------------------

#[instrument(name = "network.mdns_discovered", skip_all, fields(discovered_count = peers.len()))]
async fn handle_mdns_discovered(
    peers: Vec<(PeerId, Multiaddr)>,
    swarm: &mut Swarm<Libp2pBehaviour>,
    caches: &Arc<RwLock<PeerCaches>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    local_peer_id: &str,
) {
    let mut peers: Vec<(PeerId, Multiaddr)> = peers
        .into_iter()
        .filter(|(peer_id, _)| peer_id.to_string() != local_peer_id)
        .collect();

    // Sort so QUIC addresses are added to the swarm first
    peers.sort_by_key(|(_, addr)| {
        if addr.to_string().contains("/quic-v1") {
            0
        } else {
            1
        }
    });

    for (peer_id, address) in peers.iter() {
        swarm.add_peer_address(peer_id.clone(), address.clone());
    }

    let discovered = collect_mdns_discovered(peers);

    // Single write lock: apply discovery + read cache size
    let (events, cache_size) = {
        let mut caches = caches.write().await;
        let events = apply_mdns_discovered(&mut caches, discovered.clone(), Utc::now());
        let size = caches.discovered_peers.len();
        (events, size)
    };

    // Detailed per-peer logging only at DEBUG level to avoid formatting overhead
    if tracing::enabled!(tracing::Level::DEBUG) {
        for (peer_id, addresses) in &discovered {
            debug!(
                event = "peer.mdns_discovered",
                peer_id = %peer_id,
                address_count = addresses.len(),
                addresses = ?addresses,
                "recorded mDNS discovery snapshot"
            );
        }
    }

    info!(
        emitted_event_count = events.len(),
        discovered_cache_size = cache_size,
        "processed mdns discovered event"
    );

    for event in events {
        let _ = try_send_event(event_tx, event, "PeerDiscovered");
    }
}

#[instrument(name = "network.mdns_expired", skip_all, fields(expired_count = peers.len()))]
async fn handle_mdns_expired(
    peers: Vec<(PeerId, Multiaddr)>,
    caches: &Arc<RwLock<PeerCaches>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    local_peer_id: &str,
) {
    let peers: Vec<(PeerId, Multiaddr)> = peers
        .into_iter()
        .filter(|(peer_id, _)| peer_id.to_string() != local_peer_id)
        .collect();
    let expired = collect_mdns_expired(peers);

    // Single write lock: snapshot addresses before expiry, apply expiry, read cache size
    let (events, cache_size, expired_snapshots) = {
        let mut caches = caches.write().await;
        let expired_snapshots: Vec<_> = expired
            .iter()
            .map(|peer_id| {
                let addresses = caches
                    .discovered_peers
                    .get(peer_id)
                    .map(|peer| peer.addresses.clone())
                    .unwrap_or_default();
                (peer_id.clone(), addresses)
            })
            .collect();
        let events = apply_mdns_expired(&mut caches, expired);
        let size = caches.discovered_peers.len();
        (events, size, expired_snapshots)
    };

    if tracing::enabled!(tracing::Level::DEBUG) {
        for (peer_id, addresses) in &expired_snapshots {
            debug!(
                event = "peer.mdns_expired",
                peer_id = %peer_id,
                address_count = addresses.len(),
                addresses = ?addresses,
                "recorded mDNS expiry snapshot"
            );
        }
    }

    if cache_size == 0 && !events.is_empty() {
        warn!(
            emitted_event_count = events.len(),
            discovered_cache_size = cache_size,
            "All discovered peers expired via mDNS; outbound sync will be unavailable until peers are rediscovered"
        );
    } else {
        info!(
            emitted_event_count = events.len(),
            discovered_cache_size = cache_size,
            "processed mdns expired event"
        );
    }

    for event in events {
        let _ = try_send_event(event_tx, event, "PeerLost");
    }
}

#[instrument(name = "network.connection_established", skip_all, fields(peer_id = %peer_id))]
async fn handle_connection_established(
    peer_id: PeerId,
    connection_id: libp2p::swarm::ConnectionId,
    endpoint: ConnectedPoint,
    swarm: &mut Swarm<Libp2pBehaviour>,
    caches: &Arc<RwLock<PeerCaches>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    _local_peer_id: &str,
) {
    let peer_id_string = peer_id.to_string();
    let observed_at = Utc::now();
    let (address, endpoint_direction) = match &endpoint {
        ConnectedPoint::Dialer { address, .. } => (Some(address.clone()), "dialer"),
        ConnectedPoint::Listener { send_back_addr, .. } => {
            (Some(send_back_addr.clone()), "listener")
        }
    };

    if let Some(address) = address.as_ref() {
        swarm.add_peer_address(peer_id, address.clone());
    }

    let endpoint_address = address
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| "-".to_string());

    // Single write lock: record dial observation + apply connection + get snapshot + get inferior connections
    let (event, snapshot, inferior_connection_ids) = {
        let mut caches = caches.write().await;
        if endpoint_direction == "dialer" {
            caches.record_dial_observation(
                &peer_id_string,
                successful_dial_observation(&endpoint_address, observed_at),
            );
            caches.record_address_success(&peer_id_string, &endpoint_address);
        }
        let event = apply_peer_ready_from_connection(
            &mut caches,
            &peer_id_string,
            connection_id,
            observed_at,
            address,
        );
        let inferior_connection_ids = caches.inferior_connection_ids(&peer_id_string);
        let snapshot = snapshot_peer_addresses(&caches, &peer_id_string, observed_at);
        (event, snapshot, inferior_connection_ids)
    };

    if !inferior_connection_ids.is_empty() {
        for inferior_connection_id in inferior_connection_ids.iter().copied() {
            let _ = swarm.close_connection(inferior_connection_id);
        }
        info!(
            event = "peer.connection_superseded",
            kept_endpoint_address = %snapshot
                .best_connected_address
                .as_deref()
                .unwrap_or("-"),
            kept_effective_priority = snapshot
                .best_connected_effective_priority
                .unwrap_or(u8::MAX),
            closed_connection_count = inferior_connection_ids.len(),
            closed_connection_ids = ?inferior_connection_ids,
            "closed inferior connections after a better path became available"
        );
    }

    if let Some(event) = event {
        let _ = try_send_event(event_tx, event, "PeerReady");
        info!(
            event = "peer.connection_established",
            endpoint_direction,
            endpoint_address = %endpoint_address,
            endpoint_transport = transport_label_str(&endpoint_address),
            known_address_count = snapshot.candidate_addresses.len(),
            "peer connection established"
        );
    } else {
        debug!("connection established for unknown peer {peer_id_string}");
    }
}

#[instrument(name = "network.connection_closed", skip_all, fields(peer_id = %peer_id))]
async fn handle_connection_closed(
    peer_id: PeerId,
    connection_id: libp2p::swarm::ConnectionId,
    endpoint: ConnectedPoint,
    caches: &Arc<RwLock<PeerCaches>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    _local_peer_id: &str,
) {
    let peer_id = peer_id.to_string();

    // Single write lock: mark closed + snapshot
    let (event, snapshot) = {
        let mut caches = caches.write().await;
        let event = if caches.mark_connection_closed(&peer_id, connection_id) {
            Some(NetworkEvent::PeerNotReady {
                peer_id: peer_id.clone(),
            })
        } else {
            None
        };
        let snapshot = snapshot_peer_addresses(&caches, &peer_id, Utc::now());
        (event, snapshot)
    };

    let endpoint_address = match endpoint {
        ConnectedPoint::Dialer { address, .. } => address.to_string(),
        ConnectedPoint::Listener { send_back_addr, .. } => send_back_addr.to_string(),
    };

    if let Some(event) = event {
        let _ = try_send_event(event_tx, event, "PeerNotReady");
        info!(
            event = "peer.connection_closed",
            endpoint_address = %endpoint_address,
            known_address_count = snapshot.candidate_addresses.len(),
            connected_age_ms = ?snapshot.connected_age_ms,
            "peer connection closed"
        );
    } else {
        debug!(
            event = "peer.connection_closed",
            endpoint_address = %endpoint_address,
            remaining_connected_address_count = snapshot.connected_address_count,
            "connection closed but peer still has another active path"
        );
    }
}

#[instrument(name = "network.outgoing_connection_error", skip_all)]
async fn handle_outgoing_connection_error(
    peer_id: Option<PeerId>,
    error: libp2p::swarm::DialError,
    caches: &Arc<RwLock<PeerCaches>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    _local_peer_id: &str,
) {
    let peer_id_str = peer_id.as_ref().map(ToString::to_string);
    let observed_at = Utc::now();

    let snapshot = if let Some(peer_id) = peer_id_str.as_ref() {
        let mut caches = caches.write().await;
        let observation = dial_observation_from_error(&error, observed_at);
        let error_msg = error.to_string();
        for addr in &observation.dial_attempt_addresses {
            caches.record_address_failure(peer_id, addr, &error_msg);
        }
        caches.record_dial_observation(peer_id, observation);
        Some(snapshot_peer_addresses(&caches, peer_id, observed_at))
    } else {
        None
    };

    error!(
        event = "peer.outgoing_connection_error",
        peer_id = %peer_id_str.as_deref().unwrap_or("-"),
        known_address_count = snapshot
            .as_ref()
            .map(|s| s.candidate_addresses.len())
            .unwrap_or(0),
        known_addresses = ?snapshot
            .as_ref()
            .map(|s| s.candidate_addresses.clone())
            .unwrap_or_default(),
        chosen_dial_addr = %snapshot
            .as_ref()
            .and_then(|s| s.chosen_dial_addr.as_deref())
            .unwrap_or("-"),
        chosen_dial_addr_resolution = snapshot
            .as_ref()
            .and_then(|s| s.chosen_dial_addr_resolution)
            .unwrap_or("unknown"),
        dial_attempt_address_count = snapshot
            .as_ref()
            .map(|s| s.dial_attempt_address_count)
            .unwrap_or(0),
        dial_attempt_addresses = ?snapshot
            .as_ref()
            .map(|s| s.dial_attempt_addresses.clone())
            .unwrap_or_default(),
        peer_marked_reachable = snapshot
            .as_ref()
            .map(|s| s.peer_marked_reachable)
            .unwrap_or(false),
        connected_age_ms = ?snapshot.as_ref().and_then(|s| s.connected_age_ms),
        discovered_age_ms = ?snapshot.as_ref().and_then(|s| s.discovered_age_ms),
        last_seen_age_ms = ?snapshot.as_ref().and_then(|s| s.last_seen_age_ms),
        last_dial_age_ms = ?snapshot.as_ref().and_then(|s| s.last_dial_age_ms),
        last_dial_outcome = snapshot
            .as_ref()
            .and_then(|s| s.last_dial_outcome)
            .unwrap_or("unknown"),
        error = %error,
        "outgoing connection error"
    );

    if let Err(err) = event_tx
        .send(NetworkEvent::Error("network connection error".to_string()))
        .await
    {
        warn!("failed to publish network error event: {err}");
    }
}

#[instrument(name = "network.incoming_connection_error", skip_all, fields(send_back_addr = %send_back_addr))]
async fn handle_incoming_connection_error(
    send_back_addr: Multiaddr,
    error: libp2p::swarm::ListenError,
    event_tx: &mpsc::Sender<NetworkEvent>,
) {
    error!(
        event = "peer.incoming_connection_error",
        transport = transport_label(&send_back_addr),
        error = %error,
        "incoming connection error"
    );
    if let Err(err) = event_tx
        .send(NetworkEvent::Error("network connection error".to_string()))
        .await
    {
        warn!("failed to publish network error event: {err}");
    }
}

#[instrument(name = "network.dial_request", skip_all, fields(peer_id = %dial_req.peer))]
async fn handle_dial_request(
    dial_req: DialRequest,
    swarm: &mut Swarm<Libp2pBehaviour>,
    caches: &Arc<RwLock<PeerCaches>>,
) {
    if swarm.is_connected(&dial_req.peer) && !dial_req.allow_connected_dial {
        debug!(
            peer_id = %dial_req.peer,
            "pre-dial: peer already connected, skipping dial"
        );
        let _ = dial_req.result_tx.send(Ok(()));
        return;
    }

    use libp2p::swarm::dial_opts::{DialOpts, PeerCondition};

    // Filter stale mDNS addresses before dialing
    let live_addresses: std::collections::HashSet<String> = {
        let caches = caches.read().await;
        caches
            .address_registry
            .candidates_for(&dial_req.peer.to_string())
            .iter()
            .map(|r| r.addr.clone())
            .collect()
    };

    let filtered_addresses: Vec<libp2p::Multiaddr> = dial_req
        .addresses
        .into_iter()
        .filter(|a| live_addresses.contains(&a.to_string()))
        .collect();

    if filtered_addresses.is_empty() {
        warn!(
            peer_id = %dial_req.peer,
            "pre-dial: all explicit addresses expired in PeerCaches"
        );
        let _ = dial_req.result_tx.send(Err(anyhow!(
            "pre-dial addresses expired before dial started"
        )));
        return;
    }

    let addr_count = filtered_addresses.len();
    debug!(
        peer_id = %dial_req.peer,
        address_count = addr_count,
        addresses = ?filtered_addresses.iter().map(ToString::to_string).collect::<Vec<_>>(),
        "pre-dial: initiating dial with explicit addresses"
    );

    let peer_condition = if dial_req.allow_connected_dial {
        PeerCondition::NotDialing
    } else {
        PeerCondition::DisconnectedAndNotDialing
    };

    let result = swarm.dial(
        DialOpts::peer_id(dial_req.peer)
            .addresses(filtered_addresses)
            .condition(peer_condition)
            .build(),
    );

    match result {
        Ok(()) => {
            let _ = dial_req.result_tx.send(Ok(()));
        }
        Err(err) => {
            warn!(
                peer_id = %dial_req.peer,
                error = %err,
                "pre-dial: dial initiation failed"
            );
            let _ = dial_req
                .result_tx
                .send(Err(anyhow!("pre-dial failed: {err}")));
        }
    }
}
