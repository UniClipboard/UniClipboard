//! # DaemonService Trait
//!
//! Defines the contract for long-lived daemon services managed by the daemon runtime.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// Health status of a daemon service.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServiceHealth {
    /// Service is running normally.
    Healthy,
    /// Service is running but with degraded functionality.
    Degraded(String),
    /// Service has stopped.
    Stopped,
}

/// Trait for long-lived daemon services managed by the daemon runtime.
///
/// Services are started with a [`CancellationToken`] for cooperative shutdown.
/// The `health_check()` method is synchronous to allow lock-free status polling.
#[async_trait]
pub trait DaemonService: Send + Sync {
    /// Human-readable name of this service (e.g., "clipboard-watcher").
    fn name(&self) -> &str;

    /// Start the service. The service should select on `cancel.cancelled()` to
    /// support cooperative shutdown.
    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()>;

    /// Stop the service gracefully.
    async fn stop(&self) -> anyhow::Result<()>;

    /// Return the current health status. Synchronous for lock-free polling.
    #[allow(dead_code)]
    fn health_check(&self) -> ServiceHealth;
}
