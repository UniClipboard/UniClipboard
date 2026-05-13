//! iroh 地址候选过滤规则。
//!
//! 这里是唯一事实来源：节点发布地址、配对码 ticket 生成、测试都应复用同一套
//! 判断，避免不同路径对虚拟网卡地址得出不同结论。

use std::borrow::Cow;
use std::net::IpAddr;

use iroh::{EndpointAddr, TransportAddr};
use tracing::debug;

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
pub(crate) fn apply_addr_filter<'a>(
    addrs: &'a Vec<TransportAddr>,
    allow_overlay: bool,
) -> Cow<'a, Vec<TransportAddr>> {
    let any_virtual = addrs
        .iter()
        .any(|addr| should_filter_transport_addr(addr, allow_overlay));
    if !any_virtual {
        return Cow::Borrowed(addrs);
    }

    let kept: Vec<TransportAddr> = addrs
        .iter()
        .filter(|addr| !should_filter_transport_addr(addr, allow_overlay))
        .cloned()
        .collect();
    let dropped: Vec<String> = addrs
        .iter()
        .filter_map(|addr| match addr {
            TransportAddr::Ip(socket) if is_virtual_nic_ip(socket.ip(), allow_overlay) => {
                Some(socket.to_string())
            }
            _ => None,
        })
        .collect();
    log_dropped_addrs(&dropped, allow_overlay);
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
