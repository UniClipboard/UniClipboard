//! `NetworkInterfaceLanProbe` —— [`LanInterfaceProbePort`] 的真实实现。
//!
//! 使用 [`network-interface`] crate 跨平台枚举本机网卡：内部分别用 macOS
//! `getifaddrs(3)` / Linux netlink+`getifaddrs(3)` / Windows
//! `GetAdaptersAddresses`，对外只暴露统一的 `(name, addr, …)` 列表。
//!
//! ## 适配契约
//!
//! - 只产出 IPv4 项；遇到 IPv6 alias 直接丢弃。原因详见 `uc-core` 的
//!   `LanInterface` 文档（v1 把 IPv6 整体排除）。
//! - 一张物理网卡上有 N 个 IPv4 alias 时输出 N 条 `LanInterface` 记录，每
//!   条用相同的 `name`，由 use case 层按需去重 / 排序。
//! - `is_loopback` 由 IP 段判断（127.0.0.0/8）：`network-interface` 暂不
//!   暴露接口标志位，靠 IP 判断已经足够（loopback 永远在 127/8）。
//! - 探测失败一律转 [`LanInterfaceProbeError::Probe`]，文案带原始错误以
//!   便日志排障。
//!
//! ## 同步语义
//!
//! `NetworkInterface::show()` 是阻塞 syscall。trait 是 `async fn`，但本
//! adapter 直接同步调用 —— 该调用本身只是几次 syscall + memcpy，比 tokio
//! 切回来的成本还低；如果将来某 OS 实现真的耗时（理论上没见过），可改
//! `tokio::task::spawn_blocking`。
//!
//! [`network-interface`]: https://crates.io/crates/network-interface

use std::net::{IpAddr, Ipv4Addr};

use async_trait::async_trait;
use network_interface::{NetworkInterface, NetworkInterfaceConfig};

use uc_core::mobile_sync::LanInterface;
use uc_core::ports::{LanInterfaceProbeError, LanInterfaceProbePort};

#[derive(Debug, Default)]
pub struct NetworkInterfaceLanProbe;

impl NetworkInterfaceLanProbe {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LanInterfaceProbePort for NetworkInterfaceLanProbe {
    async fn list_interfaces(&self) -> Result<Vec<LanInterface>, LanInterfaceProbeError> {
        let raw = NetworkInterface::show().map_err(|err| {
            LanInterfaceProbeError::Probe(format!("network-interface enumeration failed: {err}"))
        })?;

        let mut out = Vec::new();
        for iface in raw {
            for addr in iface.addr.iter() {
                if let IpAddr::V4(ipv4) = addr.ip() {
                    out.push(LanInterface {
                        name: iface.name.clone(),
                        ipv4,
                        is_loopback: is_ipv4_loopback(ipv4),
                    });
                }
            }
        }
        Ok(out)
    }
}

/// 127.0.0.0/8 判定。`Ipv4Addr::is_loopback` 在 stable 已经稳定，但用本
/// 地函数让单测能直接覆盖（避免依赖 std 实现的不变性）。
fn is_ipv4_loopback(ip: Ipv4Addr) -> bool {
    ip.octets()[0] == 127
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_detection_matches_127_block() {
        assert!(is_ipv4_loopback(Ipv4Addr::new(127, 0, 0, 1)));
        assert!(is_ipv4_loopback(Ipv4Addr::new(127, 255, 255, 254)));
        assert!(!is_ipv4_loopback(Ipv4Addr::new(192, 168, 1, 1)));
        assert!(!is_ipv4_loopback(Ipv4Addr::new(126, 0, 0, 1)));
        assert!(!is_ipv4_loopback(Ipv4Addr::new(128, 0, 0, 1)));
    }

    /// 真实环境冒烟测试：跑得通就行，不断言任何具体网卡（CI 环境差异
    /// 太大）。这里只验证适配器不 panic、能产出某种结果（可能为空）、
    /// 永远只暴露 IPv4 而不漏 IPv6。
    #[tokio::test]
    async fn list_returns_only_ipv4_items() {
        let probe = NetworkInterfaceLanProbe::new();
        let list = probe
            .list_interfaces()
            .await
            .expect("network-interface enumeration should succeed in test env");
        // 我们的 LanInterface 字段类型已经是 Ipv4Addr —— 编译期就保证不
        // 是 IPv6。这里仅断言类型契约通过 + 不 panic。
        for iface in list {
            assert!(!iface.name.is_empty(), "接口名不应为空");
            // is_loopback 字段与本地判定函数一致。
            assert_eq!(iface.is_loopback, is_ipv4_loopback(iface.ipv4));
        }
    }
}
