use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr, TransportAddr};
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
const STAGGERED_DELAYS: [Duration; 3] = [
    Duration::from_millis(0),
    Duration::from_secs(2),
    Duration::from_secs(5),
];

/// LAN-only Mode 反向防御：本端 `RelayMode::Disabled` **不阻止** iroh 用对端
/// `EndpointAddr` 中的 `relay_url` 发起出站连接。已配对 peer 的 addr blob 在
/// `to_persistable_addr` 阶段被收敛为"NodeId + Relay 提示"，所以 LAN-only
/// 用户复制内容触发 dispatch 时仍会看到日志：
///   `iroh::endpoint: connecting relay_url=Some(...) ip_addresses=[]`
///
/// 当 [`super::runtime_consts::lan_only`] 为 `true` 时剥掉 `TransportAddr::Relay`
/// 项，强制 iroh 只能走 mDNS 重新解析的直连地址。如果对端不在同一子网
/// （mDNS 不可达），connect 自然失败 —— 这正是 LAN-only 的设计意图。
///
/// 非 LAN-only 路径下零开销直接返回原 addr（一次 `Vec::iter().any` 短路）。
pub(super) fn strip_relay_if_lan_only(addr: EndpointAddr) -> EndpointAddr {
    if !super::runtime_consts::lan_only() {
        return addr;
    }
    let EndpointAddr { id, addrs } = addr;
    let kept = addrs
        .into_iter()
        .filter(|a| !matches!(a, TransportAddr::Relay(_)));
    EndpointAddr::from_parts(id, kept)
}

pub(super) async fn connect_with_staggered_retry(
    endpoint: Arc<Endpoint>,
    addr: EndpointAddr,
    alpn: &'static [u8],
    purpose: &'static str,
) -> Result<Connection, String> {
    let addr = strip_relay_if_lan_only(addr);
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
