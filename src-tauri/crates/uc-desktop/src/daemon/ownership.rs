//! GUI 进程对 daemon runtime 的所有权跟踪（GUI-framework agnostic）。
//!
//! 双模 daemon 生命周期协调：GUI 启动时探测本机 daemon HTTP 端点。
//!
//! - **External**：已有 daemon 在跑（`cli start` 拉起的独立 daemon binary）；
//!   GUI 只是 client，关闭时不动它。
//! - **Owned**：daemon 是 GUI 自己 in-process 启动的（持有
//!   [`DaemonHandle`]）；GUI 退出时调 [`DaemonHandle::shutdown`] 触发优雅
//!   关闭。
//!
//! shell 层（Tauri / 未来 native）拿这个状态做关闭时分支：
//!
//! ```ignore
//! match ownership.take_owned() {
//!     Some(handle) => { handle.shutdown(timeout).await?; }
//!     None         => { /* External 或 None：什么都不做 */ }
//! }
//! ```

use std::sync::{Arc, Mutex};

use crate::daemon::DaemonHandle;

/// daemon runtime 的所有权状态。
enum OwnershipState {
    /// 尚未开始 bootstrap，或已 take 走了 handle。
    None,
    /// daemon 是 GUI 进程自己 in-process 启动的——持有 handle，关闭时调 shutdown。
    Owned(DaemonHandle),
    /// 已有外部 daemon（独立进程）在跑——GUI 只是 client，关闭时不动它。
    External,
}

impl Default for OwnershipState {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Default)]
struct DaemonOwnershipInner {
    state: Mutex<OwnershipState>,
}

/// daemon runtime 所有权的 GUI 端跟踪句柄。
///
/// `Clone` + 内部 `Arc<Mutex<...>>`——shell 可以把它放进 Tauri `manage` 状态、
/// 也可以多份 clone 给 setup / RunEvent 闭包。
#[derive(Clone, Default)]
pub struct DaemonOwnership(Arc<DaemonOwnershipInner>);

impl DaemonOwnership {
    /// 记录"daemon 由本进程 in-process 启动"——consumes [`DaemonHandle`]。
    pub fn set_owned(&self, handle: DaemonHandle) {
        let mut guard = self
            .0
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = OwnershipState::Owned(handle);
    }

    /// 记录"已连接到外部 daemon"——GUI 只是 client，关闭时不影响 daemon。
    pub fn set_external(&self) {
        let mut guard = self
            .0
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = OwnershipState::External;
    }

    /// 重置为 `None`。
    pub fn clear(&self) {
        let mut guard = self
            .0
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = OwnershipState::None;
    }

    /// 当前是否拥有 in-process daemon。
    pub fn is_owned(&self) -> bool {
        let guard = self
            .0
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        matches!(*guard, OwnershipState::Owned(_))
    }

    /// 取走 owned daemon handle 并把状态重置为 `None`；只有 `Owned`
    /// 状态会返回 `Some`。shell 在 GUI 退出 hook 里调用：拿到 handle 后
    /// `await handle.shutdown(timeout)` 触发优雅关闭。
    pub fn take_owned(&self) -> Option<DaemonHandle> {
        let mut guard = self
            .0
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !matches!(*guard, OwnershipState::Owned(_)) {
            return None;
        }
        match std::mem::replace(&mut *guard, OwnershipState::None) {
            OwnershipState::Owned(handle) => Some(handle),
            _ => None,
        }
    }
}
