//! CLI development runtime backed by the public engine entrypoint.

use std::sync::Arc;
use std::time::Duration;

use uc_engine::{Engine, EventStream};

use crate::{prepare_desktop_engine_host, DesktopHostFileHandles};

pub struct CliEngineRuntime {
    engine: Arc<Engine>,
    _events: EventStream,
    file_handles: DesktopHostFileHandles,
}

impl CliEngineRuntime {
    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    pub fn file_handles(&self) -> &DesktopHostFileHandles {
        &self.file_handles
    }

    pub async fn shutdown(self) {
        if let Err(error) = self.engine.shutdown(Duration::from_secs(5)).await {
            tracing::warn!(error = %error, "CLI engine shutdown failed");
        }
    }
}

pub async fn build_cli_engine_runtime(
    log_profile: Option<uc_observability::LogProfile>,
) -> anyhow::Result<CliEngineRuntime> {
    if let Some(profile) = log_profile {
        std::env::set_var("UC_LOG_PROFILE", profile.to_string());
    }
    crate::observability::tracing::init_tracing_subscriber()?;
    crate::observability::tracing::install_panic_logging_hook();

    let prepared = prepare_desktop_engine_host()?;
    let file_handles = prepared.file_handles();
    let (config, host) = prepared.into_engine_start();
    let (engine, events) = Engine::start(config, host).await?;
    Ok(CliEngineRuntime {
        engine: Arc::new(engine),
        _events: events,
        file_handles,
    })
}
