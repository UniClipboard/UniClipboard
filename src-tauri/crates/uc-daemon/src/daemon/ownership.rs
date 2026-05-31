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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    /// Build a `DaemonHandle` whose backing task immediately returns Ok —
    /// good enough for state-machine tests that don't actually shut anything
    /// down. Production `DaemonHandle::new` is `pub(crate)`, which is exactly
    /// the visibility we need from this same crate's test module.
    fn dummy_handle() -> DaemonHandle {
        let cancel = CancellationToken::new();
        let cancel_for_task = cancel.clone();
        let join = tokio::spawn(async move {
            // Wait to be cancelled so `shutdown(_)` actually exercises the
            // cancel→join path. If the test never calls shutdown, the task
            // is dropped at test end which is fine.
            cancel_for_task.cancelled().await;
            Ok(())
        });
        DaemonHandle::new(cancel, join)
    }

    #[tokio::test]
    async fn default_state_is_none_and_not_owned() {
        let ownership = DaemonOwnership::default();
        assert!(!ownership.is_owned());
        assert!(
            ownership.take_owned().is_none(),
            "take_owned on a fresh ownership must return None"
        );
    }

    #[tokio::test]
    async fn set_external_does_not_count_as_owned() {
        let ownership = DaemonOwnership::default();
        ownership.set_external();
        assert!(
            !ownership.is_owned(),
            "External daemon is not ours — must not be reported as Owned"
        );
        assert!(
            ownership.take_owned().is_none(),
            "External daemon must not yield a shutdown handle to GUI exit hook"
        );
    }

    #[tokio::test]
    async fn set_owned_then_take_returns_handle_once() {
        let ownership = DaemonOwnership::default();
        ownership.set_owned(dummy_handle());
        assert!(ownership.is_owned());

        let handle = ownership
            .take_owned()
            .expect("first take_owned must yield the handle");
        // Drive the handle to completion via shutdown to prove it's a real,
        // controllable handle and to clean up the spawned task deterministically.
        handle
            .shutdown(Duration::from_secs(1))
            .await
            .expect("dummy task exits Ok on cancel");

        assert!(
            !ownership.is_owned(),
            "after take_owned, state resets to None"
        );
        assert!(
            ownership.take_owned().is_none(),
            "take_owned must be one-shot — second call yields None"
        );
    }

    #[tokio::test]
    async fn clear_drops_owned_handle_and_resets_state() {
        let ownership = DaemonOwnership::default();
        ownership.set_owned(dummy_handle());

        ownership.clear();

        assert!(!ownership.is_owned());
        assert!(
            ownership.take_owned().is_none(),
            "clear() must drop the owned handle entirely"
        );
    }

    #[tokio::test]
    async fn clone_shares_underlying_state() {
        // Tauri stores DaemonOwnership in `manage(...)` and clones it into
        // every closure — clones must point at the same Arc<Mutex<...>>.
        let a = DaemonOwnership::default();
        let b = a.clone();

        a.set_owned(dummy_handle());
        assert!(b.is_owned(), "clone must observe set_owned via shared Arc");

        let _ = b
            .take_owned()
            .expect("clone can take handle set on original");
        assert!(!a.is_owned(), "take through clone must reset original too");
    }

    #[tokio::test]
    async fn state_transitions_owned_to_external_drops_handle() {
        // GUI shell hypothetical: misconfigured re-bootstrap. Setting
        // External over Owned must not silently leak the in-process daemon
        // handle, because the GUI exit hook will then think there's nothing
        // to shut down. Today's contract is: Owned → External replaces the
        // state, the previous handle is dropped. We assert that observable
        // contract here so a future refactor can't quietly change it.
        let ownership = DaemonOwnership::default();
        ownership.set_owned(dummy_handle());
        ownership.set_external();
        assert!(!ownership.is_owned());
        assert!(ownership.take_owned().is_none());
    }
}
