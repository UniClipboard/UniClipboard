use local_ip_address::list_afinet_netifas;
use std::net::{IpAddr, Ipv4Addr};
use tracing::{debug, warn};

/// Detect the best physical LAN IPv4 address for libp2p to listen on.
///
/// 检测最佳的物理局域网 IPv4 地址，供 libp2p 监听使用。
///
/// # Filtering rules / 过滤规则
/// - Exclude loopback (127.*)
/// - Exclude link-local (169.254.*)
/// - Exclude tunnel interfaces (utun, tun, tap)
/// - Exclude Clash TUN addresses (198.18.0.0/15)
/// - Only keep private IPv4 addresses (10.*, 172.16-31.*, 192.168.*)
pub fn get_physical_lan_ip() -> Option<Ipv4Addr> {
    let interfaces = match list_afinet_netifas() {
        Ok(ifaces) => ifaces,
        Err(e) => {
            warn!(error = %e, "failed to enumerate network interfaces");
            return None;
        }
    };

    for (iface_name, ip) in interfaces {
        if is_loopback_interface(&iface_name) {
            debug!(interface = %iface_name, "skip loopback interface for p2p listen");
            continue;
        }

        if let IpAddr::V4(v4) = ip {
            if v4.is_loopback() || v4.is_link_local() {
                debug!(interface = %iface_name, ip = %v4, "skip non-routable interface address");
                continue;
            }

            if is_tunnel_interface(&iface_name) {
                debug!(interface = %iface_name, ip = %v4, "skip tunnel interface for p2p listen");
                continue;
            }

            if is_clash_tun_address(v4) {
                debug!(interface = %iface_name, ip = %v4, "skip clash tun address for p2p listen");
                continue;
            }

            if is_private_ipv4(v4) {
                debug!(ip = %v4, interface = %iface_name, "detected physical LAN IP");
                return Some(v4);
            }

            debug!(interface = %iface_name, ip = %v4, "skip non-private ipv4 for p2p listen");
        }
    }

    warn!("no suitable physical LAN IP found");
    None
}

fn is_loopback_interface(name: &str) -> bool {
    name == "lo" || name.starts_with("lo")
}

fn is_tunnel_interface(name: &str) -> bool {
    name.contains("utun") || name.contains("tun") || name.contains("tap")
}

fn is_clash_tun_address(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 198 && octets[1] >= 18
}

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    match octets[0] {
        10 => true,
        172 => (16..=31).contains(&octets[1]),
        192 => octets[1] == 168,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_is_not_private() {
        assert!(!is_private_ipv4(Ipv4Addr::LOCALHOST));
    }

    #[test]
    fn private_ranges_detected() {
        assert!(is_private_ipv4(Ipv4Addr::new(10, 0, 0, 1)));
        assert!(is_private_ipv4(Ipv4Addr::new(172, 16, 0, 1)));
        assert!(is_private_ipv4(Ipv4Addr::new(172, 31, 255, 254)));
        assert!(is_private_ipv4(Ipv4Addr::new(192, 168, 1, 1)));
    }

    #[test]
    fn non_private_rejected() {
        assert!(!is_private_ipv4(Ipv4Addr::new(8, 8, 8, 8)));
        assert!(!is_private_ipv4(Ipv4Addr::new(172, 32, 0, 1)));
        assert!(!is_private_ipv4(Ipv4Addr::new(192, 169, 1, 1)));
    }

    #[test]
    fn clash_tun_addresses_detected() {
        assert!(is_clash_tun_address(Ipv4Addr::new(198, 18, 0, 1)));
        assert!(is_clash_tun_address(Ipv4Addr::new(198, 19, 0, 1)));
        assert!(!is_clash_tun_address(Ipv4Addr::new(198, 17, 0, 1)));
        assert!(!is_clash_tun_address(Ipv4Addr::new(192, 168, 1, 1)));
    }

    #[test]
    fn tunnel_interfaces_detected() {
        assert!(is_tunnel_interface("utun0"));
        assert!(is_tunnel_interface("utun3"));
        assert!(is_tunnel_interface("tun0"));
        assert!(is_tunnel_interface("tap0"));
        assert!(!is_tunnel_interface("en0"));
        assert!(!is_tunnel_interface("eth0"));
        assert!(!is_tunnel_interface("wlan0"));
    }

    #[test]
    fn get_physical_lan_ip_returns_some_on_dev_machine() {
        // This test validates the function runs without panic.
        // On CI without a LAN, it may return None — that's acceptable.
        let _result = get_physical_lan_ip();
    }
}
