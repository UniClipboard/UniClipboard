//! T3a — `Connection::closed()` liveness probe.
//!
//! This file originally probed iroh 0.95's `Endpoint::conn_type(peer_id)`
//! semantics — three tests asserting properties of the
//! `Watcher<ConnectionType>` it returned (see git history for the full
//! study). iroh 0.97 / 0.98 removed `conn_type` in favour of the
//! snapshot-style `remote_info(id) -> Option<RemoteInfo>`, so those
//! assertions no longer apply (and won't compile). The probe that
//! survives is the one that actually backs `IrohPresenceAdapter` in
//! production: `Connection::closed().await` fires reliably when the peer
//! tears down, which is what the adapter's per-peer watchdog awaits.
//!
//! Runs loopback-only (relays disabled) so it has no external dependency.

use std::time::Duration;

use iroh::{Endpoint, RelayMode};

const PROBE_ALPN: &[u8] = b"uniclipboard/presence-probe/0";

/// Bind a single endpoint with relays disabled and `PROBE_ALPN` registered.
async fn bind_endpoint() -> Endpoint {
    Endpoint::builder(iroh::endpoint::presets::N0)
        .alpns(vec![PROBE_ALPN.to_vec()])
        .relay_mode(RelayMode::Disabled)
        .bind()
        .await
        .expect("bind endpoint")
}

/// Wait until `endpoint.addr().addrs` has published at least one direct addr.
/// Without relays the magicsock takes a handful of ms to surface loopback
/// addresses; short retry loop matches the Slice 1 pairing test fixture.
async fn wait_for_direct_addrs(endpoint: &Endpoint) {
    for _ in 0..100 {
        if !endpoint.addr().addrs.is_empty() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("endpoint never published direct addresses");
}

/// Spawn an accept loop that holds every incoming connection open until the
/// peer closes it. Returns a cancel token via `JoinHandle::abort()` drop.
fn spawn_hold_open_acceptor(endpoint: Endpoint) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(incoming) = endpoint.accept().await {
            let Ok(conn) = incoming.await else { continue };
            tokio::spawn(async move {
                // Hold until peer closes.
                let _ = conn.closed().await;
            });
        }
    })
}

/// The actual fallback: `Connection::closed().await` fires when the peer
/// drops. Adapter `IrohPresenceAdapter` holds the `Connection` and awaits
/// this future in a watchdog task per tracked peer.
#[tokio::test]
async fn connection_closed_fires_when_peer_shuts_down() {
    let peer = bind_endpoint().await;
    wait_for_direct_addrs(&peer).await;
    let peer_addr = peer.addr();
    let acceptor = spawn_hold_open_acceptor(peer.clone());

    let prober = bind_endpoint().await;
    wait_for_direct_addrs(&prober).await;
    let connection = prober
        .connect(peer_addr, PROBE_ALPN)
        .await
        .expect("dial peer");
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert!(
        connection.close_reason().is_none(),
        "connection should be alive immediately after dial",
    );

    let closed_fut = connection.closed();
    let started = std::time::Instant::now();

    // Shut down the peer entirely.
    acceptor.abort();
    peer.close().await;

    // Connection::closed() must resolve within the Phase 1 budget (≤ 10s).
    match tokio::time::timeout(Duration::from_secs(10), closed_fut).await {
        Ok(reason) => {
            println!(
                "Connection::closed() fired after {:?} with reason = {:?}",
                started.elapsed(),
                reason
            );
        }
        Err(_) => panic!(
            "Connection::closed() did not fire within 10s — offline detection \
             would miss Phase 1 budget",
        ),
    }

    prober.close().await;
}
