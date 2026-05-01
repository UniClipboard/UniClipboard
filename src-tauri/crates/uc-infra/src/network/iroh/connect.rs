use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr};
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
const STAGGERED_DELAYS: [Duration; 3] = [
    Duration::from_millis(0),
    Duration::from_secs(2),
    Duration::from_secs(5),
];

pub(super) async fn connect_with_staggered_retry(
    endpoint: Arc<Endpoint>,
    addr: EndpointAddr,
    alpn: &'static [u8],
    purpose: &'static str,
) -> Result<Connection, String> {
    let mut attempts = JoinSet::new();

    for (idx, delay) in STAGGERED_DELAYS.iter().copied().enumerate() {
        let endpoint = Arc::clone(&endpoint);
        let addr = addr.clone();
        let addr_id = addr.id;
        attempts.spawn(async move {
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }

            let attempt_no = idx + 1;
            debug!(
                purpose,
                attempt = attempt_no,
                timeout_ms = ATTEMPT_TIMEOUT.as_millis(),
                "iroh connect attempt started"
            );

            match tokio::time::timeout(ATTEMPT_TIMEOUT, endpoint.connect(addr, alpn)).await {
                Ok(Ok(connection)) => {
                    // Diagnostic for UniClipboard#486 — log which path won
                    // the candidate race. iroh 0.98 replaced the older
                    // `Endpoint::conn_type(id) -> Watcher<ConnectionType>` with
                    // the snapshot-style `remote_info(id) -> Option<RemoteInfo>`;
                    // we render only the `Active` `TransportAddrInfo`s, which
                    // is the closest equivalent to the old Direct/Relay/Mixed
                    // tag for "what's the path actually carrying packets right
                    // now".
                    let conn_type_str = match endpoint.remote_info(addr_id).await {
                        Some(info) => {
                            let active: Vec<String> = info
                                .addrs()
                                .filter(|a| {
                                    matches!(a.usage(), iroh::endpoint::TransportAddrUsage::Active)
                                })
                                .map(|a| format!("{:?}", a.addr()))
                                .collect();
                            if active.is_empty() {
                                "no_active_paths".to_string()
                            } else {
                                active.join(",")
                            }
                        }
                        None => "unavailable".to_string(),
                    };
                    info!(
                        purpose,
                        attempt = attempt_no,
                        peer = %addr_id.fmt_short(),
                        conn_type = %conn_type_str,
                        "iroh connect selected path (refs UniClipboard#486)"
                    );
                    Ok((attempt_no, connection))
                }
                Ok(Err(err)) => Err((attempt_no, err.to_string())),
                Err(_) => Err((
                    attempt_no,
                    format!("timed out after {}ms", ATTEMPT_TIMEOUT.as_millis()),
                )),
            }
        });
    }

    let mut failures = Vec::new();
    while let Some(joined) = attempts.join_next().await {
        match joined {
            Ok(Ok((attempt, connection))) => {
                if attempt > 1 {
                    debug!(
                        purpose,
                        attempt, "iroh connect recovered on staggered retry"
                    );
                }
                attempts.abort_all();
                return Ok(connection);
            }
            Ok(Err((attempt, err))) => {
                debug!(
                    purpose,
                    attempt,
                    error = %err,
                    "iroh connect attempt failed"
                );
                failures.push(format!("attempt {attempt}: {err}"));
            }
            Err(err) => {
                warn!(purpose, error = %err, "iroh connect attempt task failed");
                failures.push(format!("task failed: {err}"));
            }
        }
    }

    Err(failures.join("; "))
}
