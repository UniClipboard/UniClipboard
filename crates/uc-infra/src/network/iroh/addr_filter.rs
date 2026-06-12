//! iroh 地址候选过滤规则的唯一事实来源。
//!
//! **为什么需要这个模块**：虚拟网卡地址（Clash fake-ip 198.18/15、IPv4
//! link-local 169.254/16、CGNAT/Tailscale overlay 100.64/10 与
//! fd7a:115c:a1e0::/48）在三条路径上都得用完全相同的判定 ——
//!
//! 1. `node.rs::build_addr_filter` 决定本端 endpoint 发布哪些地址；
//! 2. `invitation_adapter::serialize_filtered_endpoint_ticket` 决定哪些
//!    地址进 sponsor ticket；
//! 3. `dev pairing addrs / issue --addr` 列出候选给开发者诊断。
//!
//! 任何一条路径独立维护过滤规则都会产生分叉 —— UniClipboard#486 的 Fedora
//! 配对超时就是 sponsor ticket 序列化了一个被 endpoint AddrFilter 滤掉的
//! 地址。把判定集中在这里，让上述三处共享同一份代码，从根上消除分叉的可能。

use std::borrow::Cow;
use std::net::{IpAddr, Ipv4Addr};

use iroh::{EndpointAddr, TransportAddr};
use tracing::{debug, info, warn};

/// A snapshot of one local LAN interface — IP + netmask — captured at
/// endpoint-bind time so the hairpin filter (`apply_addr_filter`) can decide
/// whether a peer candidate IP falls inside one of *our* RFC1918 subnets and
/// is therefore reachable via direct LAN.
///
/// Captured once because [`iroh::address_lookup::AddrFilter`] is fixed at
/// endpoint bind; the user must restart the daemon if they switch wifi /
/// LAN. That matches the existing constraint on `allow_overlay`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalLanV4 {
    pub ip: Ipv4Addr,
    pub netmask: Ipv4Addr,
}

/// 判断某个 IP 是否属于需要过滤的虚拟网卡候选。
pub(crate) fn is_virtual_nic_ip(ip: IpAddr, allow_overlay: bool) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            if (o[0] == 198 && (o[1] & 0xfe) == 18) || (o[0] == 169 && o[1] == 254) {
                return true;
            }
            if !allow_overlay && o[0] == 100 && (o[1] & 0xc0) == 64 {
                return true;
            }
            false
        }
        IpAddr::V6(v6) => {
            let segs = v6.segments();
            if !allow_overlay && segs[0] == 0xfd7a && segs[1] == 0x115c && segs[2] == 0xa1e0 {
                return true;
            }
            false
        }
    }
}

fn should_filter_transport_addr(addr: &TransportAddr, allow_overlay: bool) -> bool {
    match addr {
        TransportAddr::Ip(socket) => is_virtual_nic_ip(socket.ip(), allow_overlay),
        _ => false,
    }
}

/// True if `ip` is an IPv4 candidate that's a public, routable address
/// (excludes RFC1918 private, loopback, link-local, CGNAT, broadcast,
/// documentation, virtual-NIC ranges).
///
/// Used by the hairpin filter to identify candidates that should be dropped
/// when we know the peer is reachable via direct LAN. CGNAT/Tailscale 100.64
/// and Clash 198.18 are handled via `is_virtual_nic_ip(.., allow_overlay=false)`
/// so we re-use that judgment instead of duplicating range checks.
fn is_public_v4(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !v4.is_private()
                && !v4.is_loopback()
                && !v4.is_link_local()
                && !v4.is_broadcast()
                && !v4.is_documentation()
                && !v4.is_unspecified()
                && !is_virtual_nic_ip(IpAddr::V4(v4), /* allow_overlay= */ false)
        }
        IpAddr::V6(_) => false,
    }
}

/// True if `a` and `b` fall in the same IPv4 subnet given `netmask`.
/// Bit-wise: `(a & mask) == (b & mask)`.
fn ips_in_same_v4_subnet(a: Ipv4Addr, b: Ipv4Addr, netmask: Ipv4Addr) -> bool {
    let mask = u32::from(netmask);
    (u32::from(a) & mask) == (u32::from(b) & mask)
}

/// True if at least one peer candidate is an RFC1918 IPv4 that lives inside
/// one of our local LAN subnets — i.e. we can reach the peer directly over
/// the LAN. When this is true, the peer's public-routable IPv4 candidates
/// are *hairpin* artifacts (NAT-reflected public IP that magicsock keeps
/// trying as if it were a separate path). On most consumer routers those
/// paths either fail or succeed with much worse RTT than the LAN path,
/// causing iroh to oscillate between fast LAN (~5ms) and hairpin (~80ms+)
/// during a single transfer.
fn peer_reachable_in_local_lan(addrs: &[TransportAddr], local_lan_v4: &[LocalLanV4]) -> bool {
    if local_lan_v4.is_empty() {
        return false;
    }
    addrs.iter().any(|addr| match addr {
        TransportAddr::Ip(socket) => match socket.ip() {
            IpAddr::V4(peer_v4) if peer_v4.is_private() => local_lan_v4
                .iter()
                .any(|lan| ips_in_same_v4_subnet(peer_v4, lan.ip, lan.netmask)),
            _ => false,
        },
        _ => false,
    })
}

/// Enumerate this host's RFC1918 IPv4 interface addresses (with their
/// netmasks) so the hairpin filter can decide whether a peer candidate
/// falls inside one of our LAN subnets.
///
/// Run once at endpoint-bind time; the result is captured into the
/// `AddrFilter` closure and is not refreshed afterwards. Switching wifi /
/// LAN requires a daemon restart (same constraint as `allow_overlay`).
///
/// Returns an empty vec — and logs a warn — if interface enumeration fails;
/// the hairpin filter then degrades to "never drop public IPs", i.e. the
/// pre-patch behaviour.
pub(crate) fn enumerate_local_lan_v4() -> Vec<LocalLanV4> {
    let ifaces = match if_addrs::get_if_addrs() {
        Ok(ifaces) => ifaces,
        Err(e) => {
            warn!(
                target: "iroh.addr_filter",
                error = %e,
                "if_addrs::get_if_addrs failed; hairpin filter degrades to off"
            );
            return Vec::new();
        }
    };

    let out: Vec<LocalLanV4> = ifaces
        .into_iter()
        .filter(|iface| !iface.is_loopback())
        .filter_map(|iface| match iface.addr {
            if_addrs::IfAddr::V4(v4) if v4.ip.is_private() => Some(LocalLanV4 {
                ip: v4.ip,
                netmask: v4.netmask,
            }),
            _ => None,
        })
        .collect();

    info!(
        target: "iroh.addr_filter",
        count = out.len(),
        subnets = ?out,
        "enumerated local LAN v4 subnets for hairpin filter"
    );
    out
}

fn log_dropped_addrs(dropped: &[String], allow_overlay: bool) {
    if dropped.is_empty() {
        return;
    }
    debug!(
        target: "iroh.addr_filter",
        allow_overlay,
        dropped_count = dropped.len(),
        dropped = ?dropped,
        "filtered virtual-NIC addresses from candidate set",
    );
}

/// 过滤 iroh `AddrFilter` 收到的候选地址集合。
///
/// 两层过滤:
///
/// 1. **虚拟 NIC 过滤** (existing) — Clash fake-ip 198.18/15、IPv4 link-local
///    169.254/16、CGNAT 100.64/10、Tailscale ULA fd7a:115c:a1e0::/48。
///    详见 `is_virtual_nic_ip`。
///
/// 2. **Hairpin public IP 过滤** (新) — 如果 peer 的某个 RFC1918 IP 跟本机
///    任一 LAN 子网同 subnet（即我们能直连 LAN），就丢掉 peer 的所有 public
///    routable IPv4 候选。这避免 iroh magicsock 在快 LAN 路径 (~5ms RTT) 和
///    NAT-hairpin-via-公网 IP 路径 (~80ms+ RTT) 之间来回 oscillate, 把一次
///    transfer 期间的 cwnd 反复打回 slow-start。 对端跨 LAN（任何 RFC1918
///    都不在我们 subnet 内）时不动 public IP, 不破坏 cross-LAN sync。
///
/// 签名形态 `&Vec<TransportAddr> -> Cow<Vec<TransportAddr>>` 由
/// `iroh::address_lookup::AddrFilter::new` 的回调契约决定 —— **不要**
/// clippy-fix 成 `&[TransportAddr]`, 否则会和 iroh 的 `Fn` 签名失配。
pub(crate) fn apply_addr_filter<'a>(
    addrs: &'a Vec<TransportAddr>,
    allow_overlay: bool,
    local_lan_v4: &[LocalLanV4],
) -> Cow<'a, Vec<TransportAddr>> {
    let any_virtual = addrs
        .iter()
        .any(|addr| should_filter_transport_addr(addr, allow_overlay));
    let drop_hairpin = peer_reachable_in_local_lan(addrs, local_lan_v4);

    if !any_virtual && !drop_hairpin {
        return Cow::Borrowed(addrs);
    }

    let kept: Vec<TransportAddr> = addrs
        .iter()
        .filter(|addr| !should_filter_transport_addr(addr, allow_overlay))
        .filter(|addr| {
            // Apply hairpin filter only when peer is LAN-reachable.
            if !drop_hairpin {
                return true;
            }
            match addr {
                TransportAddr::Ip(socket) => !is_public_v4(socket.ip()),
                _ => true,
            }
        })
        .cloned()
        .collect();

    let virtual_dropped: Vec<String> = addrs
        .iter()
        .filter_map(|addr| match addr {
            TransportAddr::Ip(socket) if is_virtual_nic_ip(socket.ip(), allow_overlay) => {
                Some(format!("virtual:{}", socket))
            }
            _ => None,
        })
        .collect();
    let hairpin_dropped: Vec<String> = if drop_hairpin {
        addrs
            .iter()
            .filter_map(|addr| match addr {
                TransportAddr::Ip(socket)
                    if !is_virtual_nic_ip(socket.ip(), allow_overlay)
                        && is_public_v4(socket.ip()) =>
                {
                    Some(format!("hairpin:{}", socket))
                }
                _ => None,
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut all_dropped = virtual_dropped;
    all_dropped.extend(hairpin_dropped);
    log_dropped_addrs(&all_dropped, allow_overlay);

    Cow::Owned(kept)
}

/// 过滤完整的 `EndpointAddr`，用于把本端地址写进可交给远端拨号的 ticket。
pub(crate) fn filter_endpoint_addr(addr: EndpointAddr, allow_overlay: bool) -> EndpointAddr {
    let EndpointAddr { id, addrs } = addr;
    let mut kept = Vec::new();
    let mut dropped = Vec::new();

    for addr in addrs {
        if should_filter_transport_addr(&addr, allow_overlay) {
            if let TransportAddr::Ip(socket) = &addr {
                dropped.push(socket.to_string());
            }
        } else {
            kept.push(addr);
        }
    }

    log_dropped_addrs(&dropped, allow_overlay);
    EndpointAddr::from_parts(id, kept)
}
