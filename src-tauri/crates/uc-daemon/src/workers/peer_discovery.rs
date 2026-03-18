//! Placeholder peer discovery worker.

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::worker::{DaemonWorker, WorkerHealth};

/// Placeholder peer discovery worker.
///
/// In the skeleton phase this worker does nothing except wait for cancellation.
/// A future plan will integrate with the libp2p mDNS discovery subsystem.
pub struct PeerDiscoveryWorker;

#[async_trait]
impl DaemonWorker for PeerDiscoveryWorker {
    fn name(&self) -> &str {
        "peer-discovery"
    }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        tracing::info!("peer discovery started (placeholder)");
        cancel.cancelled().await;
        tracing::info!("peer discovery cancelled");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        tracing::info!("peer discovery stopped");
        Ok(())
    }

    fn health_check(&self) -> WorkerHealth {
        WorkerHealth::Healthy
    }
}
