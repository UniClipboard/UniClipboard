use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use uc_engine::{EngineError, EngineErrorCategory};

use uc_engine_uniffi::{
    core_version, BindingConfig, BindingEngineState, BindingError, BindingErrorCategory,
    BindingEvent, BindingHost, BindingOperationTerminal, HostBindingError, InvitationIssued,
    MobileEngine, TextSendReport,
};

static ENGINE_TEST_LOCK: Mutex<()> = Mutex::new(());

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
    let _test_guard = engine_test_guard();
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

#[test]
fn dropping_a_running_binding_stops_its_worker() {
    let _test_guard = engine_test_guard();
    let (finished, completion) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let root = tempfile::tempdir().expect("temporary host root must be available");
        let host = Arc::new(MemoryHost::new(root.path()));
        let engine = MobileEngine::start(
            BindingConfig {
                app_version: "1.2.3".to_owned(),
                profile_id: "binding-drop".to_owned(),
            },
            host,
        )
        .expect("binding engine must start");

        drop(engine);
        let _ = finished.send(());
    });

    completion
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("dropping the binding must not leave its worker waiting on the event stream");
}

#[test]
fn mobile_host_observes_suspend_and_resume_state_events() {
    let _test_guard = engine_test_guard();
    let root = tempfile::tempdir().expect("temporary host root must be available");
    let host = Arc::new(MemoryHost::new(root.path()));
    let engine = MobileEngine::start(
        BindingConfig {
            app_version: "1.2.3".to_owned(),
            profile_id: "binding-lifecycle".to_owned(),
        },
        host,
    )
    .expect("binding engine must start");
    engine
        .create_space(
            Some("mobile-lifecycle-host".to_owned()),
            "correct horse battery staple".to_owned(),
        )
        .expect("binding must create a space before lifecycle transitions");

    wait_for_state(&engine, BindingEngineState::Running);
    engine.suspend().expect("binding engine must suspend");
    wait_for_state(&engine, BindingEngineState::Suspended);
    engine.resume().expect("binding engine must resume");
    wait_for_state(&engine, BindingEngineState::Running);

    engine
        .shutdown(5_000)
        .expect("binding engine must shut down within the deadline");
}

fn wait_for_state(engine: &MobileEngine, expected: BindingEngineState) {
    for _ in 0..50 {
        if let Some(BindingEvent::StateChanged { state }) = engine.next_event(100) {
            if state == expected {
                return;
            }
        }
    }
    panic!("binding did not deliver the expected state: {expected:?}");
}

#[test]
fn completed_engine_operations_are_available_as_structured_events() {
    let _test_guard = engine_test_guard();
    let root = tempfile::tempdir().expect("temporary host root must be available");
    let host = Arc::new(MemoryHost::new(root.path()));
    let engine = MobileEngine::start(
        BindingConfig {
            app_version: "1.2.3".to_owned(),
            profile_id: "binding-operation-events".to_owned(),
        },
        host,
    )
    .expect("binding engine must start");
    engine
        .create_space(
            Some("mobile-event-host".to_owned()),
            "correct horse battery staple".to_owned(),
        )
        .expect("binding must create a space");

    for _ in 0..50 {
        if let Some(BindingEvent::OperationFinished {
            operation_id,
            terminal,
            failure,
        }) = engine.next_event(100)
        {
            assert!(!operation_id.is_empty());
            assert_eq!(terminal, BindingOperationTerminal::Succeeded);
            assert_eq!(failure, None);
            engine
                .shutdown(5_000)
                .expect("binding engine must shut down within the deadline");
            return;
        }
    }
    panic!("binding did not deliver a completed-operation event");
}

#[test]
fn pairing_methods_return_invitation_data_and_stable_join_errors() {
    let _test_guard = engine_test_guard();
    let root = tempfile::tempdir().expect("temporary host root must be available");
    let host = Arc::new(MemoryHost::new(root.path()));
    let engine = MobileEngine::start(
        BindingConfig {
            app_version: "1.2.3".to_owned(),
            profile_id: "binding-pairing".to_owned(),
        },
        host,
    )
    .expect("binding engine must start");
    engine
        .create_space(
            Some("mobile-pairing-host".to_owned()),
            "correct horse battery staple".to_owned(),
        )
        .expect("binding must create a space");

    let invitation: InvitationIssued = engine
        .issue_invitation()
        .expect("binding must return a pairing invitation");
    assert!(!invitation.invitation_code.is_empty());
    assert!(invitation.expires_at_ms > 0);

    let error = engine
        .join_space(
            invitation.invitation_code,
            Some("  ".to_owned()),
            "correct horse battery staple".to_owned(),
        )
        .expect_err("blank device name must be rejected by the core");
    assert!(matches!(
        error,
        BindingError::Engine {
            category: BindingErrorCategory::InvalidInput,
            retryable: false,
            ..
        }
    ));

    engine
        .shutdown(5_000)
        .expect("binding engine must shut down within the deadline");
}

#[test]
fn text_send_returns_a_content_free_delivery_summary() {
    let _test_guard = engine_test_guard();
    let root = tempfile::tempdir().expect("temporary host root must be available");
    let host = Arc::new(MemoryHost::new(root.path()));
    let engine = MobileEngine::start(
        BindingConfig {
            app_version: "1.2.3".to_owned(),
            profile_id: "binding-send-text".to_owned(),
        },
        host,
    )
    .expect("binding engine must start");
    engine
        .create_space(
            Some("mobile-text-host".to_owned()),
            "correct horse battery staple".to_owned(),
        )
        .expect("binding must create a space");

    let report: TextSendReport = engine
        .send_text("private binding text".to_owned(), Vec::new())
        .expect("binding must send text through the core");
    assert!(!report.entry_id.is_empty());
    assert!(report.at_ms > 0);
    assert_eq!(report.total_accepted, 0);
    assert_eq!(report.total_duplicate, 0);
    assert_eq!(report.total_offline, 0);
    assert_eq!(report.total_errored, 0);
    assert_eq!(report.total_pending, 0);
    assert!(!format!("{report:?}").contains("private binding text"));

    engine
        .shutdown(5_000)
        .expect("binding engine must shut down within the deadline");
}

fn engine_test_guard() -> MutexGuard<'static, ()> {
    match ENGINE_TEST_LOCK.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
