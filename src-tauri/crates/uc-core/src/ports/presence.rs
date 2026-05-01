//! Presence port (Slice 2 Phase 1).
//!
//! Tracks whether each `SpaceMember` is currently reachable on the iroh
//! endpoint. Consumed by `MemberRosterFacade` for the roster view and by
//! `EnsureReachableAllUseCase` which fires after F1 `start_network` to
//! pre-connect every member.
//!
//! `ensure_reachable` is a single-target primitive; batching ("pre-connect
//! the whole roster") lives in the application layer so this port stays
//! minimal.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::sync::broadcast;

use crate::ids::DeviceId;

/// Reachability snapshot for one member.
///
/// Intentionally three-valued: `Unknown` distinguishes "never probed" from
/// "probed and confirmed offline". No `Connecting` / `Degraded` — Slice 2
/// has no consumer that could act on those.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReachabilityState {
    Online,
    Offline,
    Unknown,
}

/// Notification delivered on state change.
#[derive(Debug, Clone)]
pub struct PresenceEvent {
    pub device_id: DeviceId,
    pub state: ReachabilityState,
    pub at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum PresenceError {
    /// No stored [`PeerAddressRecord`](crate::ports::peer_address::PeerAddressRecord)
    /// for this device — cannot dial. Application layer treats this as
    /// "member is offline" rather than a fatal error.
    #[error("no known address for device {0:?}")]
    NoAddress(DeviceId),
    #[error("internal: {0}")]
    Internal(String),
}

#[async_trait]
pub trait PresencePort: Send + Sync {
    /// Actively probe / dial the target device.
    ///
    /// Returns the resulting state — typically `Online` on success, `Offline`
    /// on dial failure. A `NoAddress` error surfaces when the peer address
    /// repository has no record for this device.
    async fn ensure_reachable(&self, device: &DeviceId)
        -> Result<ReachabilityState, PresenceError>;

    /// Force-revalidate reachability, bypassing any cached "alive connection"
    /// fast-path inside the implementation.
    ///
    /// 用途：定期 probe 场景。`ensure_reachable` 在已有 alive 连接时即时返回
    /// `Online`，这是业务路径（剪贴板传输前先确保连接可用）的合理优化；
    /// 但当对端真断网时，QUIC `Connection::closed` watchdog 要等
    /// `max_idle_timeout`（典型 60s）才能触发 Offline，期间
    /// `ensure_reachable` 会持续撒谎说 Online。`verify_reachable` 强制重新
    /// 拨号验证：拨号成功记 Online，拨号失败立即记 Offline 并清理 stale
    /// 连接，把离线检测时延压到拨号失败时间（typically 5–15s）。
    ///
    /// Default impl 委托给 `ensure_reachable`，便于 mock / fake 复用而不必
    /// 实现两份语义；带 fast-path 缓存的真实 adapter 应当 override。
    async fn verify_reachable(
        &self,
        device: &DeviceId,
    ) -> Result<ReachabilityState, PresenceError> {
        self.ensure_reachable(device).await
    }

    /// Read the current cached state without dialing.
    ///
    /// Returns `Unknown` if the device has never been probed in the current
    /// process lifetime.
    async fn current_state(&self, device: &DeviceId) -> ReachabilityState;

    /// Multi-consumer subscription for state-change events.
    ///
    /// Each call returns a fresh receiver. Lagging receivers drop messages
    /// per `broadcast` contract — acceptable because the latest state can
    /// always be recovered via [`current_state`].
    fn subscribe(&self) -> broadcast::Receiver<PresenceEvent>;
}
