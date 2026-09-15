//! Connection state for daemon clients.

use std::sync::{Arc, RwLock};
use uc_daemon_contract::api::auth::DaemonConnectionInfo;

#[derive(Default)]
struct DaemonConnectionStateInner {
    connection_info: Option<DaemonConnectionInfo>,
    revision: u64,
    session_token_cache: Option<(String, u64)>,
}

#[derive(Clone, Default)]
pub struct DaemonConnectionState(Arc<RwLock<DaemonConnectionStateInner>>);

impl DaemonConnectionState {
    pub fn set(&self, connection_info: DaemonConnectionInfo) {
        match self.0.write() {
            Ok(mut guard) => {
                guard.revision = guard.revision.wrapping_add(1);
                guard.connection_info = Some(connection_info);
                guard.session_token_cache = None;
            }
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in DaemonConnectionState::set, recovering from poisoned state"
                );
                let mut guard = poisoned.into_inner();
                guard.revision = guard.revision.wrapping_add(1);
                guard.connection_info = Some(connection_info);
                guard.session_token_cache = None;
            }
        }
    }

    pub fn get(&self) -> Option<DaemonConnectionInfo> {
        self.snapshot().map(|(connection, _)| connection)
    }

    pub(crate) fn snapshot(&self) -> Option<(DaemonConnectionInfo, u64)> {
        match self.0.read() {
            Ok(guard) => guard
                .connection_info
                .clone()
                .map(|connection| (connection, guard.revision)),
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in DaemonConnectionState::snapshot, recovering from poisoned state"
                );
                let guard = poisoned.into_inner();
                guard
                    .connection_info
                    .clone()
                    .map(|connection| (connection, guard.revision))
            }
        }
    }

    pub(crate) fn cached_session_token(&self, revision: u64, now: u64) -> Option<String> {
        match self.0.read() {
            Ok(guard) if guard.revision == revision => guard
                .session_token_cache
                .as_ref()
                .filter(|(_, expires_at)| *expires_at > now + 30)
                .map(|(token, _)| token.clone()),
            Ok(_) => None,
            Err(poisoned) => {
                let guard = poisoned.into_inner();
                if guard.revision != revision {
                    return None;
                }
                guard
                    .session_token_cache
                    .as_ref()
                    .filter(|(_, expires_at)| *expires_at > now + 30)
                    .map(|(token, _)| token.clone())
            }
        }
    }

    pub(crate) fn cache_session_token_if_current(
        &self,
        revision: u64,
        token: String,
        expires_at: u64,
    ) -> bool {
        match self.0.write() {
            Ok(mut guard) if guard.revision == revision => {
                guard.session_token_cache = Some((token, expires_at));
                true
            }
            Ok(_) => false,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                if guard.revision != revision {
                    return false;
                }
                guard.session_token_cache = Some((token, expires_at));
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(name: &str) -> DaemonConnectionInfo {
        DaemonConnectionInfo {
            base_url: format!("http://{name}"),
            ws_url: format!("ws://{name}"),
            token: format!("{name}-token"),
            pid: 42,
        }
    }

    #[test]
    fn stale_exchange_cannot_fill_cache_after_connection_replacement() {
        let state = DaemonConnectionState::default();
        state.set(connection("first"));
        let (_, first_revision) = state.snapshot().expect("first connection");

        state.set(connection("second"));

        assert!(!state.cache_session_token_if_current(
            first_revision,
            "first-session".to_string(),
            600,
        ));
        let (_, second_revision) = state.snapshot().expect("second connection");
        assert_eq!(state.cached_session_token(second_revision, 0), None);
    }
}
