use local_ip_address::{list_afinet_netifas, local_ip};
use sha2::{Digest, Sha256};

use crate::domain::network::NetworkInterface;

pub fn string_to_32_bytes(input: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().into()
}

pub fn generate_device_id() -> String {
    // 生成6位随机数字
    let random_number = rand::random::<u32>() % 1000000;
    format!("{:06}", random_number)
}

/// 检查 IP 地址是否有效
pub fn is_valid_ip(ip: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    for part in parts {
        if part.parse::<u8>().is_err() {
            return false;
        }
    }
    true
}

/// 检查端口是否有效
pub fn is_valid_port(port: u16) -> bool {
    port >= 1024
}

/// 获取以太网 IP 地址或 WiFi IP 地址
pub fn get_local_ip() -> String {
    match local_ip() {
        Ok(ip) => ip.to_string(),
        Err(e) => {
            // 记录错误并返回一个默认值
            eprintln!("获取本地 IP 地址时出错: {}", e);
            "127.0.0.1".to_string()
        }
    }
}

/// 获取首选的本地网络地址用于 P2P 监听
///
/// 优先返回非回环的 IPv4 地址，如果不存在则返回 0.0.0.0
pub fn get_preferred_local_address() -> String {
    match local_ip() {
        Ok(ip) => {
            // local_ip_address 库会自动选择合适的接口
            ip.to_string()
        }
        Err(_) => {
            log::warn!("Failed to get local IP, falling back to 0.0.0.0");
            "0.0.0.0".to_string()
        }
    }
}

/// 获取用于 QUIC 监听的物理网卡 IP
///
/// QUIC 不能绑定 0.0.0.0（在有多网卡/VPN 环境下会失败）
/// 必须明确绑定到物理 LAN IP
///
/// 过滤规则：
/// - 排除 loopback (127.*)
/// - 排除 tunnel/utun 接口（如 Clash 的 198.18.*）
/// - 排除 link-local (169.254.*)
/// - 只保留 IPv4 私网地址 (10.*, 172.16-31.*, 192.168.*)
pub fn get_physical_lan_ip() -> Option<String> {
    use std::net::IpAddr;

    match list_afinet_netifas() {
        Ok(interfaces) => {
            for (iface_name, ip) in interfaces {
                if let IpAddr::V4(v4) = ip {
                    // 排除 loopback
                    if v4.is_loopback() {
                        continue;
                    }

                    // 排除 link-local
                    if v4.is_link_local() {
                        continue;
                    }

                    // 排除 tunnel 接口（utun, tun, tap）
                    if iface_name.contains("utun")
                        || iface_name.contains("tun")
                        || iface_name.contains("tap")
                    {
                        continue;
                    }

                    // 排除 Clash TUN 模式的地址范围 (198.18.0.0/15)
                    if v4.octets()[0] == 198 && v4.octets()[1] >= 18 {
                        continue;
                    }

                    // 只保留 IPv4 私网地址
                    if is_private_ipv4(v4) {
                        log::info!("Found physical LAN IP: {} (interface: {})", v4, iface_name);
                        return Some(v4.to_string());
                    }
                }
            }
            log::warn!("No suitable physical LAN IP found for QUIC");
            None
        }
        Err(e) => {
            log::error!("Failed to enumerate network interfaces: {}", e);
            None
        }
    }
}

/// 检查是否为 IPv4 私网地址
fn is_private_ipv4(ip: std::net::Ipv4Addr) -> bool {
    let octets = ip.octets();
    match octets[0] {
        10 => true,                                // 10.0.0.0/8
        172 => octets[1] >= 16 && octets[1] <= 31, // 172.16.0.0/12
        192 => octets[1] == 168,                   // 192.168.0.0/16
        _ => false,
    }
}

/// 获取当前平台
pub fn get_current_platform() -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    format!("{} {}", os, arch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_to_32_bytes() {
        let input = "test";
        let output = string_to_32_bytes(input);
        assert_eq!(output.len(), 32);
    }

    #[test]
    fn test_generate_device_id() {
        let id = generate_device_id();
        assert_eq!(id.len(), 6);
    }

    #[test]
    fn test_is_valid_ip() {
        assert!(is_valid_ip("192.168.1.1"));
        assert!(is_valid_ip("0.0.0.0"));
        assert!(is_valid_ip("255.255.255.255"));
        assert!(!is_valid_ip("256.256.256.256"));
        assert!(!is_valid_ip("192.168.1"));
        assert!(!is_valid_ip("192.168.1.1.1"));
        assert!(!is_valid_ip("192.168.1.a"));
        assert!(!is_valid_ip("..."));
    }

    #[test]
    fn test_is_valid_port() {
        assert!(!is_valid_port(0));
        assert!(!is_valid_port(1023));
        assert!(is_valid_port(1024));
        assert!(is_valid_port(8080));
        assert!(is_valid_port(65535));
    }

    #[test]
    fn test_get_local_ip() {
        let ip = get_local_ip();
        println!("local ip: {}", ip);
        assert!(is_valid_ip(&ip));
    }
}
