use std::sync::Mutex;

static UC_PROFILE_ENV_LOCK: Mutex<()> = Mutex::new(());

struct UcProfileEnvGuard {
    previous: Option<String>,
}

impl UcProfileEnvGuard {
    fn new(value: Option<&str>) -> Self {
        let previous = std::env::var("UC_PROFILE").ok();
        match value {
            Some(profile) => std::env::set_var("UC_PROFILE", profile),
            None => std::env::remove_var("UC_PROFILE"),
        }
        Self { previous }
    }
}

impl Drop for UcProfileEnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(profile) => std::env::set_var("UC_PROFILE", profile),
            None => std::env::remove_var("UC_PROFILE"),
        }
    }
}

pub fn with_uc_profile<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
    let _env_lock = UC_PROFILE_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _profile_guard = UcProfileEnvGuard::new(value);
    f()
}
