use anyhow::anyhow;
use chrono::Utc;
use libp2p::core::ConnectedPoint;
use libp2p::futures::StreamExt;
use libp2p::swarm::{Swarm, SwarmEvent};
use libp2p::{mdns, Multiaddr, PeerId};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock, Semaphore};

use tracing::{debug, error, info, info_span, instrument, warn, Instrument};
use uc_core::network::address_registry;
use uc_core::network::events::PeerRuntimeState;
use uc_core::network::{NetworkEvent, PairingState};
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
use super::recovery_coordinator::{CoordinatorCmd, RecoveryCoordinator};
use super::recovery_probe::{send_recovery_probe, ProbeOutcome};
use super::{
    try_send_event, BusinessCommand, DialRequest, DIAL_FAILURE_STREAK_THRESHOLD,
    MAX_IN_FLIGHT_BUSINESS_COMMANDS, RECOVERY_PROBE_CADENCE,
};

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
/// Drive the libp2p swarm with recovery coordinator support.
///
/// See module-level doc for the full event loop description.
///
/// `platform_signal_rx` — optional channel from the platform integration layer
/// (sleep/wake, network change).  `None` when platform integration is not yet
/// wired (Wave 1 graceful degradation).
///
/// Returns `Some((business_rx, dial_rx, platform_signal_rx))` when a session
/// rebuild was requested by the recovery coordinator — the caller is expected
/// to tear down the current swarm, build a fresh one, and call `run_swarm`
/// again with the returned receivers. Returns `None` on normal exit (channels
/// closed).
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
    mut platform_signal_rx: Option<mpsc::Receiver<PlatformSignal>>,
) -> Option<(
    mpsc::Receiver<BusinessCommand>,
    mpsc::Receiver<DialRequest>,
    Option<mpsc::Receiver<PlatformSignal>>,
)> {
    info!("libp2p mDNS swarm started");
    let mut next_business_command_id: u64 = 1;
    let business_command_semaphore = Arc::new(Semaphore::new(MAX_IN_FLIGHT_BUSINESS_COMMANDS));
    let mut pending_business_command: Option<(u64, BusinessCommand)> = None;
    let mut gc_interval =
        tokio::time::interval(Duration::from_secs(address_registry::GC_INTERVAL_SECS));

    // ── Recovery coordinator ──────────────────────────────────────────────
    let mut coordinator = RecoveryCoordinator::new();
    let mut recovery_tick_interval = tokio::time::interval(RECOVERY_PROBE_CADENCE);
    // Channel on which spawned probe tasks send back their outcomes.
    let (probe_outcome_tx, mut probe_outcome_rx) = mpsc::channel::<ProbeOutcome>(32);
    // Set to true when the coordinator requests a full session rebuild.
    let mut should_rebuild = false;

    loop {
        tokio::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::Behaviour(Libp2pBehaviourEvent::Mdns(mdns_event)) => match mdns_event {
                        mdns::Event::Discovered(peers) => {
                            handle_mdns_discovered(peers, &mut swarm, &caches, &event_tx, &local_peer_id).await;
                        }
                        mdns::Event::Expired(peers) => {
                            handle_mdns_expired(
                                peers,
                                &caches,
                                &event_tx,
                                &policy_resolver,
                                &local_peer_id,
                                &mut coordinator,
                            ).await;
                        }
                    },
                    SwarmEvent::Behaviour(Libp2pBehaviourEvent::Stream) => {}
                    SwarmEvent::ConnectionEstablished {
                        peer_id,
                        connection_id,
                        endpoint,
                        ..
                    } => {
                        let peer_id_str = peer_id.to_string();
                        handle_connection_established(
                            peer_id, connection_id, endpoint, &mut swarm, &caches, &event_tx, &local_peer_id,
                        ).await;
                        // Notify coordinator: this may end a recovery cycle.
                        let recovery_cmds = coordinator.on_connection_established(&peer_id_str, Instant::now());
                        // Obtain stream control BEFORE the await boundary (Swarm is !Sync).
                        let sc = swarm.behaviour().stream.new_control();
                        if dispatch_coordinator_cmds(
                            recovery_cmds, sc, &caches, &event_tx, &dial_tx,
                            &probe_outcome_tx,
                        ).await {
                            should_rebuild = true;
                            break;
                        }
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
                        let failure_info = handle_outgoing_connection_error(
                            peer_id, error, &caches, &event_tx, &local_peer_id,
                        ).await;

                        // Wire dial failure streak → recovery coordinator.
                        if let Some((peer_id_str, consecutive_failures)) = failure_info {
                            if consecutive_failures >= DIAL_FAILURE_STREAK_THRESHOLD
                                && !coordinator.is_recovering(&peer_id_str)
                            {
                                let peer_id_core = uc_core::PeerId::from(peer_id_str.clone());
                                let is_paired = async {
                                    match policy_resolver.resolve_for_peer(&peer_id_core).await {
                                        Ok(policy) => policy.pairing_state == PairingState::Trusted,
                                        Err(err) => {
                                            warn!(
                                                event = "recovery.resolve_pairing_state_failed",
                                                peer_id = %peer_id_str,
                                                error = %err,
                                                "failed to resolve pairing state; treating as paired (optimistic)"
                                            );
                                            // Optimistic: allow recovery to proceed so transient
                                            // lookup failures don't silently suppress it.  If the
                                            // peer turns out to be unpaired the probe will fail
                                            // fast with no side-effects.
                                            true
                                        }
                                    }
                                }
                                .instrument(info_span!(
                                    "recovery.resolve_pairing_state",
                                    peer_id = %peer_id_str
                                ))
                                .await;

                                if is_paired {
                                    info!(
                                        event = "peer.dial_failure_streak_detected",
                                        peer_id = %peer_id_str,
                                        consecutive_failures,
                                        threshold = DIAL_FAILURE_STREAK_THRESHOLD,
                                        "dial failure streak for paired peer — starting recovery"
                                    );
                                    let recovery_cmds = coordinator.on_dial_failure_streak(
                                        peer_id_str, Instant::now(),
                                    );
                                    let sc = swarm.behaviour().stream.new_control();
                                    if dispatch_coordinator_cmds(
                                        recovery_cmds, sc, &caches, &event_tx, &dial_tx,
                                        &probe_outcome_tx,
                                    ).await {
                                        should_rebuild = true;
                                        break;
                                    }
                                }
                            }
                        }
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

            // ── Recovery probe tick ───────────────────────────────────────
            _ = recovery_tick_interval.tick() => {
                let cmds = coordinator.tick(Instant::now());
                let sc = swarm.behaviour().stream.new_control();
                if dispatch_coordinator_cmds(
                    cmds, sc, &caches, &event_tx, &dial_tx,
                    &probe_outcome_tx,
                ).await {
                    should_rebuild = true;
                    break;
                }
            }

            // ── Probe outcome ─────────────────────────────────────────────
            Some(outcome) = probe_outcome_rx.recv() => {
                debug!(
                    event = "peer.recovery_probe_outcome_received",
                    peer_id = %outcome.peer_id,
                    recovery_cycle_id = %outcome.cycle_id,
                    attempt = outcome.attempt,
                    success = outcome.result.is_ok(),
                    "received probe outcome from spawned task"
                );
                let cmds = coordinator.on_probe_result(
                    &outcome.peer_id,
                    &outcome.cycle_id,
                    outcome.result.is_ok(),
                    outcome.result.as_ref().err().map(|e| e.to_string()).as_deref(),
                    Instant::now(),
                );
                let sc = swarm.behaviour().stream.new_control();
                if dispatch_coordinator_cmds(
                    cmds, sc, &caches, &event_tx, &dial_tx,
                    &probe_outcome_tx,
                ).await {
                    should_rebuild = true;
                    break;
                }
            }

            // ── Platform signal (sleep/wake, network change) ──────────────
            Some(signal) = async {
                if let Some(rx) = platform_signal_rx.as_mut() { rx.recv().await } else { None }
            } => {
                let signal_kind = match signal {
                    PlatformSignal::SleepWake => "sleep_wake",
                    PlatformSignal::NetworkChange => "network_change",
                };
                info!(
                    event = "platform.signal_received",
                    signal = signal_kind,
                    "platform signal received; forwarding to recovery coordinator"
                );
                let cmds = match signal {
                    PlatformSignal::SleepWake => coordinator.on_sleep_wake(Instant::now()),
                    PlatformSignal::NetworkChange => coordinator.on_network_change(Instant::now()),
                };
                let sc = swarm.behaviour().stream.new_control();
                if dispatch_coordinator_cmds(
                    cmds, sc, &caches, &event_tx, &dial_tx,
                    &probe_outcome_tx,
                ).await {
                    should_rebuild = true;
                    break;
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

    // Fail any buffered business command so its oneshot receiver doesn't hang.
    if let Some((_cmd_id, cmd)) = pending_business_command {
        match cmd {
            BusinessCommand::SendClipboard { result_tx, .. }
            | BusinessCommand::EnsureBusinessPath { result_tx, .. }
            | BusinessCommand::UnpairPeer { result_tx, .. } => {
                let _ = result_tx.send(Err(anyhow!("session rebuild in progress")));
            }
            BusinessCommand::AnnounceDeviceName { .. } => {}
        }
    }

    if should_rebuild {
        info!(
            event = "network.session_rebuild_loop_exited",
            "run_swarm exiting to allow session rebuild"
        );
        Some((business_rx, dial_rx, platform_signal_rx))
    } else {
        info!("run_swarm exiting normally");
        None
    }
}

// ── Platform integration types ────────────────────────────────────────────────

/// Platform-level signals that can trigger recovery actions.
///
/// The platform integration layer (task #8 — IOKit / SCNetworkReachability on
/// macOS, netlink on Linux, etc.) sends these into the swarm loop via the
/// optional `platform_signal_rx` channel.
#[derive(Debug, Clone, Copy)]
pub(super) enum PlatformSignal {
    /// The local device just woke from sleep.
    //
    // Only constructed on non-test macOS builds where IOKit is wired up
    // (see `platform_signals::macos`). Test and non-macOS builds rely on
    // reactive recovery triggers instead.
    #[allow(dead_code)]
    SleepWake,
    /// A local network interface or IP address changed.
    NetworkChange,
}

// ── Coordinator command dispatcher ────────────────────────────────────────────

/// Execute a batch of coordinator commands returned from a signal or tick.
///
/// `stream_control` must be obtained from `swarm.behaviour().stream.new_control()`
/// by the caller **before** the await boundary (so `Swarm` is not held across
/// any `.await` inside this function — `Swarm` is `!Sync` and causes non-`Send`
/// futures if held over an await point).
/// Returns `true` if the coordinator requested a full session rebuild
/// (`CoordinatorCmd::RebuildSession`), in which case `run_swarm` should break
/// its event loop and return the receivers to the caller for restart.
#[instrument(name = "recovery.dispatch_cmds", skip_all, fields(cmd_count = cmds.len()))]
async fn dispatch_coordinator_cmds(
    cmds: Vec<CoordinatorCmd>,
    stream_control: libp2p_stream::Control,
    caches: &Arc<RwLock<PeerCaches>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    dial_tx: &mpsc::Sender<DialRequest>,
    probe_outcome_tx: &mpsc::Sender<ProbeOutcome>,
) -> bool {
    for cmd in cmds {
        match cmd {
            CoordinatorCmd::EmitEvent(event) => {
                let label = match &event {
                    NetworkEvent::PeerStateChanged { state, .. } => match state {
                        PeerRuntimeState::Online => "PeerStateOnline",
                        PeerRuntimeState::Recovering => "PeerStateRecovering",
                        PeerRuntimeState::Offline => "PeerStateOffline",
                    },
                    NetworkEvent::PeerRecoveryStarted { .. } => "PeerRecoveryStarted",
                    NetworkEvent::PeerRecovered { .. } => "PeerRecovered",
                    NetworkEvent::PeerRecoveryFailed { .. } => "PeerRecoveryFailed",
                    _ => "recovery_event",
                };
                let _ = try_send_event(event_tx, event, label);
            }

            CoordinatorCmd::SendProbe {
                peer_id,
                cycle_id,
                attempt,
            } => {
                let Ok(peer) = peer_id.parse::<PeerId>() else {
                    warn!(
                        event = "peer.recovery_probe_failed",
                        peer_id = %peer_id,
                        recovery_cycle_id = %cycle_id,
                        attempt,
                        error = "invalid peer id",
                        "recovery probe skipped: could not parse peer id"
                    );
                    continue;
                };

                // Retrieve the last known usable address for Step 1.
                let usable_addr = {
                    let caches = caches.read().await;
                    caches
                        .last_dial_observations
                        .get(&peer_id)
                        .and_then(|obs| obs.chosen_dial_addr.clone())
                };

                info!(
                    event = "peer.recovery_probe_attempt",
                    peer_id = %peer_id,
                    recovery_cycle_id = %cycle_id,
                    attempt,
                    probe_method = "business_stream_open",
                    usable_addr = usable_addr.as_deref().unwrap_or("-"),
                    "dispatching recovery probe"
                );

                let probe_control = stream_control.clone();
                let probe_dial_tx = dial_tx.clone();
                let probe_outcome_tx = probe_outcome_tx.clone();
                tokio::spawn(send_recovery_probe(
                    probe_control,
                    probe_dial_tx,
                    peer_id,
                    peer,
                    cycle_id,
                    attempt,
                    usable_addr,
                    probe_outcome_tx,
                ));
            }

            CoordinatorCmd::DialBroad {
                peer_id,
                cycle_id,
                escalation_level,
            } => {
                // Step 2: dial all known candidate addresses for the peer.
                let peer = match peer_id.parse::<PeerId>() {
                    Ok(p) => p,
                    Err(err) => {
                        error!(
                            event = "peer.recovery_dial_broad_parse_failed",
                            peer_id = %peer_id,
                            recovery_cycle_id = %cycle_id,
                            escalation_level,
                            error = %err,
                            "Step 2 broad dial: failed to parse peer id"
                        );
                        continue;
                    }
                };

                let candidate_addresses: Vec<libp2p::Multiaddr> = {
                    let caches = caches.read().await;
                    caches
                        .address_registry
                        .candidates_for(&peer_id)
                        .iter()
                        .filter_map(|r| r.addr.parse().ok())
                        .collect()
                };

                if candidate_addresses.is_empty() {
                    info!(
                        event = "peer.recovery_escalated",
                        peer_id = %peer_id,
                        recovery_cycle_id = %cycle_id,
                        escalation_level,
                        candidate_address_count = 0,
                        "Step 2 broad dial skipped: no candidate addresses"
                    );
                    continue;
                }

                info!(
                    event = "peer.recovery_escalated",
                    peer_id = %peer_id,
                    recovery_cycle_id = %cycle_id,
                    escalation_level,
                    candidate_address_count = candidate_addresses.len(),
                    "Step 2 broad dial: dialing all known candidate addresses"
                );

                let (result_tx, _result_rx) = tokio::sync::oneshot::channel();
                if let Err(err) = dial_tx
                    .send(DialRequest {
                        peer,
                        addresses: candidate_addresses,
                        allow_connected_dial: false,
                        bypass_address_filter: false,
                        result_tx,
                    })
                    .await
                {
                    error!(
                        event = "peer.recovery_dial_broad_send_failed",
                        peer_id = %peer_id,
                        recovery_cycle_id = %cycle_id,
                        escalation_level,
                        error = %err,
                        "Step 2 broad dial: failed to send dial request"
                    );
                }
            }

            CoordinatorCmd::RebuildSession {
                rebuild_id,
                reason,
                participating_peer_ids,
            } => {
                info!(
                    event = "network.session_rebuild_started",
                    rebuild_id = %rebuild_id,
                    rebuild_reason = reason.as_str(),
                    recovering_peer_count = participating_peer_ids.len(),
                    "local network session rebuild requested; exiting run_swarm for restart"
                );
                // Signal the caller to tear down the current swarm and
                // rebuild.  Remaining commands (if any) are intentionally
                // dropped — they're moot once the swarm is being torn down.
                return true;
            }
        }
    }
    false
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

/// Handle mDNS expiry.
///
/// For **paired** peers (`Trusted` pairing state), recovery is started instead
/// of immediately emitting `PeerLost`.  For unpaired peers the original
/// behavior is preserved.
#[instrument(name = "network.mdns_expired", skip_all, fields(expired_count = peers.len()))]
async fn handle_mdns_expired(
    peers: Vec<(PeerId, Multiaddr)>,
    caches: &Arc<RwLock<PeerCaches>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    policy_resolver: &Arc<dyn ConnectionPolicyResolverPort>,
    local_peer_id: &str,
    coordinator: &mut RecoveryCoordinator,
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
        // For paired peers, start a recovery cycle instead of emitting PeerLost
        // immediately.  The coordinator will emit PeerLost (as PeerStateOffline)
        // only after the 120-second recovery window has been exhausted.
        if let NetworkEvent::PeerLost(ref peer_id_str) = event {
            let peer_id_core = uc_core::PeerId::from(peer_id_str.clone());
            let is_paired = async {
                match policy_resolver.resolve_for_peer(&peer_id_core).await {
                    Ok(policy) => policy.pairing_state == PairingState::Trusted,
                    Err(_) => false,
                }
            }
            .instrument(info_span!(
                "recovery.resolve_pairing_state",
                peer_id = %peer_id_str
            ))
            .await;

            if is_paired {
                debug!(
                    peer_id = %peer_id_str,
                    "mDNS expired for paired peer — starting recovery cycle instead of PeerLost"
                );
                // Recovery cycle emits PeerRecoveryStarted; PeerLost is suppressed.
                let recovery_cmds =
                    coordinator.on_mdns_expired(peer_id_str.clone(), Instant::now());
                for cmd in recovery_cmds {
                    if let CoordinatorCmd::EmitEvent(ev) = cmd {
                        let _ = try_send_event(event_tx, ev, "PeerRecoveryStarted");
                    }
                }
                continue;
            }
        }

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
            connected_address_count = snapshot.connected_address_count,
            connected_addresses = ?snapshot.connected_addresses,
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
            connected_address_count = snapshot.connected_address_count,
            connected_addresses = ?snapshot.connected_addresses,
            best_connected_address = %snapshot
                .best_connected_address
                .as_deref()
                .unwrap_or("-"),
            best_connected_transport = snapshot
                .best_connected_address
                .as_deref()
                .map(transport_label_str)
                .unwrap_or("unknown"),
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
            connected_address_count = snapshot.connected_address_count,
            connected_addresses = ?snapshot.connected_addresses,
            best_connected_address = %snapshot
                .best_connected_address
                .as_deref()
                .unwrap_or("-"),
            "peer connection closed"
        );
    } else {
        debug!(
            event = "peer.connection_closed",
            endpoint_address = %endpoint_address,
            remaining_connected_address_count = snapshot.connected_address_count,
            remaining_connected_addresses = ?snapshot.connected_addresses,
            best_connected_address = %snapshot
                .best_connected_address
                .as_deref()
                .unwrap_or("-"),
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
) -> Option<(String, u32)> {
    let peer_id_str = peer_id.as_ref().map(ToString::to_string);
    let observed_at = Utc::now();

    let (snapshot, consecutive_failures) = if let Some(peer_id) = peer_id_str.as_ref() {
        let mut caches = caches.write().await;
        let observation = dial_observation_from_error(&error, observed_at);
        let error_msg = error.to_string();
        for addr in &observation.dial_attempt_addresses {
            caches.record_address_failure(peer_id, addr, &error_msg);
        }
        caches.record_dial_observation(peer_id, observation);
        let failures = caches.record_dial_failure(peer_id);
        (
            Some(snapshot_peer_addresses(&caches, peer_id, observed_at)),
            Some(failures),
        )
    } else {
        (None, None)
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

    peer_id_str.zip(consecutive_failures)
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

    // Filter stale mDNS addresses before dialing.  Recovery probes pass
    // historical addresses from `last_dial_observations` that are deliberately
    // absent from the live registry; honour `bypass_address_filter` so they
    // are not discarded.
    let filtered_addresses: Vec<libp2p::Multiaddr> = if dial_req.bypass_address_filter {
        dial_req.addresses
    } else {
        let live_addresses: std::collections::HashSet<String> = {
            let caches = caches.read().await;
            caches
                .address_registry
                .candidates_for(&dial_req.peer.to_string())
                .iter()
                .map(|r| r.addr.clone())
                .collect()
        };

        dial_req
            .addresses
            .into_iter()
            .filter(|a| live_addresses.contains(&a.to_string()))
            .collect()
    };

    if filtered_addresses.is_empty() {
        warn!(
            peer_id = %dial_req.peer,
            bypass_filter = dial_req.bypass_address_filter,
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
