use std::sync::Arc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uc_engine::{ConnectivityOpportunity, Engine, Operation};
use uc_platform::system_wake::SystemWakeMonitor;

pub(super) async fn start(
    engine: Arc<Engine>,
) -> anyhow::Result<(CancellationToken, JoinHandle<()>)> {
    let mut monitor = tokio::task::spawn_blocking(SystemWakeMonitor::start).await??;
    let cancel = CancellationToken::new();
    let stopping = cancel.clone();
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = stopping.cancelled() => break,
                event = monitor.events.recv() => {
                    if event.is_none() { break; }
                    if engine.execute(Operation::NotifyConnectivityOpportunity { reason: ConnectivityOpportunity::SystemWake }).await.is_err() {
                        tracing::warn!(error_kind = "connectivity_opportunity", "system wake could not be reported to Engine");
                    }
                }
            }
        }
        if monitor.shutdown().await.is_err() {
            tracing::warn!(
                error_kind = "system_wake_monitor",
                "system wake monitor stopped with an error"
            );
        }
    });
    Ok((cancel, task))
}
