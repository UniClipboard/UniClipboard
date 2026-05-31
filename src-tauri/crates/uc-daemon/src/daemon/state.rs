//! # RuntimeState
//!
//! Snapshot-only state for the daemon runtime. Tracks uptime and cached
//! service health statuses. Does NOT own services — `DaemonApp` owns services
//! and periodically updates this snapshot.

use std::time::Instant;

use crate::daemon::service::ServiceHealth;

#[derive(Debug, Clone, PartialEq)]
pub struct DaemonServiceSnapshot {
    pub name: String,
    pub health: ServiceHealth,
}

/// Runtime state snapshot for the daemon.
///
/// This struct holds only pure data (start time + cached service statuses).
/// It is fully `Send + Sync` without trait object concerns. RPC reads never
/// contend with service lifecycle because this is a snapshot, not a live view.
#[allow(dead_code)]
pub struct RuntimeState {
    start_time: Instant,
    worker_statuses: Vec<DaemonServiceSnapshot>,
    connected_peer_count: u32,
}

#[allow(dead_code)]
impl RuntimeState {
    /// Create a new RuntimeState with the given initial service statuses.
    pub fn new(initial_statuses: Vec<DaemonServiceSnapshot>) -> Self {
        Self {
            start_time: Instant::now(),
            worker_statuses: initial_statuses,
            connected_peer_count: 0,
        }
    }

    /// Elapsed time since the daemon started, in seconds.
    pub fn uptime_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Current cached service statuses.
    pub fn worker_statuses(&self) -> &[DaemonServiceSnapshot] {
        &self.worker_statuses
    }

    /// Replace the cached service statuses with a fresh snapshot.
    pub fn update_worker_statuses(&mut self, statuses: Vec<DaemonServiceSnapshot>) {
        self.worker_statuses = statuses;
    }

    /// Update the health of a single named service in the cached snapshot (Phase 67).
    ///
    /// Used to transition peer-discovery from `Stopped` → `Healthy` when the deferred
    /// `PeerDiscoveryWorker` starts after setup completes on an uninitialized device.
    /// No-op if the named service is not found.
    pub fn update_service_health(&mut self, name: &str, health: ServiceHealth) {
        if let Some(snapshot) = self.worker_statuses.iter_mut().find(|s| s.name == name) {
            snapshot.health = health;
        }
    }

    /// Current connected peer count tracked by the daemon runtime.
    pub fn connected_peer_count(&self) -> u32 {
        self.connected_peer_count
    }

    /// Replace the cached connected peer count with a fresh summary.
    pub fn update_connected_peer_count(&mut self, count: u32) {
        self.connected_peer_count = count;
    }
}
