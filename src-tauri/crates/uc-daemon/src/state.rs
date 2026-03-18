//! # RuntimeState
//!
//! Snapshot-only state for the daemon runtime. Tracks uptime and cached
//! worker health statuses. Does NOT own workers — `DaemonApp` owns workers
//! and periodically updates this snapshot.

use std::time::Instant;

use crate::rpc::types::WorkerStatus;

/// Runtime state snapshot for the daemon.
///
/// This struct holds only pure data (start time + cached worker statuses).
/// It is fully `Send + Sync` without trait object concerns. RPC reads never
/// contend with worker lifecycle because this is a snapshot, not a live view.
pub struct RuntimeState {
    start_time: Instant,
    worker_statuses: Vec<WorkerStatus>,
}

impl RuntimeState {
    /// Create a new RuntimeState with the given initial worker statuses.
    pub fn new(initial_statuses: Vec<WorkerStatus>) -> Self {
        Self {
            start_time: Instant::now(),
            worker_statuses: initial_statuses,
        }
    }

    /// Elapsed time since the daemon started, in seconds.
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Current cached worker statuses.
    pub fn worker_statuses(&self) -> &[WorkerStatus] {
        &self.worker_statuses
    }

    /// Replace the cached worker statuses with a fresh snapshot.
    pub fn update_worker_statuses(&mut self, statuses: Vec<WorkerStatus>) {
        self.worker_statuses = statuses;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worker::WorkerHealth;

    #[test]
    fn test_uptime_is_non_zero() {
        let state = RuntimeState::new(vec![]);
        // Sleep briefly to ensure elapsed time > 0
        std::thread::sleep(std::time::Duration::from_millis(10));
        assert!(
            state.start_time.elapsed().as_millis() > 0,
            "elapsed time should be > 0 after sleep"
        );
    }

    #[test]
    fn test_update_worker_statuses() {
        let mut state = RuntimeState::new(vec![]);
        assert_eq!(state.worker_statuses().len(), 0);

        state.update_worker_statuses(vec![
            WorkerStatus {
                name: "clipboard-watcher".to_string(),
                health: WorkerHealth::Healthy,
            },
            WorkerStatus {
                name: "peer-discovery".to_string(),
                health: WorkerHealth::Stopped,
            },
        ]);
        assert_eq!(state.worker_statuses().len(), 2);
        assert_eq!(state.worker_statuses()[0].name, "clipboard-watcher");
        assert_eq!(state.worker_statuses()[1].health, WorkerHealth::Stopped);
    }
}
