use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;
use tracing::warn;
use uc_app::realtime::{
    run_pairing_realtime_consumer, run_pairing_realtime_consumer_with_rx,
    run_peers_realtime_consumer, run_peers_realtime_consumer_with_rx, run_setup_realtime_consumer,
    run_setup_realtime_consumer_with_rx, run_setup_state_realtime_consumer,
    run_setup_state_realtime_consumer_with_rx, SetupPairingEventHub,
};
use uc_app::task_registry::TaskRegistry;
use uc_core::ports::{HostEventEmitterPort, RealtimeTopic};

use crate::http::DaemonPairingClient;
use crate::ws_bridge::DaemonWsBridgeConfig;
use crate::{DaemonConnectionState, DaemonWsBridge};

const GUI_PAIRING_LEASE_TTL_MS: u64 = 300_000;
const GUI_PAIRING_LEASE_RENEW_INTERVAL: Duration = Duration::from_secs(60);
const GUI_PAIRING_LEASE_WAIT_INTERVAL: Duration = Duration::from_millis(250);
const SETUP_PAIRING_HUB_BUFFER: usize = 64;

#[async_trait]
trait PairingLeasePort: Send + Sync {
    async fn set_enabled(&self, enabled: bool) -> Result<()>;
}

struct DaemonPairingLeasePort {
    connection_state: DaemonConnectionState,
}

impl DaemonPairingLeasePort {
    fn new(connection_state: DaemonConnectionState) -> Self {
        Self { connection_state }
    }
}

#[async_trait]
impl PairingLeasePort for DaemonPairingLeasePort {
    async fn set_enabled(&self, enabled: bool) -> Result<()> {
        DaemonPairingClient::new(self.connection_state.clone())
            .register_gui_participant(enabled, Some(GUI_PAIRING_LEASE_TTL_MS))
            .await?;
        Ok(())
    }
}

async fn maintain_gui_pairing_leases(
    connection_state: DaemonConnectionState,
    lease_port: Arc<dyn PairingLeasePort>,
    token: CancellationToken,
) {
    loop {
        if token.is_cancelled() {
            break;
        }

        if connection_state.get().is_none() {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = sleep(GUI_PAIRING_LEASE_WAIT_INTERVAL) => {}
            }
            continue;
        }

        if let Err(err) = lease_port.set_enabled(true).await {
            warn!(error = %err, ttl_ms = GUI_PAIRING_LEASE_TTL_MS, "failed to renew gui pairing lease");
        }

        tokio::select! {
            _ = token.cancelled() => break,
            _ = sleep(GUI_PAIRING_LEASE_RENEW_INTERVAL) => {}
        }
    }

    if connection_state.get().is_some() {
        if let Err(err) = lease_port.set_enabled(false).await {
            warn!(error = %err, "failed to revoke gui pairing lease");
        }
    }
}

/// Installs the daemon-backed setup/pairing facade into SetupAssemblyPorts.
///
/// This function is moved from uc-tauri bootstrap. The `SetupAssemblyPorts` type
/// and `build_setup_pairing_facade` function are deferred to Phase 55 extraction.
pub fn install_daemon_setup_pairing_facade(
    _setup_ports: &mut uc_bootstrap::assembly::SetupAssemblyPorts,
    connection_state: DaemonConnectionState,
) -> Arc<SetupPairingEventHub> {
    let setup_hub = Arc::new(SetupPairingEventHub::new(SETUP_PAIRING_HUB_BUFFER));
    // Phase 55: wire up setup_ports.setup_pairing_facade = build_setup_pairing_facade(connection_state, setup_hub.clone())
    let _ = (connection_state, setup_hub.clone());
    setup_hub
}

pub async fn start_realtime_runtime(
    connection_state: DaemonConnectionState,
    event_emitter: Arc<dyn HostEventEmitterPort>,
    setup_hub: Arc<SetupPairingEventHub>,
    task_registry: &Arc<TaskRegistry>,
) {
    let bridge = Arc::new(DaemonWsBridge::new(
        connection_state.clone(),
        DaemonWsBridgeConfig::default(),
    ));

    let pairing_rx = match bridge
        .subscribe("pairing_realtime_consumer", &[RealtimeTopic::Pairing])
        .await
    {
        Ok(rx) => Some(rx),
        Err(err) => {
            warn!(error = %err, "failed to eagerly subscribe pairing realtime consumer");
            None
        }
    };

    let peers_rx = match bridge
        .subscribe("peers_realtime_consumer", &[RealtimeTopic::Peers])
        .await
    {
        Ok(rx) => Some(rx),
        Err(err) => {
            warn!(error = %err, "failed to eagerly subscribe peers realtime consumer");
            None
        }
    };

    let setup_rx = match bridge
        .subscribe("setup_realtime_consumer", &[RealtimeTopic::Pairing])
        .await
    {
        Ok(rx) => Some(rx),
        Err(err) => {
            warn!(error = %err, "failed to eagerly subscribe setup realtime consumer");
            None
        }
    };

    let setup_state_rx = match bridge
        .subscribe("setup_state_realtime_consumer", &[RealtimeTopic::Setup])
        .await
    {
        Ok(rx) => Some(rx),
        Err(err) => {
            warn!(error = %err, "failed to eagerly subscribe setup state realtime consumer");
            None
        }
    };

    let pairing_bridge = bridge.clone();
    let pairing_emitter = event_emitter.clone();
    task_registry
        .spawn("realtime_pairing_consumer", |_token| async move {
            let result = match pairing_rx {
                Some(mut rx) => {
                    run_pairing_realtime_consumer_with_rx(&mut rx, pairing_emitter).await
                }
                None => run_pairing_realtime_consumer(pairing_bridge, pairing_emitter).await,
            };
            if let Err(err) = result {
                warn!(error = %err, "pairing realtime consumer stopped");
            }
        })
        .await;

    let peers_bridge = bridge.clone();
    let peers_emitter = event_emitter.clone();
    task_registry
        .spawn("realtime_peers_consumer", |_token| async move {
            let result = match peers_rx {
                Some(mut rx) => run_peers_realtime_consumer_with_rx(&mut rx, peers_emitter).await,
                None => run_peers_realtime_consumer(peers_bridge, peers_emitter).await,
            };
            if let Err(err) = result {
                warn!(error = %err, "peers realtime consumer stopped");
            }
        })
        .await;

    let setup_bridge = bridge.clone();
    let setup_hub_clone = setup_hub.clone();
    task_registry
        .spawn("realtime_setup_consumer", |_token| async move {
            let result = match setup_rx {
                Some(mut rx) => run_setup_realtime_consumer_with_rx(&mut rx, setup_hub_clone).await,
                None => run_setup_realtime_consumer(setup_bridge, setup_hub_clone).await,
            };
            if let Err(err) = result {
                warn!(error = %err, "setup realtime consumer stopped");
            }
        })
        .await;

    let setup_state_bridge = bridge.clone();
    let setup_state_emitter = event_emitter.clone();
    task_registry
        .spawn("realtime_setup_state_consumer", |_token| async move {
            let result = match setup_state_rx {
                Some(mut rx) => {
                    run_setup_state_realtime_consumer_with_rx(&mut rx, setup_state_emitter).await
                }
                None => {
                    run_setup_state_realtime_consumer(setup_state_bridge, setup_state_emitter).await
                }
            };
            if let Err(err) = result {
                warn!(error = %err, "setup state realtime consumer stopped");
            }
        })
        .await;

    task_registry
        .spawn("daemon_ws_bridge", move |token| async move {
            if let Err(err) = bridge.run(token).await {
                warn!(error = %err, "daemon websocket bridge stopped");
            }
        })
        .await;

    let lease_connection_state = connection_state.clone();
    let lease_port: Arc<dyn PairingLeasePort> =
        Arc::new(DaemonPairingLeasePort::new(connection_state.clone()));
    task_registry
        .spawn("gui_pairing_lease_keeper", move |token| async move {
            maintain_gui_pairing_leases(lease_connection_state, lease_port, token).await;
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc as StdArc;
    use uc_daemon::api::auth::DaemonConnectionInfo;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct LeaseCall {
        enabled: bool,
    }

    #[derive(Default)]
    struct RecordingLeasePort {
        calls: tokio::sync::Mutex<Vec<LeaseCall>>,
    }

    impl RecordingLeasePort {
        async fn get_calls_async(&self) -> Vec<LeaseCall> {
            self.calls.lock().await.clone()
        }
    }

    #[async_trait]
    impl PairingLeasePort for RecordingLeasePort {
        async fn set_enabled(&self, enabled: bool) -> Result<()> {
            self.calls.lock().await.push(LeaseCall { enabled });
            Ok(())
        }
    }

    #[tokio::test]
    async fn gui_pairing_lease_keeper_registers_and_revokes_gui_leases() {
        let connection_state = DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: "http://127.0.0.1:42715".to_string(),
            ws_url: "ws://127.0.0.1:42715/ws".to_string(),
            token: "test-token".to_string(),
        });
        let lease_port_concrete: StdArc<RecordingLeasePort> =
            StdArc::new(RecordingLeasePort::default());
        let lease_port: StdArc<dyn PairingLeasePort> = lease_port_concrete.clone();
        let token = CancellationToken::new();

        let task = tokio::spawn(maintain_gui_pairing_leases(
            connection_state,
            lease_port.clone(),
            token.clone(),
        ));

        tokio::time::sleep(Duration::from_millis(25)).await;
        token.cancel();
        task.await.expect("lease keeper should stop cleanly");

        let calls = lease_port_concrete.get_calls_async().await;
        assert_eq!(
            calls,
            vec![LeaseCall { enabled: true }, LeaseCall { enabled: false },]
        );
    }
}
