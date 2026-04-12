//! Recovery probe — lightweight stream open used to confirm peer reachability.
//!
//! # What a probe is (from the PRD)
//!
//! 1. Attempt to open a business stream to the target peer.
//! 2. Do not write any payload.
//! 3. Close the stream immediately after the open call returns.
//!
//! A probe is **successful** when `open_stream` returns `Ok`.
//! A probe is **failed** when it returns an error or exceeds
//! [`BUSINESS_STREAM_OPEN_TIMEOUT`].
//!
//! The receiver already treats "stream opened, EOF before header" as a probe
//! (`stream_handler.rs:71-72`), so no receiver-side changes are needed.

use anyhow::{anyhow, Result};
use futures::AsyncWriteExt;
use libp2p::{Multiaddr, PeerId, StreamProtocol};
use libp2p_stream as stream;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, instrument, warn};

use super::{DialRequest, BUSINESS_PROTOCOL_ID, BUSINESS_STREAM_OPEN_TIMEOUT};

/// Outcome of a dispatched recovery probe, sent back to the swarm loop.
#[derive(Debug)]
pub(crate) struct ProbeOutcome {
    /// Peer the probe targeted.
    pub peer_id: String,
    /// Recovery cycle this probe belongs to.
    pub cycle_id: String,
    /// Probe attempt number within the cycle (1-based).
    pub attempt: u32,
    /// `Ok(())` = probe succeeded (stream open returned success).
    /// `Err(...)` = probe failed (stream open error or timeout).
    pub result: Result<()>,
}

/// Send a single recovery probe to `peer_id`.
///
/// Step 1 path: if `usable_addr` is `Some`, dial that specific address first,
/// then open a stream.  This is the "retry usable path" step from the PRD.
/// Pass `None` to skip the explicit dial (peer already connected, or Step 2
/// broad dial is already in progress).
///
/// The result is sent to `outcome_tx` so the swarm loop's `select!` can feed
/// it back into the [`RecoveryCoordinator`].
#[instrument(
    name = "recovery.probe",
    level = "debug",
    skip(control, dial_tx, outcome_tx, peer),
    fields(
        peer_id = %peer_id_str,
        recovery_cycle_id = %cycle_id,
        attempt,
        has_usable_addr = usable_addr.is_some()
    )
)]
pub(crate) async fn send_recovery_probe(
    mut control: stream::Control,
    dial_tx: mpsc::Sender<DialRequest>,
    peer_id_str: String,
    peer: PeerId,
    cycle_id: String,
    attempt: u32,
    usable_addr: Option<String>,
    outcome_tx: mpsc::Sender<ProbeOutcome>,
) {
    let result = run_probe(
        &mut control,
        &dial_tx,
        &peer_id_str,
        peer,
        &cycle_id,
        attempt,
        usable_addr,
    )
    .await;

    let outcome = ProbeOutcome {
        peer_id: peer_id_str,
        cycle_id,
        attempt,
        result,
    };
    // Ignore send error: the swarm loop may have shut down.
    let _ = outcome_tx.send(outcome).await;
}

async fn run_probe(
    control: &mut stream::Control,
    dial_tx: &mpsc::Sender<DialRequest>,
    peer_id_str: &str,
    peer: PeerId,
    cycle_id: &str,
    attempt: u32,
    usable_addr: Option<String>,
) -> Result<()> {
    // Step 1: if we have a specific address, dial it before opening the stream.
    if let Some(addr_str) = usable_addr {
        let addr: Multiaddr = addr_str
            .parse()
            .map_err(|e| anyhow!("probe: failed to parse usable_addr {addr_str}: {e}"))?;

        let (result_tx, result_rx) = oneshot::channel();
        dial_tx
            .send(DialRequest {
                peer,
                addresses: vec![addr.clone()],
                allow_connected_dial: false,
                bypass_address_filter: true,
                result_tx,
            })
            .await
            .map_err(|_| anyhow!("probe: dial_tx closed"))?;

        let dial_result = tokio::time::timeout(BUSINESS_STREAM_OPEN_TIMEOUT, result_rx).await;
        match dial_result {
            Err(_elapsed) => {
                warn!(
                    event = "peer.recovery_probe_failed",
                    peer_id = %peer_id_str,
                    recovery_cycle_id = %cycle_id,
                    attempt,
                    probe_method = "business_stream_open",
                    result = "failure",
                    error = "dial timed out",
                    "recovery probe dial timed out"
                );
                return Err(anyhow!("probe: dial timed out"));
            }
            Ok(Err(_channel_err)) => {
                return Err(anyhow!("probe: dial result channel dropped"));
            }
            Ok(Ok(Err(err))) => {
                // Dial errors that mean "already in progress" are fine — the
                // existing dial will establish the connection we need.
                let msg = err.to_string();
                if !msg.contains("dial is in progress") {
                    warn!(
                        event = "peer.recovery_probe_failed",
                        peer_id = %peer_id_str,
                        recovery_cycle_id = %cycle_id,
                        attempt,
                        probe_method = "business_stream_open",
                        result = "failure",
                        error_kind = "dial_error",
                        retryable = true,
                        error = %err,
                        "recovery probe dial error"
                    );
                    return Err(anyhow!("probe: dial error: {err}"));
                }
                debug!(
                    event = "peer.recovery_probe_dial_in_progress",
                    peer_id = %peer_id_str,
                    recovery_cycle_id = %cycle_id,
                    attempt,
                    "dial already in progress; piggy-backing on existing dial and proceeding to stream open"
                );
            }
            Ok(Ok(Ok(()))) => {
                // Dial initiated successfully.
                debug!(
                    event = "peer.recovery_probe_dial_ok",
                    peer_id = %peer_id_str,
                    recovery_cycle_id = %cycle_id,
                    attempt,
                    "dial initiated; proceeding to stream open"
                );
            }
        }
    }

    // Step 2: open the business stream (the actual probe).
    let open_result = tokio::time::timeout(
        BUSINESS_STREAM_OPEN_TIMEOUT,
        control.open_stream(peer, StreamProtocol::new(BUSINESS_PROTOCOL_ID)),
    )
    .await;

    match open_result {
        Err(_elapsed) => {
            warn!(
                event = "peer.recovery_probe_failed",
                peer_id = %peer_id_str,
                recovery_cycle_id = %cycle_id,
                attempt,
                probe_method = "business_stream_open",
                result = "failure",
                error = "stream open timed out",
                "recovery probe stream open timed out"
            );
            Err(anyhow!("probe: stream open timed out"))
        }
        Ok(Err(err)) => {
            warn!(
                event = "peer.recovery_probe_failed",
                peer_id = %peer_id_str,
                recovery_cycle_id = %cycle_id,
                attempt,
                probe_method = "business_stream_open",
                result = "failure",
                error = %err,
                "recovery probe stream open failed"
            );
            Err(anyhow!("probe: stream open error: {err}"))
        }
        Ok(Ok(mut stream)) => {
            // Close gracefully so the remote observes EOF before header
            // (rather than a RST from drop).
            if let Err(err) = stream.close().await {
                warn!(
                    event = "peer.recovery_probe_close_error",
                    peer_id = %peer_id_str,
                    recovery_cycle_id = %cycle_id,
                    attempt,
                    error = %err,
                    "recovery probe stream close failed (probe still counts as success)"
                );
            }
            drop(stream);
            info!(
                event = "peer.recovery_probe_succeeded",
                peer_id = %peer_id_str,
                recovery_cycle_id = %cycle_id,
                attempt,
                probe_method = "business_stream_open",
                result = "success",
                "recovery probe succeeded"
            );
            Ok(())
        }
    }
}
