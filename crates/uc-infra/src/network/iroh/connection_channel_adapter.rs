//! Iroh-backed implementation of [`ConnectionChannelPort`]
//! (v0.7.0 LAN-only milestone · Phase 96 INDIC-01).
//!
//! ## 真相源：委托 `conn_path::path_for`
//!
//! 与 `presence_adapter.rs` 不同 ——本 adapter **不持有连接**、**不发拨号**、
//! **不订阅事件**。`channel_for(device)` 被调用时它只做两件事：
//!
//! 1. 从 `peer_addr_repo` 拿 `addr_blob` 解码出 `EndpointAddr.id`（iroh
//!    `EndpointId`）。
//! 2. 交给 `conn_path::path_for(endpoint, id, OnMissing::Offline)` —— 由它
//!    探测 `remote_info` snapshot 并裁决 Direct / Relay / Unknown / Offline。
//!
//! 裁决逻辑（`Direct > Relay` 优先级、`Active` 与候选判定、不可用地址过滤）
//! 和 per-sync 埋点共用同一份 `conn_path`，单一真相源——细节看那个模块，
//! 不在此重复。本处选 `OnMissing::Offline`：没有 `RemoteInfo` 意味着 magicsock
//! 从未观察到该 peer。
//!
//! 一处仍需当心：`conn_path` 的不可用地址过滤**只影响显示判定**，与节点级
//! `addr_filter::is_virtual_nic_ip`（决定 outbound dial / ticket 候选）是
//! 两套有意分叉的策略，别合并——尤其 IPv6 链路本地的处理两边不同。
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
use uc_core::ports::connection_channel::{ConnectionChannelPort, ConnectionPath};
use uc_core::ports::peer_address::PeerAddressRepositoryPort;

use super::conn_path::{path_for, OnMissing};

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

        // Step 2: snapshot magicsock state and classify. No `RemoteInfo` at
        // all ⇒ peer never observed by magicsock ⇒ Offline (see `OnMissing`).
        path_for(&self.endpoint, endpoint_addr.id, OnMissing::Offline).await
    }
}
