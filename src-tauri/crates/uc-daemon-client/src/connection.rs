//! Connection state for daemon clients.

use std::sync::{Arc, RwLock};
use uc_daemon::api::auth::DaemonConnectionInfo;

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

#[cfg(test)]
mod tests {
    use super::*;
    use uc_daemon::api::auth::DaemonConnectionInfo;

    #[test]
    fn daemon_connection_state_stores_connection_info_in_memory() {
        let state = DaemonConnectionState::default();
        assert!(state.get().is_none());

        state.set(DaemonConnectionInfo {
            base_url: "http://127.0.0.1:42715".to_string(),
            ws_url: "ws://127.0.0.1:42715/ws".to_string(),
            token: "test-token".to_string(),
        });

        let info = state.get().expect("should have connection info");
        assert_eq!(info.base_url, "http://127.0.0.1:42715");
        assert_eq!(info.token, "test-token");
    }
}
