//! GUI 进程对 daemon 子进程拉起过程的协调状态（GUI-framework agnostic）。
//!
//! `DaemonBootstrapOwnership` 跟踪：
//!
//! - 已 spawn 的 daemon child PID
//! - 因版本不兼容触发的替换次数
//! - 上一次替换的原因
//!
//! 这是"桌面侧拉起 daemon"流程的协调状态，与具体 GUI 框架无关。子进程
//! 本体（`CommandChild` 或 `std::process::Child`）由各 shell 自己管理
//! （Tauri shell 用 `uc_daemon_local::daemon_lifecycle::GuiOwnedDaemonState`
//! 持有 `tauri-plugin-shell::CommandChild`）。

use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DaemonBootstrapOwnershipSnapshot {
    pub replacement_attempt: u8,
    pub spawned_child_pid: Option<u32>,
    pub last_incompatible_reason: Option<String>,
}

#[derive(Clone, Default)]
pub struct DaemonBootstrapOwnershipState(Arc<RwLock<DaemonBootstrapOwnershipSnapshot>>);

impl DaemonBootstrapOwnershipState {
    pub fn snapshot(&self) -> DaemonBootstrapOwnershipSnapshot {
        match self.0.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in DaemonBootstrapOwnershipState::snapshot, recovering from poisoned state"
                );
                poisoned.into_inner().clone()
            }
        }
    }

    pub fn record_spawned_child(&self, pid: Option<u32>) {
        match self.0.write() {
            Ok(mut guard) => {
                guard.spawned_child_pid = pid;
            }
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in DaemonBootstrapOwnershipState::record_spawned_child, recovering from poisoned state"
                );
                let mut guard = poisoned.into_inner();
                guard.spawned_child_pid = pid;
            }
        }
    }

    pub fn clear_spawned_child(&self) {
        self.record_spawned_child(None);
    }

    pub fn record_replacement_attempt(&self, reason: String) {
        match self.0.write() {
            Ok(mut guard) => {
                guard.replacement_attempt = guard.replacement_attempt.saturating_add(1);
                guard.last_incompatible_reason = Some(reason);
            }
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in DaemonBootstrapOwnershipState::record_replacement_attempt, recovering from poisoned state"
                );
                let mut guard = poisoned.into_inner();
                guard.replacement_attempt = guard.replacement_attempt.saturating_add(1);
                guard.last_incompatible_reason = Some(reason);
            }
        }
    }
}
