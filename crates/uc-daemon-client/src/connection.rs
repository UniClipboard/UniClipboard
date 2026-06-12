//! Connection state for daemon clients.

use std::sync::{Arc, RwLock};
use uc_daemon_contract::api::auth::DaemonConnectionInfo;

#[derive(Clone, Default)]
pub struct DaemonConnectionState(Arc<RwLock<Option<DaemonConnectionInfo>>>);

impl DaemonConnectionState {
    pub fn set(&self, connection_info: DaemonConnectionInfo) {
        match self.0.write() {
            Ok(mut guard) => {
                *guard = Some(connection_info);
            }
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in DaemonConnectionState::set, recovering from poisoned state"
                );
                let mut guard = poisoned.into_inner();
                *guard = Some(connection_info);
            }
        }
    }

    pub fn get(&self) -> Option<DaemonConnectionInfo> {
        match self.0.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in DaemonConnectionState::get, recovering from poisoned state"
                );
                poisoned.into_inner().clone()
            }
        }
    }
}
