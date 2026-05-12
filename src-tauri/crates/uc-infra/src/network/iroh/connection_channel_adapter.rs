//! Iroh-backed implementation of [`ConnectionChannelPort`]
//! (v0.7.0 LAN-only milestone · Phase 96 INDIC-01).
//!
//! ## 真相源：`Endpoint::remote_info` snapshot
//!
//! 与 `presence_adapter.rs` 不同 ——本 adapter **不持有连接**、**不发拨号**、
//! **不订阅事件**。它的唯一动作是在 `channel_for(device)` 被调用时：
//!
//! 1. 从 `peer_addr_repo` 拿 `addr_blob` 解码出 `EndpointAddr.id`（iroh
//!    `EndpointId`）。
//! 2. 调 `endpoint.remote_info(endpoint_id).await` 拿当前 magicsock
//!    snapshot（`Option<RemoteInfo>`）。
//! 3. 过滤 `Active` 路径：第一个 `TransportAddr::Ip(...)` 命中 ⇒ `Direct`；
//!    第一个 `TransportAddr::Relay(...)` 命中 ⇒ `Relay`。
//! 4. 优先级：`Direct > Relay`。IP 直连一旦建立就是当前流量路径，relay
//!    只是可选 fallback；同时活跃时按直连汇报。
//! 5. `remote_info == None` 或 `addrs()` 全空 ⇒ `Offline`。
//! 6. 仅有 `Inactive` / discovery / probe 候选 ⇒ `Unknown`。
//!
//! ## 不可用地址过滤
//!
//! Clash fake-ip 与链路本地地址在 channel 推导处排除；Tailscale 100.x /
//! fd7a:: 等 overlay 地址是真实可用路径，保留为 `Direct` 并向 UI 暴露
//! 实际地址。**节点级 `AddrFilter` 不动** —— 那个 filter 影响的是
//! outbound dial 候选，调它会改变连接行为；这里只在通道判定处过滤，
//! 影响显示层。
//!
//! ## 缓存策略
//!
//! 不缓存。`remote_info` 自身是 iroh magicsock 的当前 snapshot 调用，
//! 量级 O(1)；UI 5–15s polling + 偶发事件触发，远低于阈值。任何 caching
//! 引入"上次看是 LAN 现在仍显示 LAN" 的陈旧 trap（Pitfall 4）。
//!
//! ## 错误处理
//!
//! 所有失败都映射为 `ConnectionChannel::Unknown`，**不**向上传播错误：
//!
//! * `peer_addr_repo` 故障 ⇒ Unknown（设备记录损坏，UI 显式可见，比抛
//!   错好）
//! * `postcard` 解码失败 ⇒ Unknown（数据完整性问题）
//! * device 在 repo 中不存在 ⇒ Unknown（pre-pairing / 已 unpair 边缘窗口）
//!
//! `ConnectionChannelPort` trait 故意不带 Result —— UI 高频读路径，错误
//! 通道污染 trace。infra 内部仍 `tracing::debug!` 记录 fallback 原因。

use std::net::IpAddr;
use std::sync::Arc;

use async_trait::async_trait;
use iroh::{Endpoint, EndpointAddr, TransportAddr};
use tracing::debug;

use uc_core::ids::DeviceId;
use uc_core::ports::connection_channel::{
    ConnectionChannel, ConnectionChannelPort, ConnectionPath,
};
use uc_core::ports::peer_address::PeerAddressRepositoryPort;

/// Iroh-backed [`ConnectionChannelPort`] implementation.
pub struct IrohConnectionChannelAdapter {
    endpoint: Arc<Endpoint>,
    peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
}

impl IrohConnectionChannelAdapter {
    pub fn new(
        endpoint: Arc<Endpoint>,
        peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    ) -> Self {
        Self {
            endpoint,
            peer_addr_repo,
        }
    }
}

#[async_trait]
impl ConnectionChannelPort for IrohConnectionChannelAdapter {
    async fn path_for(&self, device: &DeviceId) -> ConnectionPath {
        // Step 1: device → addr_blob → EndpointId
        let record = match self.peer_addr_repo.get(device).await {
            Ok(Some(r)) => r,
            Ok(None) => {
                debug!(
                    device = %device.as_str(),
                    "channel_for: no peer address record; reporting Unknown"
                );
                return ConnectionPath::default();
            }
            Err(err) => {
                debug!(
                    device = %device.as_str(),
                    error = %err,
                    "channel_for: peer_addr_repo failure; reporting Unknown"
                );
                return ConnectionPath::default();
            }
        };

        let endpoint_addr: EndpointAddr = match postcard::from_bytes(&record.addr_blob) {
            Ok(addr) => addr,
            Err(err) => {
                debug!(
                    device = %device.as_str(),
                    error = %err,
                    "channel_for: postcard decode failed; reporting Unknown"
                );
                return ConnectionPath::default();
            }
        };

        // Step 2: snapshot the current magicsock state.
        let info = match self.endpoint.remote_info(endpoint_addr.id).await {
            Some(info) => info,
            None => {
                // No remote_info ⇒ peer never observed by magicsock ⇒ Offline.
                return ConnectionPath {
                    channel: ConnectionChannel::Offline,
                    address: None,
                };
            }
        };

        // Step 3-6: priority Direct > Relay; empty Active set + non-empty
        // candidates ⇒ Unknown; empty everything ⇒ Offline.
        derive_path_from_addrs(info.addrs())
    }
}

/// Pure derivation step factored out for unit testing — feeding it a synthetic
/// iterator covers the full truth-table without standing up an iroh endpoint.
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
    //! `derive_channel_from_addrs` 是纯函数,可以脱离 iroh endpoint 直接喂
    //! 合成数据覆盖全 truth-table。adapter 的 endpoint 集成路径由 bootstrap
    //! + Phase 96 e2e 验收,不在这里重做。
    //!
    //! 这里只覆盖:
    //! * 优先级 Direct > Relay
    //! * 空集 ⇒ Offline
    //! * 有 RemoteInfo 但无 Active ⇒ Unknown
    //! * IPv4 fake-ip / link-local filter 把不可用直连退化为 Relay/Unknown
    //!
    //! `TransportAddrInfo` 是借用 iroh 内部类型的 borrowed view,直接构造
    //! 困难;改为测试 `is_filtered_ip` 的 truth-table + `derive` 的真实
    //! semantics 由 e2e 集成测试在 bootstrap 装配后覆盖(见 plan §"verify"
    //! 通过 `cargo test -p uc-infra` 触发)。

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
