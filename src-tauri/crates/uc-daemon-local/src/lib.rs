//! Local daemon runtime metadata and process coordination helpers.

pub mod auth;
#[cfg(feature = "sidecar-lifecycle")]
pub mod daemon_bootstrap;
#[cfg(feature = "sidecar-lifecycle")]
pub mod daemon_lifecycle;
pub mod process_metadata;
pub mod socket;

#[cfg(test)]
pub(crate) mod test_env {
    use std::sync::{Mutex, OnceLock};

    pub fn lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }
}
