//! GUI 进程对 daemon runtime 的所有权跟踪（GUI-framework agnostic）。
//!
//! ADR-008 P3-3 起 GUI 永远是外部 `uniclipd` 的纯客户端——daemon 从来不在 GUI
//! 进程内启动，所以历史上的 `Owned(DaemonHandle)`（GUI 进程内 daemon、退出时
//! 调 `DaemonHandle::shutdown`）已成死代码，本类型在 ADR-008 P4-3 (D3) 收敛为
//! 一个轻量信息标记：
//!
//! - **None**：还没探测 / attach 任何 daemon。
//! - **External**：已连接到外部 daemon 进程（GUI 只是 client）。
//!
//! 注意：**"彻底退出时是否停 daemon" 不读这个标记**。那个决策（修订版 D3）是
//! "明确点退出就停连接的 daemon，不论谁拉起"，仅保留 identity 校验 + 拒杀
//! legacy in-process 两个安全闸（见 `uc-desktop` 的
//! `stop_local_daemon_on_full_quit`）。想留 daemon 的用户用关窗 / 轻量模式。
//!
//! ADR-008 后续 cleanup：本类型唯一消费者是 GUI（`uc-desktop` 的
//! `daemon_probe` 与 `uc-tauri` 的 run loop），daemon runtime 自身从不引用它。
//! 故它落在 `uc-desktop`，而**不**经由 `uc-daemon` re-export——避免仅为这个
//! ~40 行类型把整棵 daemon runtime 依赖树链进 GUI 构建。

use std::sync::{Arc, Mutex};

/// daemon runtime 的 GUI 端所有权状态。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum OwnershipState {
    /// 尚未开始 probe / attach。
    #[default]
    None,
    /// 已连接到外部 daemon（独立进程）——GUI 只是 client。
    External,
}

#[derive(Default)]
struct DaemonOwnershipInner {
    state: Mutex<OwnershipState>,
}

/// daemon runtime 所有权的 GUI 端跟踪句柄。
///
/// `Clone` + 内部 `Arc<Mutex<...>>`——shell 可以把它放进 Tauri `manage` 状态，
/// 也可以多份 clone 给 setup / RunEvent 闭包。
#[derive(Clone, Default)]
pub struct DaemonOwnership(Arc<DaemonOwnershipInner>);

impl DaemonOwnership {
    /// 记录"已连接到外部 daemon"——GUI 只是 client。
    pub fn set_external(&self) {
        let mut guard = self
            .0
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = OwnershipState::External;
    }

    /// 重置为 `None`（probe 失败 / 主动断开）。
    pub fn clear(&self) {
        let mut guard = self
            .0
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = OwnershipState::None;
    }

    /// 当前是否已 attach 到外部 daemon。
    pub fn is_external(&self) -> bool {
        let guard = self
            .0
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        matches!(*guard, OwnershipState::External)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_none() {
        assert!(!DaemonOwnership::default().is_external());
    }

    #[test]
    fn set_external_then_clear() {
        let ownership = DaemonOwnership::default();
        ownership.set_external();
        assert!(ownership.is_external());
        ownership.clear();
        assert!(!ownership.is_external());
    }

    #[test]
    fn clone_shares_underlying_state() {
        // Tauri stores DaemonOwnership in `manage(...)` and clones it into
        // every closure — clones must point at the same Arc<Mutex<...>>.
        let a = DaemonOwnership::default();
        let b = a.clone();
        a.set_external();
        assert!(
            b.is_external(),
            "clone must observe set_external via shared Arc"
        );
    }
}
