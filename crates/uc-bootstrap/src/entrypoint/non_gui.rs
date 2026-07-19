//! Desktop non-GUI runtime helpers.

use std::sync::Arc;

use uc_application::facade::{
    AppFacade, ClipboardOutboundDeps, ClipboardOutboundFacade, InMemoryLifecycleStatus,
    LifecycleStatusGateway, SearchCoordinator, SearchCoordinatorDeps,
};
use uc_core::clipboard::ClipboardIntegrationMode;

use crate::layer::paths::get_storage_paths;
use crate::subsystem::sync_engine::{build_sync_engine_assembly, SyncEngineAssembly};

pub use uc_engine::internal::facade::{
    build_app_facade_from_deps, build_mobile_sync_facade, AppFacadeAssemblyOptions,
    ClipboardRestoreAssembly,
};

/// In-process application runtime used by direct CLI commands.
pub struct CliAppRuntime {
    pub app_facade: Arc<AppFacade>,
    sync_engine_assembly: Option<SyncEngineAssembly>,
}

impl CliAppRuntime {
    pub fn app_facade(&self) -> &Arc<AppFacade> {
        &self.app_facade
    }

    pub async fn shutdown(mut self) {
        if let Some(assembly) = self.sync_engine_assembly.take() {
            assembly.shutdown().await;
        }
    }
}

/// Build the complete runtime used by direct CLI network commands.
pub async fn build_cli_app_runtime(
    log_profile: Option<uc_observability::LogProfile>,
) -> anyhow::Result<CliAppRuntime> {
    let (config, wired) = crate::entrypoint::cli::build_cli_wiring_context(log_profile).await?;
    let storage_paths = get_storage_paths(&config)?;

    let settings = wired
        .deps
        .settings
        .load()
        .await
        .map_err(|err| anyhow::anyhow!("settings load failed at startup: {err}"))?;
    let allow_relay_fallback = settings.network.allow_relay_fallback;
    let allow_overlay_network_addrs = settings.network.allow_overlay_network_addrs;
    let custom_relay_urls = settings.network.custom_relay_urls.clone();
    let congestion_controller = settings.network.congestion_controller;

    let mut iroh_config = crate::wiring::network_policy::relay_policy_to_iroh_config(
        allow_relay_fallback,
        allow_overlay_network_addrs,
        custom_relay_urls,
        congestion_controller,
        None,
    );
    crate::wiring::network_policy::apply_iroh_direct_reachability_from_env(&mut iroh_config);
    crate::wiring::network_policy::apply_congestion_controller_from_env(&mut iroh_config);

    tracing::info!(
        target: "settings.network",
        allow_relay_fallback,
        disable_relays = iroh_config.disable_relays,
        allow_overlay_network_addrs = iroh_config.allow_overlay_network_addrs,
        custom_relay_count = iroh_config.custom_relay_urls.len(),
        congestion_controller = %iroh_config.congestion_controller,
        "applying network settings: allow_relay_fallback={} -> disable_relays={}, allow_overlay_network_addrs={}, custom_relay_count={}, cc={}",
        allow_relay_fallback,
        iroh_config.disable_relays,
        iroh_config.allow_overlay_network_addrs,
        iroh_config.custom_relay_urls.len(),
        iroh_config.congestion_controller,
    );

    let assembly =
        build_sync_engine_assembly(&wired.deps, &wired.sync_engine, &wired.shared, iroh_config)
            .await
            .map_err(|err| anyhow::anyhow!("failed to bind iroh endpoint: {err}"))?;
    let deps = &wired.deps;

    let lifecycle_status: Arc<dyn LifecycleStatusGateway> =
        Arc::new(InMemoryLifecycleStatus::new());
    let search_coordinator = Arc::new(SearchCoordinator::new(SearchCoordinatorDeps::new(
        deps.search.search_index.clone(),
        deps.search.search_maintenance.clone(),
        deps.search.search_key_derivation.clone(),
        deps.search.search_pipeline.clone(),
        deps.clipboard.entry_ports.list.clone(),
        deps.clipboard.entry_ports.get.clone(),
        deps.clipboard.representation_ports.list_for_event.clone(),
        deps.clipboard.selection_repo.clone(),
        deps.clipboard.clipboard_event_reader_repo.clone(),
        deps.storage.entry_file_set_repo.clone(),
        uc_infra::search::constants::CURRENT_INDEX_VERSION,
    )));

    let mobile_sync_apply_inbound = uc_engine::internal::facade::build_fallback_apply_inbound(deps);
    let clipboard_outbound = Arc::new(ClipboardOutboundFacade::new(ClipboardOutboundDeps {
        settings: deps.settings.clone(),
        clipboard_sync: assembly.clipboard_sync.clone(),
        blob_transfer: assembly.blob.clone(),
        entry_repo: deps.clipboard.entry_ports.get.clone(),
        event_repo: wired.shared.clipboard_event_reader_repo.clone(),
        selection_repo: deps.clipboard.selection_repo.clone(),
        representation_repo: deps.clipboard.representation_ports.get.clone(),
        rep_processing_repo: deps
            .clipboard
            .representation_ports
            .update_processing_result
            .clone(),
        payload_resolver: deps.clipboard.payload_resolver.clone(),
        blob_store: deps.storage.blob_store.clone(),
        entry_delivery_repo: wired.shared.entry_delivery_repo.clone(),
        trusted_peer_repo: wired.shared.trusted_peer_repo.clone(),
        device_identity: deps.device.device_identity.clone(),
        entry_file_set_repo: deps.storage.entry_file_set_repo.clone(),
    }));

    let app_facade = build_app_facade_from_deps(
        deps,
        &storage_paths,
        lifecycle_status,
        AppFacadeAssemblyOptions {
            space_setup: Some(assembly.facade.clone()),
            member_roster: Some(assembly.roster.clone()),
            clipboard_sync: Some(assembly.clipboard_sync.clone()),
            blob_transfer: Some(assembly.blob.clone()),
            blob_transfer_port: Some(Arc::clone(&assembly.blob_transfer)),
            clipboard_outbound: Some(clipboard_outbound),
            file_transfer: Some(wired.shared.file_transfer_facade.clone()),
            search_coordinator: Some(search_coordinator),
            mobile_sync_apply_inbound: Some(mobile_sync_apply_inbound),
            ..Default::default()
        },
    );

    Ok(CliAppRuntime {
        app_facade,
        sync_engine_assembly: Some(assembly),
    })
}

/// Parse the desktop clipboard integration mode.
pub fn parse_clipboard_integration_mode(raw: Option<&str>) -> ClipboardIntegrationMode {
    let Some(raw_value) = raw else {
        return ClipboardIntegrationMode::Full;
    };

    let normalized = raw_value.trim();
    if normalized.eq_ignore_ascii_case("passive") {
        return ClipboardIntegrationMode::Passive;
    }
    if normalized.eq_ignore_ascii_case("full") {
        return ClipboardIntegrationMode::Full;
    }

    tracing::warn!(
        uc_clipboard_mode = %raw_value,
        "Invalid UC_CLIPBOARD_MODE value; falling back to full integration"
    );
    ClipboardIntegrationMode::Full
}

/// Resolve the clipboard integration mode from `UC_CLIPBOARD_MODE`.
pub fn resolve_clipboard_integration_mode() -> ClipboardIntegrationMode {
    let raw = std::env::var("UC_CLIPBOARD_MODE").ok();
    parse_clipboard_integration_mode(raw.as_deref())
}
