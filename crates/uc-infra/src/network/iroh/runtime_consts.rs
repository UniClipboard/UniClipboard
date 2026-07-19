//! Process-wide iroh runtime configuration.
//!
//! The active node owns these values through `NodeRunLease`. The state is
//! reset before that lease is released, so a replacement node cannot inherit a
//! previous node's bind-time configuration.

#[cfg(not(any(test, feature = "test-util")))]
use std::sync::Mutex;

#[cfg(not(any(test, feature = "test-util")))]
use tracing::warn;

#[cfg(not(any(test, feature = "test-util")))]
static LAN_ONLY: Mutex<bool> = Mutex::new(false);

/// Install the active node's LAN-only policy. Test multi-node harnesses do not
/// share a process-wide policy and therefore retain the no-op implementation.
pub(crate) fn install_lan_only(lan_only: bool) {
    #[cfg(not(any(test, feature = "test-util")))]
    {
        let mut current = LAN_ONLY.lock().unwrap_or_else(|poisoned| {
            warn!("iroh LAN-only runtime state lock poisoned while installing policy");
            poisoned.into_inner()
        });
        *current = lan_only;
    }
    #[cfg(any(test, feature = "test-util"))]
    {
        let _ = lan_only;
    }
}

/// Clear the active node's LAN-only policy before its runtime lease is
/// released. Test multi-node harnesses intentionally have no shared policy.
#[cfg(not(any(test, feature = "test-util")))]
pub(crate) fn clear_lan_only() {
    #[cfg(not(any(test, feature = "test-util")))]
    {
        let mut current = LAN_ONLY.lock().unwrap_or_else(|poisoned| {
            warn!("iroh LAN-only runtime state lock poisoned while clearing policy");
            poisoned.into_inner()
        });
        *current = false;
    }
}

/// Whether the active production node is in LAN-only mode. Test multi-node
/// harnesses always return false because no global node policy exists there.
pub(crate) fn lan_only() -> bool {
    #[cfg(not(any(test, feature = "test-util")))]
    {
        let current = LAN_ONLY.lock().unwrap_or_else(|poisoned| {
            warn!("iroh LAN-only runtime state lock poisoned while reading policy");
            poisoned.into_inner()
        });
        *current
    }
    #[cfg(any(test, feature = "test-util"))]
    {
        false
    }
}
