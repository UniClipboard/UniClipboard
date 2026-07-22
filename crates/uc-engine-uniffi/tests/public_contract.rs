use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use uc_engine::{EngineError, EngineErrorCategory};

use uc_engine_uniffi::{
    core_version, BindingConfig, BindingError, BindingErrorCategory, BindingHost, HostBindingError,
    MobileEngine,
};

#[test]
fn core_version_uses_the_binding_package_version() {
    assert_eq!(
        core_version(),
        format!("core-v{}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn engine_errors_keep_their_stable_code_category_and_retryability() {
    let cases = [
        (
            EngineErrorCategory::InvalidInput,
            BindingErrorCategory::InvalidInput,
        ),
        (
            EngineErrorCategory::InvalidState,
            BindingErrorCategory::InvalidState,
        ),
        (
            EngineErrorCategory::Unauthorized,
            BindingErrorCategory::Unauthorized,
        ),
        (
            EngineErrorCategory::NotFound,
            BindingErrorCategory::NotFound,
        ),
        (
            EngineErrorCategory::Conflict,
            BindingErrorCategory::Conflict,
        ),
        (
            EngineErrorCategory::Unavailable,
            BindingErrorCategory::Unavailable,
        ),
        (
            EngineErrorCategory::DeadlineExceeded,
            BindingErrorCategory::DeadlineExceeded,
        ),
        (
            EngineErrorCategory::Internal,
            BindingErrorCategory::Internal,
        ),
    ];

    for (index, (engine_category, binding_category)) in cases.into_iter().enumerate() {
        let code = 2000 + index as u32;
        let retryable = index % 2 == 0;
        assert_eq!(
            BindingError::from(EngineError::new(code, engine_category, retryable)),
            BindingError::Engine {
                code,
                category: binding_category,
                retryable,
            }
        );
    }
}

struct MemoryHost {
    private_data: PathBuf,
    cache: PathBuf,
    temporary: PathBuf,
    values: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryHost {
    fn new(root: &Path) -> Self {
        Self {
            private_data: root.join("data"),
            cache: root.join("cache"),
            temporary: root.join("temporary"),
            values: Mutex::new(HashMap::new()),
        }
    }

    fn values(&self) -> MutexGuard<'_, HashMap<String, Vec<u8>>> {
        match self.values.lock() {
            Ok(values) => values,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl BindingHost for MemoryHost {
    fn private_data_directory(&self) -> Result<String, HostBindingError> {
        Ok(self.private_data.to_string_lossy().into_owned())
    }

    fn cache_directory(&self) -> Result<String, HostBindingError> {
        Ok(self.cache.to_string_lossy().into_owned())
    }

    fn temporary_directory(&self) -> Result<String, HostBindingError> {
        Ok(self.temporary.to_string_lossy().into_owned())
    }

    fn secure_storage_get(&self, key: String) -> Result<Option<Vec<u8>>, HostBindingError> {
        Ok(self.values().get(&key).cloned())
    }

    fn secure_storage_set(&self, key: String, value: Vec<u8>) -> Result<(), HostBindingError> {
        self.values().insert(key, value);
        Ok(())
    }

    fn secure_storage_delete(&self, key: String) -> Result<(), HostBindingError> {
        self.values().remove(&key);
        Ok(())
    }
}

#[test]
fn mobile_host_can_create_a_space_and_shutdown_through_the_binding() {
    let root = tempfile::tempdir().expect("temporary host root must be available");
    let host = Arc::new(MemoryHost::new(root.path()));
    let engine = MobileEngine::start(
        BindingConfig {
            app_version: "1.2.3".to_owned(),
            profile_id: "binding-contract".to_owned(),
        },
        host.clone(),
    )
    .expect("binding engine must start");

    let created = engine
        .create_space(
            Some("mobile-contract-host".to_owned()),
            "correct horse battery staple".to_owned(),
        )
        .expect("binding must return the create-space result");
    assert!(!created.space_id.is_empty());
    assert!(!created.self_device_id.is_empty());
    assert!(!created.identity_fingerprint.is_empty());
    assert!(
        !host.values().is_empty(),
        "create-space must persist secrets through the host callback"
    );

    engine
        .shutdown(5_000)
        .expect("binding engine must shut down within the deadline");
}
