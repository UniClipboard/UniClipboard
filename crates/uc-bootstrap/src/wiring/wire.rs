pub(crate) use uc_engine::internal::wire::{wire_dependencies_from_inputs, CoreWiringInputs};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use uc_core::app_dirs::AppPaths;
    use uc_core::ports::SecureStoragePort;
    use uc_platform::file_secure_storage::FileSecureStorage;

    use super::{wire_dependencies_from_inputs, CoreWiringInputs};
    use crate::layer::platform::{noop_system_clipboard_layer, SystemClipboardWiring};

    #[test]
    fn core_wiring_accepts_preselected_host_inputs() {
        let temp = tempfile::tempdir().expect("tempdir");
        let data_root = temp.path().join("data");
        let paths = AppPaths::with_base_data_local_dir(data_root.clone());
        let storage = FileSecureStorage::with_base_dir(temp.path().join("keyring"));

        let (wired, _background) = wire_dependencies_from_inputs(CoreWiringInputs {
            paths,
            secure_storage: Arc::new(storage) as Arc<dyn SecureStoragePort>,
            profile_id: uc_core::ids::ProfileId::from("default"),
            legacy_iroh_identity_dir: data_root.join("iroh-identity"),
            iroh_blob_store_dir: data_root.join("iroh-blobs"),
            system_clipboard: noop_system_clipboard_layer(),
            analytics_sink: Arc::new(uc_observability::analytics::NoopAnalyticsSink),
            analytics_facade: Arc::new(uc_observability::analytics::NoopAnalyticsFacade),
            host_event_emitter: Arc::new(crate::observability::host_event::LoggingHostEventEmitter),
        })
        .expect("core wiring");

        assert_eq!(wired.system_clipboard_wiring, SystemClipboardWiring::Noop);
    }
}
