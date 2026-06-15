//! Seam-1 gate check. MUST stay alone in its own integration-test file: it
//! relies on `uc_mobile_init()` never having run in this process, and every
//! other test calls it — a separate test binary is the only way to keep the
//! process-wide `OnceLock` untouched.

struct NoopBridge;
impl uc_mobile::PlatformBridge for NoopBridge {
    fn app_group_dir(&self) -> String {
        String::new()
    }
}

#[test]
fn constructor_requires_uc_mobile_init() {
    match uc_mobile::MobileSyncClient::new(std::sync::Arc::new(NoopBridge), false) {
        Err(err) => assert_eq!(err, uc_mobile::SyncError::NotInitialized),
        Ok(_) => panic!("constructor must fail before uc_mobile_init()"),
    }
}
