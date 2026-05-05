//! Connection channel port (v0.7.0 LAN-only milestone · Phase 96).
//!
//! 给应用层一个**单一真相源**："此时此刻这台已配对设备的活跃连接走的是
//! LAN 直连、公网中继、还是没在线？" 实现侧（infra）通过 iroh
//! `Endpoint::remote_info` snapshot 推导，禁止应用层基于 IP 段自己猜
//! （Tailscale / Clash TUN / Docker bridge 都会让 IP 段判断翻车，参见
//! `uc-infra/src/network/iroh/node.rs` 已有的 `is_virtual_nic_ip` filter）。
//!
//! ## 4 态语义
//!
//! * `Direct` —— 当前活跃 QUIC path 是 LAN 直连（`TransportAddr::Ip`
//!   且 `usage == Active`）
//! * `Relay`  —— 当前活跃 QUIC path 经过公网中继（`TransportAddr::Relay`
//!   且 `usage == Active`）
//! * `Offline` —— 没有任何活跃路径（`remote_info` 返 `None`，或 `addrs()`
//!   为空）
//! * `Unknown` —— 还在握手 / 路径切换中（`remote_info` 已存在但没有
//!   `Active` 路径，仅有 discovery / probe 候选）
//!
//! ## 与 `PresencePort` 的边界
//!
//! `PresencePort` 回答 "对端在不在线"（三态 Online/Offline/Unknown），
//! `ConnectionChannelPort` 回答 "对端如果在线，走的哪条路"（四态）。
//! 两者读同一个 iroh endpoint 的不同切面，应用层各取所需。
//!
//! ## "Out of LAN" 不在本 port 里
//!
//! "Out of LAN" 灰态是 `channel + (network.allow_relay_fallback == false)`
//! 的合成态：在 LAN-only Mode = ON 且对端 channel ∈ {Relay, Offline}
//! 时，UI 层把它渲染成 "Out of LAN" 提示，**而不是**让 infra 生造一个
//! 第五个枚举值。这样 infra 不需要读 settings，channel 判定保持纯 iroh
//! 状态读出。

use async_trait::async_trait;

use crate::ids::DeviceId;

/// 连接通道 4 态。Phase 96 INDIC-01 需求边界:UI 必须显式可见这 4 态,
/// 不允许把 `Unknown` 默认渲染为 `Direct` / `Relay`(Pitfall 4)。
///
/// `Default = Unknown` 是显式选择 —— 任何未 probe / 握手中的 device 在
/// 视觉上必须能与 LAN/Relay/Offline 区分，避免 "对端真在中继但 UI 显示
/// LAN" 的口碑炸点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionChannel {
    /// LAN 直连(当前活跃 QUIC path 走 IP socket)。
    Direct,
    /// 公网中继(当前活跃 QUIC path 走 iroh relay)。
    Relay,
    /// 无活跃连接(对端离线 / 从未拨号 / 已断开)。
    Offline,
    /// 还在握手 / 路径切换 / 候选 probing 中。
    Unknown,
}

impl Default for ConnectionChannel {
    fn default() -> Self {
        // 显式默认 Unknown —— 切勿改为 Direct / Relay。任何"还没看清楚"
        // 的连接渲染成具体态都会让用户对 LAN-only Mode 的开关效果产生
        // 误判。
        ConnectionChannel::Unknown
    }
}

/// 单一真相源:从 infra 层读出"对端当前走的是哪条路"。
///
/// 实现契约（infra 层 `IrohConnectionChannelAdapter` 落地）:
///
/// * 必须基于 `Endpoint::remote_info` snapshot 推导,不允许查 cache /
///   IP 段。
/// * `Active` 路径的 `Ip(...)` ⇒ `Direct`,`Relay(...)` ⇒ `Relay`。
/// * 同时存在多条 `Active` 时优先级:`Direct > Relay`(LAN 直连一旦
///   建立就是当前真实流量路径,relay 仅作 fallback 候选)。
/// * `remote_info == None` 或 `addrs()` 全空 ⇒ `Offline`。
/// * 仅有 `Inactive` / discovery / probe 候选 ⇒ `Unknown`。
#[async_trait]
pub trait ConnectionChannelPort: Send + Sync {
    /// 读取某台已配对设备当前的连接通道。**不发起拨号**——纯 endpoint
    /// 状态读出,UI 高频轮询安全。
    async fn channel_for(&self, device: &DeviceId) -> ConnectionChannel;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_unknown() {
        // 防御性测试:`ConnectionChannel::default()` 必须永远是 Unknown。
        // 任何把 Default 改成具体态的 PR 应该被本测试拦下。
        assert_eq!(ConnectionChannel::default(), ConnectionChannel::Unknown);
    }
}
