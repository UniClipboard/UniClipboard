//! Single owner of "given an iroh `EndpointId`, what connection path is this
//! peer on right now?".
//!
//! [`path_for`] is the one place that probes `endpoint.remote_info(id)` and
//! turns the snapshot into a [`ConnectionPath`] verdict (direct / relay /
//! unknown / offline). Both the connection-channel adapter (UI polling) and
//! the clipboard dispatch adapter (per-sync transport telemetry) call it, so
//! the verdict — and the `remote_info` probe itself — lives here once instead
//! of being re-derived at each site. The only thing the two callers vary is
//! the [`OnMissing`] policy for "the endpoint has no `RemoteInfo` at all".
//!
//! The classification step [`derive_path_from_addrs`] is a pure function over
//! a borrowed `TransportAddrInfo` iterator, kept private so the probe can't be
//! bypassed; its IP truth-table is unit-testable without standing up an
//! endpoint.

use std::net::IpAddr;

use iroh::{Endpoint, EndpointId, TransportAddr};

use uc_core::ports::connection_channel::{ConnectionChannel, ConnectionPath};

/// What [`path_for`] reports when `remote_info(id)` returns `None` — the
/// endpoint has no [`iroh::endpoint::RemoteInfo`] for the peer at all. This is
/// the *only* point the two callers legitimately diverge, so it is an explicit
/// parameter rather than a default buried in each call site:
///
/// - [`OnMissing::Offline`] — UI device list: no `RemoteInfo` means magicsock
///   has never observed this peer, so it is genuinely offline.
/// - [`OnMissing::Unknown`] — post-dispatch telemetry probe: the send just
///   succeeded, so the peer is reachable; a `None` only means the snapshot
///   momentarily lags the just-settled dial, not "offline".
#[derive(Debug, Clone, Copy)]
pub(crate) enum OnMissing {
    Offline,
    Unknown,
}

impl OnMissing {
    fn channel(self) -> ConnectionChannel {
        match self {
            OnMissing::Offline => ConnectionChannel::Offline,
            OnMissing::Unknown => ConnectionChannel::Unknown,
        }
    }
}

/// Probe `endpoint` for the peer's current [`ConnectionPath`].
///
/// Snapshots `remote_info(id)` once and classifies it via
/// [`derive_path_from_addrs`]; falls back to `on_missing` when there is no
/// `RemoteInfo` for the peer (see [`OnMissing`]). Cheap — `remote_info` is a
/// snapshot read, not a watcher subscription — so callers sample it at the
/// exact moment they care about (UI poll tick / right after a send settles).
pub(crate) async fn path_for(
    endpoint: &Endpoint,
    id: EndpointId,
    on_missing: OnMissing,
) -> ConnectionPath {
    match endpoint.remote_info(id).await {
        Some(info) => derive_path_from_addrs(info.addrs()),
        None => ConnectionPath {
            channel: on_missing.channel(),
            address: None,
        },
    }
}

/// Derive the active [`ConnectionPath`] from a peer's `remote_info` address
/// snapshot.
///
/// Priority `Direct > Relay`: an established IP path is the route traffic
/// actually takes, so when both are `Active` it is reported as `Direct`
/// (relay is only a fallback candidate). Having a `RemoteInfo` but no
/// `Active` path ⇒ `Unknown` (mid-handshake / probing); nothing known ⇒
/// `Offline`.
fn derive_path_from_addrs<'a, I>(addrs: I) -> ConnectionPath
where
    I: IntoIterator<Item = &'a iroh::endpoint::TransportAddrInfo>,
{
    let mut saw_any = false;
    let mut active_direct: Option<String> = None;
    let mut active_relay: Option<String> = None;

    for a in addrs {
        saw_any = true;
        match (a.usage(), a.addr()) {
            (iroh::endpoint::TransportAddrUsage::Active, TransportAddr::Ip(s)) => {
                if !is_filtered_ip(s.ip()) && active_direct.is_none() {
                    active_direct = Some(s.to_string());
                }
            }
            (iroh::endpoint::TransportAddrUsage::Active, TransportAddr::Relay(u)) => {
                if active_relay.is_none() {
                    active_relay = Some(u.to_string());
                }
            }
            // Inactive / discovery candidates: do not promote, but keep
            // `saw_any = true` so the empty-set tail returns Offline only
            // when literally nothing is known.
            _ => {}
        }
    }

    if let Some(address) = active_direct {
        // 多条同时活跃时优先汇报 Direct —— IP 直连一旦建立就是当前流量
        // 路径,relay 退化为可选 fallback。
        ConnectionPath {
            channel: ConnectionChannel::Direct,
            address: Some(address),
        }
    } else if let Some(address) = active_relay {
        ConnectionPath {
            channel: ConnectionChannel::Relay,
            address: Some(address),
        }
    } else if saw_any {
        // 有 RemoteInfo 但没有 Active 路径 ⇒ 还在握手 / probe
        ConnectionPath {
            channel: ConnectionChannel::Unknown,
            address: None,
        }
    } else {
        ConnectionPath {
            channel: ConnectionChannel::Offline,
            address: None,
        }
    }
}

/// 仅过滤不适合对用户展示为可用直连的地址。Tailscale / overlay 地址保留,
/// 这样设备列表能显示实际走的 100.x / fd7a:: 路径。**仅在 channel 判定处
/// 过滤**,不影响 outbound dial 候选(那个是 `node.rs` `AddrFilter` 的职责)。
fn is_filtered_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            // 与 node.rs::is_virtual_nic_ip 同步的 IPv4 假段(Clash /
            // link-local) —— UI 不应当把这些 path 当成可用直连:
            // Clash 198.18.0.0/15 是劫持 fake-ip, 169.254/16 link-local
            // 仅本机有意义。Tailscale 100.64.0.0/10 是真实 overlay 路径,
            // 应当作为 Direct 展示。
            (o[0] == 198 && (o[1] & 0xfe) == 18) || (o[0] == 169 && o[1] == 254)
        }
        IpAddr::V6(v6) => {
            let segs = v6.octets();
            // fe80::/10 link-local
            let is_link_local = segs[0] == 0xfe && (segs[1] & 0xc0) == 0x80;
            is_link_local
        }
    }
}

#[cfg(test)]
mod tests {
    //! `is_filtered_ip` 是纯函数,直接喂合成数据覆盖全 truth-table。
    //! `derive_path_from_addrs` 的真实 semantics 由 bootstrap + Phase 96
    //! e2e 验收覆盖 —— `TransportAddrInfo` 是借用 iroh 内部类型的 borrowed
    //! view,直接构造困难,不在这里重做。

    use super::*;
    use std::net::Ipv4Addr;
    use std::net::Ipv6Addr;

    #[test]
    fn ipv4_filter_truth_table() {
        // Real LAN — 不过滤
        assert!(!is_filtered_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10))));
        assert!(!is_filtered_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))));
        assert!(!is_filtered_ip(IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1))));
        // Clash fake-ip 198.18.0.0/15
        assert!(is_filtered_ip(IpAddr::V4(Ipv4Addr::new(198, 18, 0, 1))));
        assert!(is_filtered_ip(IpAddr::V4(Ipv4Addr::new(198, 19, 255, 254))));
        // CGNAT / Tailscale 100.64.0.0/10 是真实 IP 直连路径,不应过滤。
        assert!(!is_filtered_ip(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 1))));
        assert!(!is_filtered_ip(IpAddr::V4(Ipv4Addr::new(
            100, 127, 255, 254
        ))));
        // 100.63 / 100.128 不在 /10 内
        assert!(!is_filtered_ip(IpAddr::V4(Ipv4Addr::new(100, 63, 0, 1))));
        assert!(!is_filtered_ip(IpAddr::V4(Ipv4Addr::new(100, 128, 0, 1))));
        // link-local 169.254/16
        assert!(is_filtered_ip(IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1))));
    }

    #[test]
    fn ipv6_filter_covers_ula_and_link_local() {
        // fe80::/10 link-local
        assert!(is_filtered_ip(IpAddr::V6(Ipv6Addr::new(
            0xfe80, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(is_filtered_ip(IpAddr::V6(Ipv6Addr::new(
            0xfebf, 0, 0, 0, 0, 0, 0, 1
        ))));
        // fec0 不属于 fe80::/10(高 10 bit 是 0xfec, 不等于 0xfe80..0xfebf)
        assert!(!is_filtered_ip(IpAddr::V6(Ipv6Addr::new(
            0xfec0, 0, 0, 0, 0, 0, 0, 1
        ))));
        // fc00::/7 ULA(fc00..fdff) 可能是 Tailscale 等真实 overlay 直连路径。
        assert!(!is_filtered_ip(IpAddr::V6(Ipv6Addr::new(
            0xfc00, 0, 0, 0, 0, 0, 0, 1
        ))));
        assert!(!is_filtered_ip(IpAddr::V6(Ipv6Addr::new(
            0xfd99, 0, 0, 0, 0, 0, 0, 1
        ))));
        // fe00 不属于 fc00::/7
        assert!(!is_filtered_ip(IpAddr::V6(Ipv6Addr::new(
            0xfe00, 0, 0, 0, 0, 0, 0, 1
        ))));
        // 普通全球可路由 IPv6 — 不过滤
        assert!(!is_filtered_ip(IpAddr::V6(Ipv6Addr::new(
            0x2001, 0xdb8, 0, 0, 0, 0, 0, 1
        ))));
    }
}
