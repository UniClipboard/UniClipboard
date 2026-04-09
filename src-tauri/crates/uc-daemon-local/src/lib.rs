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

    /// Provide a shared global mutex for coordinating exclusive access between tests.
    ///
    /// The mutex is initialized once on first use and returned as a `'static` reference so tests
    /// can lock it to serialize access to global resources.
    ///
    /// # Examples
    ///
    /// ```
    /// let _guard = uc_daemon_local::test_env::lock().lock().unwrap();
    /// // perform test actions that require exclusive access...
    /// ```
    pub fn lock() -> &'static Mutex<()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }
}
