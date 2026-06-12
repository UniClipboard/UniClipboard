//! Slice 2 Phase 1 · T7 — `MemberRosterFacade`.
//!
//! 对 UI / CLI 暴露已配对成员的列表,并给每条成员打上 presence 状态
//! (online / offline / unknown)和本机标记 (`is_local`)。是个典型的 thin
//! facade——不主动拨号、不管 rename / revoke、不做跨 use case 编排。主动
//! 拨号由 T6 `EnsureReachableAllUseCase` 负责,在
//! `SpaceSetupFacade::auto_start_network` (T8) 成功后一次性触发。
//!
//! ## 为什么 `list_with_presence` 不拨号
//!
//! 查询路径被频繁调用(CLI `members` 每次都会跑,未来 GUI 会做轮询或
//! subscribe)。如果每次查询都调 `presence.ensure_reachable()`,每个 peer
//! 都会产生一次 iroh dial,浪费带宽且让 UI 响应慢。`presence.current_state`
//! 是纯缓存读,O(1) 不触发 IO——查询路径应该总是走这一条。
//!
//! ## 模块导出
//!
//! 按 `uc-application/AGENTS.md` §11.4,只暴露 Facade + Query/Result/Error +
//! 订阅事件类型。`PresenceEvent` 从 `uc-core` 透出以便上层 crate 订阅
//! `subscribe_presence_events()` 时不用直接依赖 `uc-core::ports`。

mod commands;
mod errors;
mod facade;

pub use commands::{
    ContentTypesPatch, ContentTypesView, MemberSummary, MemberSyncPreferencesPatch,
    MemberSyncPreferencesView, PeerSnapshotView, RosterEntry,
};
pub use errors::RosterError;
pub use facade::{MemberRosterDeps, MemberRosterFacade};
pub use uc_core::ports::{ConnectionChannel, PresenceEvent};

/// Phase 96 INDIC-01:`ConnectionChannel` 4 态映射到稳定 wire 字符串。
///
/// 单点产出 `"direct" | "relay" | "offline" | "unknown"`,daemon 层
/// (`presence_monitor.rs` / `server.rs::peer_snapshots`)直接复用,
/// 避免每个边界各写一遍 match 翻车(Pitfall 1 反向命名同源风险)。
pub fn connection_channel_to_wire(channel: ConnectionChannel) -> &'static str {
    match channel {
        ConnectionChannel::Direct => "direct",
        ConnectionChannel::Relay => "relay",
        ConnectionChannel::Offline => "offline",
        ConnectionChannel::Unknown => "unknown",
    }
}

#[cfg(test)]
mod wire_tests {
    use super::{connection_channel_to_wire, ConnectionChannel};

    #[test]
    fn wire_strings_are_locked() {
        // Phase 96 INDIC-01:wire 字符串是 daemon ↔ frontend 协议契约,
        // 任何重命名都会让前端徽章渲染失效。本 truth-table 测试故意
        // 硬编码字面值,迫使协议变更必须同步改前端 i18n key + 渲染分支。
        assert_eq!(
            connection_channel_to_wire(ConnectionChannel::Direct),
            "direct"
        );
        assert_eq!(
            connection_channel_to_wire(ConnectionChannel::Relay),
            "relay"
        );
        assert_eq!(
            connection_channel_to_wire(ConnectionChannel::Offline),
            "offline"
        );
        assert_eq!(
            connection_channel_to_wire(ConnectionChannel::Unknown),
            "unknown"
        );
    }
}
