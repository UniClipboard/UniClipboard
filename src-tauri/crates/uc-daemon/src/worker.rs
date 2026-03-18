//! # DaemonWorker Trait
//!
//! Defines the contract for long-lived background workers in the daemon.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// Health status of a daemon worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WorkerHealth {
    /// Worker is running normally.
    Healthy,
    /// Worker is running but with degraded functionality.
    Degraded(String),
    /// Worker has stopped.
    Stopped,
}

/// Trait for long-lived background workers managed by the daemon.
///
/// Workers are started with a [`CancellationToken`] for cooperative shutdown.
/// The `health_check()` method is synchronous to allow lock-free status polling.
#[async_trait]
pub trait DaemonWorker: Send + Sync {
    /// Human-readable name of this worker (e.g., "clipboard-watcher").
    fn name(&self) -> &str;

    /// Start the worker. The worker should select on `cancel.cancelled()` to
    /// support cooperative shutdown.
    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()>;

    /// Stop the worker gracefully.
    async fn stop(&self) -> anyhow::Result<()>;

    /// Return the current health status. Synchronous for lock-free polling.
    fn health_check(&self) -> WorkerHealth;
}
