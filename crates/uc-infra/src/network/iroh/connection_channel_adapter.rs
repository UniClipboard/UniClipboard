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

use std::sync::Arc;

use async_trait::async_trait;
use iroh::{Endpoint, EndpointAddr};
use tracing::debug;

use uc_core::ids::DeviceId;
use uc_core::ports::connection_channel::{
    ConnectionChannel, ConnectionChannelPort, ConnectionPath,
};
use uc_core::ports::peer_address::PeerAddressRepositoryPort;

use super::conn_path::derive_path_from_addrs;

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
