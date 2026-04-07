//! Business command orchestration — dispatches `BusinessCommand` variants,
//! delegates stream I/O to `business_stream`, and delivers results to callers.

use anyhow::anyhow;
use chrono::Utc;
use libp2p::futures::AsyncWriteExt;
use libp2p::{PeerId, StreamProtocol};
use libp2p_stream as stream;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::time::timeout;
use tracing::{debug, info, warn};
use uc_core::network::{DeviceAnnounceMessage, NetworkEvent, ProtocolMessage};
use uc_core::ports::ConnectionPolicyResolverPort;

use super::business_stream::execute_business_stream;
use super::peer_cache::PeerCaches;
use super::{
    BusinessCommand, BUSINESS_PROTOCOL_ID, BUSINESS_STREAM_CLOSE_TIMEOUT,
    BUSINESS_STREAM_OPEN_TIMEOUT, BUSINESS_STREAM_WRITE_TIMEOUT,
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
