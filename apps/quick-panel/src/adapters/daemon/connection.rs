use std::sync::Mutex;

use quick_panel_core::ports::ServiceError;
use uc_daemon_client::DaemonClientContext;
use uc_daemon_contract::api::auth::DaemonConnectionInfo;

type Resolver = Box<dyn Fn() -> anyhow::Result<DaemonConnectionInfo> + Send + Sync>;

pub struct DaemonHistory {
    resolver: Resolver,
    shared: Mutex<Option<(DaemonConnectionInfo, DaemonClientContext)>>,
}

impl DaemonHistory {
    /// Finds the daemon the way the GUI and the CLI do: from the environment, else from the
    /// connection file the daemon writes.
    pub fn new() -> Self {
        Self::with_resolver(uc_daemon_client::resolve_connection_info_from_env)
    }

    pub fn with_resolver(
        resolver: impl Fn() -> anyhow::Result<DaemonConnectionInfo> + Send + Sync + 'static,
    ) -> Self {
        Self {
            resolver: Box::new(resolver),
            shared: Mutex::new(None),
        }
    }

    pub(super) fn resolve(&self) -> anyhow::Result<DaemonConnectionInfo> {
        (self.resolver)()
    }

    pub(super) fn context(&self) -> Result<DaemonClientContext, ServiceError> {
        let connection = self.resolve().map_err(|_| ServiceError::NotRunning)?;
        self.context_for(connection)
    }

    /// Returns the shared context, rebuilding it only when the resolved connection (address or
    /// bearer token) differs from the one it was built for, for example after the daemon restarts.
    /// A rebuild starts a fresh session-token cache.
    pub(super) fn context_for(
        &self,
        connection: DaemonConnectionInfo,
    ) -> Result<DaemonClientContext, ServiceError> {
        let mut shared = self
            .shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((known, context)) = shared.as_ref() {
            if *known == connection {
                return Ok(context.clone());
            }
        }
        let context = DaemonClientContext::new(connection.clone())
            .map_err(|_| ServiceError::CannotConnect)?;
        *shared = Some((connection, context.clone()));
        Ok(context)
    }

    #[cfg(test)]
    pub(super) fn cached_connection(&self) -> Option<DaemonConnectionInfo> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|(known, _)| known.clone())
    }
}

impl Default for DaemonHistory {
    fn default() -> Self {
        Self::new()
    }
}
